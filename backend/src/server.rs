use crate::{
    KeyStore, ProviderProtocol, Store,
    protocol::{ChatResponse, Protocol, StreamEvent, decode_request, decode_response},
    relay::{json_response, stream_response},
};
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Instant};
use tower_http::compression::CompressionLayer;

#[derive(Default)]
pub struct KeyHealth {
    pub cooldowns: std::collections::HashMap<i64, chrono::DateTime<chrono::Utc>>,
}
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub keys: KeyStore,
    /// Shared outbound client cached with the connect budget it was built
    /// for; reqwest only exposes connect_timeout at client level.
    pub http: std::sync::Arc<std::sync::Mutex<(u64, reqwest::Client)>>,
    pub bootstrap_token: String,
    pub data_directory: std::path::PathBuf,
    pub frontend_directory: std::path::PathBuf,
    pub snapshots: std::sync::Arc<crate::snapshot::Snapshots>,
    pub key_health: std::sync::Arc<tokio::sync::Mutex<KeyHealth>>,
    pub secure_cookie: bool,
    /// Deployment sits behind a trusted reverse proxy/port mapping, so
    /// `X-Forwarded-For`/`X-Real-IP` are authoritative for the client address.
    pub trusted_proxy: bool,
    pub metadata_source: String,
    /// Optional LiteLLM price table used to fill price/context gaps; empty
    /// disables the calibration layer.
    pub litellm_source: String,
    /// Optional OpenRouter-style catalog merged over the primary source for
    /// fresher live pricing; empty disables the overlay.
    pub openrouter_source: String,
    pub plugins: crate::plugin::SharedRuntime,
    pub routing_stats: std::sync::Arc<crate::strategy::RoutingStats>,
    /// Workspace change fan-out for the console SSE stream.
    pub events: crate::events::Events,
}
impl AppState {
    /// The shared outbound client, rebuilt only when the configured TCP
    /// connect budget changes. Cheap to call per attempt.
    pub fn http(&self) -> reqwest::Client {
        let connect = self.snapshots.get().connect_timeout_seconds;
        {
            let cached = self.http.lock().unwrap();
            if cached.0 == connect {
                return cached.1.clone();
            }
        }
        // Build outside the lock: client construction is not instantaneous.
        match crate::relay::protected_http_client(connect) {
            Ok(client) => {
                *self.http.lock().unwrap() = (connect, client.clone());
                client
            }
            // A builder failure cannot realistically happen with these
            // options; keep serving with the previous client.
            Err(message) => {
                eprintln!("outbound client rebuild failed: {message}");
                self.http.lock().unwrap().1.clone()
            }
        }
    }
    /// Marks a request finished before any upstream attempt was recorded and
    /// notifies open consoles. The pre-relay rejection paths use this instead
    /// of `store.finish_request` so the request list refreshes live.
    pub async fn finish_request(&self, request_id: &str, status: &str) {
        let _ = self.store.finish_request(request_id, status, 0, 0, 0).await;
        self.events.publish(crate::events::REQUESTS_CHANGED);
    }
    /// Records an intermediate status that the same request overwrites moments
    /// later (route selection). It stays silent because a console refresh for
    /// a status nobody observes is pure noise; every terminal or rejecting
    /// status still goes through `finish_request`.
    pub async fn mark_routing(&self, request_id: &str) {
        let _ = self
            .store
            .finish_request(request_id, "routing", 0, 0, 0)
            .await;
    }
    pub async fn for_testing(store: Store) -> Self {
        Self {
            store: store.clone(),
            keys: KeyStore::testing(),
            http: std::sync::Arc::new(std::sync::Mutex::new((
                10,
                crate::relay::protected_http_client(10).unwrap(),
            ))),
            bootstrap_token: "test".into(),
            data_directory: std::env::temp_dir(),
            frontend_directory: "frontend/dist".into(),
            snapshots: std::sync::Arc::new(crate::snapshot::Snapshots::load(&store).await.unwrap()),
            key_health: Default::default(),
            secure_cookie: false,
            trusted_proxy: false,
            metadata_source: crate::metadata::DEFAULT_SOURCE.into(),
            litellm_source: String::new(),
            openrouter_source: String::new(),
            plugins: crate::plugin::shared(crate::plugin::PluginRuntime::new().unwrap()),
            routing_stats: crate::strategy::RoutingStats::shared(),
            events: Default::default(),
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub enum InboundProtocol {
    Provider(Protocol),
    Gemini {
        model: String,
        stream: bool,
        credential: Option<String>,
    },
    OpenAIResponses,
}

pub fn candidate_protocols(provider: &crate::Provider, inbound: &InboundProtocol) -> Vec<Protocol> {
    if provider.plugin_id.is_some() {
        return vec![crate::relay::outbound_protocol(provider)];
    }
    match provider.protocol {
        ProviderProtocol::OpenAI => {
            if matches!(inbound, InboundProtocol::OpenAIResponses) {
                vec![Protocol::OpenAIResponses, Protocol::OpenAI]
            } else {
                vec![Protocol::OpenAI, Protocol::OpenAIResponses]
            }
        }
        ProviderProtocol::OpenAIResponses => vec![Protocol::OpenAIResponses, Protocol::OpenAI],
        ProviderProtocol::Anthropic => vec![Protocol::Anthropic],
        ProviderProtocol::Gemini => vec![Protocol::Gemini],
    }
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(openai))
        .route("/v1/responses", post(openai_responses))
        .merge(crate::response_api::router())
        .merge(crate::auxiliary::router())
        .route("/v1beta/models", get(gemini_models))
        .route("/v1/messages", post(anthropic))
        .route("/v1beta/models/{*path}", post(gemini))
        .route("/v1/models/{*path}", post(gemini))
        .nest("/api/v1", crate::admin::router(state.clone()))
        .layer(CompressionLayer::new().gzip(true))
        .fallback_service(
            tower_http::services::ServeDir::new(&state.frontend_directory).fallback(
                tower_http::services::ServeFile::new(state.frontend_directory.join("index.html")),
            ),
        )
        .with_state(state)
}
pub fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":{"message":message.into()}}))).into_response()
}
#[derive(Clone, Copy)]
pub enum ProtocolCredential {
    OpenAI,
    Anthropic,
    Gemini,
}
pub fn extract_credential(
    headers: &HeaderMap,
    protocol: ProtocolCredential,
    query: Option<&str>,
) -> Result<String, StatusCode> {
    match protocol {
        ProtocolCredential::OpenAI => headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::to_owned)
            .ok_or(StatusCode::UNAUTHORIZED),
        ProtocolCredential::Anthropic => {
            if headers
                .get("anthropic-version")
                .and_then(|value| value.to_str().ok())
                != Some("2023-06-01")
            {
                return Err(StatusCode::BAD_REQUEST);
            }
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
                .ok_or(StatusCode::UNAUTHORIZED)
        }
        ProtocolCredential::Gemini => headers
            .get("x-goog-api-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .or_else(|| {
                query.and_then(|query| {
                    url::form_urlencoded::parse(query.as_bytes())
                        .find(|(key, _)| key == "key")
                        .map(|(_, value)| value.into_owned())
                })
            })
            .ok_or(StatusCode::UNAUTHORIZED),
    }
}
fn credential(headers: &HeaderMap, protocol: Protocol) -> Option<String> {
    match protocol {
        Protocol::OpenAI | Protocol::OpenAIResponses => headers
            .get("authorization")?
            .to_str()
            .ok()?
            .strip_prefix("Bearer ")
            .map(str::to_owned),
        Protocol::Anthropic => Some(headers.get("x-api-key")?.to_str().ok()?.to_owned()),
        Protocol::Gemini => headers
            .get("x-goog-api-key")?
            .to_str()
            .ok()
            .map(str::to_owned),
    }
}
pub(crate) fn route_for_model<'a>(
    routes: &'a [crate::Route],
    model: &str,
) -> Option<&'a crate::Route> {
    routes
        .iter()
        .find(|route| route.slug == model)
        .or_else(|| {
            routes.iter().find(|route| {
                route
                    .candidates
                    .iter()
                    .any(|candidate| candidate.1 == model)
            })
        })
        .or_else(|| {
            routes
                .iter()
                .find(|route| route.patterns.iter().any(|pattern| glob(pattern, model)))
        })
        .or_else(|| routes.iter().find(|route| route.is_default))
}
pub(crate) fn model_allowed(
    client: &crate::ClientKey,
    routes: &[crate::Route],
    model: &str,
) -> bool {
    if client.models.iter().any(|pattern| glob(pattern, model)) {
        return true;
    }
    route_for_model(routes, model).is_some_and(|route| {
        route.candidates.iter().any(|candidate| {
            client
                .models
                .iter()
                .any(|pattern| glob(pattern, &candidate.1))
        })
    })
}
fn public_model_ids(
    client: &crate::ClientKey,
    snapshot: &crate::snapshot::ConfigSnapshot,
) -> Vec<String> {
    let mut models = Vec::new();
    for route in &snapshot.routes {
        let candidate_access = route.candidates.iter().any(|candidate| {
            client
                .models
                .iter()
                .any(|pattern| glob(pattern, &candidate.1))
        });
        if !candidate_access
            && !client
                .models
                .iter()
                .any(|pattern| glob(pattern, &route.slug))
        {
            continue;
        }
        models.push(route.slug.clone());
        if route.list_members {
            models.extend(
                route
                    .candidates
                    .iter()
                    .map(|candidate| candidate.1.clone())
                    .filter(|model| client.models.iter().any(|pattern| glob(pattern, model))),
            );
        }
    }
    models.sort();
    models.dedup();
    models
}
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let gemini = headers.contains_key("x-goog-api-key");
    let anthropic = !gemini && headers.contains_key("x-api-key");
    let protocol = if gemini {
        Protocol::Gemini
    } else if anthropic {
        Protocol::Anthropic
    } else {
        Protocol::OpenAI
    };
    let Some(token) = credential(&headers, protocol) else {
        return error(StatusCode::UNAUTHORIZED, "missing API key");
    };
    if anthropic
        && headers.contains_key("anthropic-version")
        && let Err(status) = extract_credential(&headers, ProtocolCredential::Anthropic, None)
    {
        return error(status, "unsupported anthropic-version");
    }
    let Ok(Some(client)) = state.store.authenticate_client(&token).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let data = public_model_ids(&client, &state.snapshots.get())
        .into_iter()
        .map(|model| {
            if gemini {
                json!({"name":format!("models/{model}"),"displayName":model,"supportedGenerationMethods":["generateContent","streamGenerateContent"]})
            } else if anthropic {
                json!({"id":model,"type":"model","display_name":model})
            } else {
                json!({"id":model,"object":"model","owned_by":"sepia"})
            }
        })
        .collect::<Vec<_>>();
    Json(if gemini {
        json!({"models":data})
    } else if anthropic {
        json!({"data":data,"has_more":false})
    } else {
        json!({"object":"list","data":data})
    })
    .into_response()
}
async fn gemini_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let credential_query = query.get("key").map(|key| format!("key={key}"));
    let token = match extract_credential(
        &headers,
        ProtocolCredential::Gemini,
        credential_query.as_deref(),
    ) {
        Ok(token) => token,
        Err(status) => return error(status, "missing API key"),
    };
    let Ok(Some(client)) = state.store.authenticate_client(&token).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let models = public_model_ids(&client, &state.snapshots.get()).into_iter().map(|model| json!({"name":format!("models/{model}"),"displayName":model,"supportedGenerationMethods":["generateContent","streamGenerateContent"]})).collect::<Vec<_>>();
    Json(json!({"models":models})).into_response()
}
async fn openai(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::connect_info::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>,
    Json(body): Json<Value>,
) -> Response {
    relay(
        state,
        headers,
        InboundProtocol::Provider(Protocol::OpenAI),
        peer.ip(),
        body,
        None,
    )
    .await
}
async fn openai_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::connect_info::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>,
    Json(body): Json<Value>,
) -> Response {
    crate::response_api::create_response(State(state), headers, peer.ip(), body).await
}
async fn anthropic(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::connect_info::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>,
    Json(body): Json<Value>,
) -> Response {
    relay(
        state,
        headers,
        InboundProtocol::Provider(Protocol::Anthropic),
        peer.ip(),
        body,
        None,
    )
    .await
}
async fn gemini(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(path): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    axum::extract::connect_info::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>,
    Json(body): Json<Value>,
) -> Response {
    let Some((model, action)) = path.split_once(':') else {
        return error(StatusCode::NOT_FOUND, "unknown Gemini action");
    };
    if action != "generateContent" && action != "streamGenerateContent" {
        return error(StatusCode::NOT_FOUND, "unknown Gemini action");
    }
    let credential_query = query.get("key").map(|key| format!("key={key}"));
    relay(
        state,
        headers,
        InboundProtocol::Gemini {
            model: model.to_string(),
            stream: action == "streamGenerateContent",
            credential: credential_query,
        },
        peer.ip(),
        body,
        None,
    )
    .await
}
pub struct ResponseLifecycle {
    pub client_key: i64,
    pub id: String,
}
pub async fn relay(
    state: AppState,
    headers: HeaderMap,
    inbound: InboundProtocol,
    peer: std::net::IpAddr,
    body: Value,
    response_state: Option<ResponseLifecycle>,
) -> Response {
    // The recorded IP and the policy whitelist must agree, so both read this
    // one resolved value instead of the raw transport peer.
    let peer = crate::client_ip::client_ip(&headers, peer, state.trusted_proxy);
    let path = match &inbound {
        InboundProtocol::Gemini { model, stream, .. } => Some((model.as_str(), *stream)),
        _ => None,
    };
    let query = match &inbound {
        InboundProtocol::Gemini { credential, .. } => credential.as_deref(),
        _ => None,
    };
    let protocol = match &inbound {
        InboundProtocol::Provider(protocol) => *protocol,
        InboundProtocol::Gemini { .. } => Protocol::Gemini,
        InboundProtocol::OpenAIResponses => Protocol::OpenAI,
    };
    let protocol_credential = match protocol {
        Protocol::OpenAI | Protocol::OpenAIResponses => ProtocolCredential::OpenAI,
        Protocol::Anthropic => ProtocolCredential::Anthropic,
        Protocol::Gemini => ProtocolCredential::Gemini,
    };
    let token = match extract_credential(&headers, protocol_credential, query) {
        Ok(token) => token,
        Err(status) => return error(status, "invalid or missing API credentials"),
    };
    let client = match state.store.authenticate_client(&token).await {
        Ok(Some(client)) => client,
        _ => return error(StatusCode::UNAUTHORIZED, "invalid API key"),
    };
    let policy = match state.store.client_policy(client.id).await {
        Ok(policy) => policy,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "client policy unavailable",
            );
        }
    };
    if !Store::ip_allowed(&policy, peer) {
        return error(StatusCode::FORBIDDEN, "client IP is not allowed");
    }
    let initial_model = path.map(|path| path.0).unwrap_or("").to_string();
    let request_id = match if let Some(lifecycle) = &response_state {
        state
            .store
            .begin_request_with_id(
                &lifecycle.id,
                client.id,
                match protocol {
                    Protocol::OpenAI => ProviderProtocol::OpenAI,
                    Protocol::OpenAIResponses => ProviderProtocol::OpenAIResponses,
                    Protocol::Anthropic => ProviderProtocol::Anthropic,
                    Protocol::Gemini => ProviderProtocol::Gemini,
                },
                &initial_model,
            )
            .await
    } else {
        state
            .store
            .begin_request(
                client.id,
                match protocol {
                    Protocol::OpenAI => ProviderProtocol::OpenAI,
                    Protocol::OpenAIResponses => ProviderProtocol::OpenAIResponses,
                    Protocol::Anthropic => ProviderProtocol::Anthropic,
                    Protocol::Gemini => ProviderProtocol::Gemini,
                },
                &initial_model,
            )
            .await
    } {
        Ok(id) => id,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "observability unavailable",
            );
        }
    };
    let _ = state
        .store
        .set_request_client_ip(&request_id, &peer.to_string())
        .await;
    let inbound_protocol = match inbound {
        InboundProtocol::Provider(protocol) => protocol,
        InboundProtocol::Gemini { .. } => Protocol::Gemini,
        InboundProtocol::OpenAIResponses => Protocol::OpenAIResponses,
    };
    let mut response_context = None;
    if state.snapshots.get().log_bodies {
        let _ = state
            .store
            .save_request_body(&request_id, &crate::relay::capped_body(&body.to_string()))
            .await;
    }
    let decoded_request = match &inbound {
        InboundProtocol::OpenAIResponses => crate::responses::decode_request_with_context(body)
            .map(|(request, context)| {
                response_context = Some(context);
                request
            }),
        InboundProtocol::Provider(protocol) => decode_request(*protocol, body, None),
        InboundProtocol::Gemini { model, .. } => {
            decode_request(Protocol::Gemini, body, Some(model))
        }
    };
    let mut request = match decoded_request {
        Ok(request) => request,
        Err(message) => {
            state.finish_request(&request_id, "invalid").await;
            return error(StatusCode::BAD_REQUEST, message);
        }
    };
    if path.is_some_and(|p| p.1) {
        request.stream = true;
    }
    match crate::plugin::rewrite_request(&state, &request).await {
        Ok(rewritten) => request = rewritten,
        Err(message) => {
            state.finish_request(&request_id, "failed").await;
            return error(StatusCode::BAD_GATEWAY, message);
        }
    }
    let snapshot = state.snapshots.get();
    if !model_allowed(&client, &snapshot.routes, &request.model) {
        state.finish_request(&request_id, "denied").await;
        return error(StatusCode::FORBIDDEN, "model is not allowed by this key");
    }
    state.mark_routing(&request_id).await;
    let started = Instant::now();
    let Some(route) = route_for_model(&snapshot.routes, &request.model) else {
        state.finish_request(&request_id, "no_route").await;
        return error(StatusCode::NOT_FOUND, "no route for model");
    };
    let candidates =
        match crate::plugin::filter_candidates(&state, &request.model, &route.candidates).await {
            Ok(candidates) => candidates,
            Err(message) => {
                state.finish_request(&request_id, "failed").await;
                return error(StatusCode::BAD_GATEWAY, message);
            }
        };
    // Plugin filters run first (they may exclude candidates); the built-in
    // strategy orders the surviving list. `priority` keeps plugin output order.
    // Weights follow candidate identity, not position: plugins may reorder
    // the surviving list, so each candidate keeps its saved weight.
    let weights: Vec<i64> = candidates
        .iter()
        .map(|candidate| {
            route
                .candidates
                .iter()
                .position(|saved| saved == candidate)
                .and_then(|index| route.weights.get(index).copied())
                .unwrap_or(1)
        })
        .collect();
    let candidates = crate::strategy::order(
        &route.strategy,
        route.id,
        &candidates,
        &weights,
        &state.routing_stats,
        &crate::strategy::OrderContext {
            route_id: route.id,
            prices: &snapshot.prices,
            contexts: &snapshot.contexts,
            estimated_tokens: crate::strategy::estimate_tokens(
                request.system.as_deref(),
                &request.messages,
            ),
        },
    );
    let quota_model = route
        .candidates
        .first()
        .map(|candidate| candidate.1.as_str())
        .unwrap_or(&request.model);
    if state
        .store
        .set_request_route(&request_id, &route.slug)
        .await
        .is_err()
    {
        state.finish_request(&request_id, "failed").await;
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "observability unavailable",
        );
    }
    let reservation = match state
        .store
        .reserve_quota(
            client.id,
            quota_model,
            request.max_tokens.unwrap_or(1024) as i64,
        )
        .await
    {
        Ok(id) => id,
        Err(message) if message.contains("quota") || message.contains("pricing") => {
            state.finish_request(&request_id, "quota_denied").await;
            return error(StatusCode::TOO_MANY_REQUESTS, message);
        }
        Err(_) => {
            state.finish_request(&request_id, "failed").await;
            return error(StatusCode::INTERNAL_SERVER_ERROR, "quota unavailable");
        }
    };
    let guard = crate::lifecycle::RequestGuard::new(
        state.store.clone(),
        request_id.clone(),
        reservation,
        state.events.clone(),
    );
    let providers = &snapshot.providers;
    let mut attempts_used = 0;
    'candidates: for (provider_id, model) in &candidates {
        let Some(provider) = providers
            .iter()
            .find(|provider| provider.id == *provider_id)
        else {
            continue;
        };
        if !state
            .store
            .provider_allowed(client.id, provider.id)
            .await
            .unwrap_or(false)
        {
            continue;
        }
        // Unified-channel providers fail over across their addresses; a
        // single-channel provider has exactly one address, so the loop below
        // behaves exactly like the address-less path.
        let addresses = match state.store.provider_endpoints(provider.id).await {
            Ok(addresses) if !addresses.is_empty() => addresses,
            _ => vec![crate::store::ProviderEndpoint {
                id: 0,
                endpoint: provider.endpoint.clone(),
            }],
        };
        let protocols = candidate_protocols(provider, &inbound);
        'addresses: for (address_index, address) in addresses.iter().enumerate() {
            let last_address = address_index + 1 == addresses.len();
            let target = crate::Provider {
                endpoint: address.endpoint.clone(),
                ..provider.clone()
            };
            'legs: for (leg, protocol) in protocols.iter().enumerate() {
                let last_leg = leg + 1 == protocols.len();
                let endpoint = crate::relay::endpoint_label(*protocol);
                // Auth failures rotate keys within this leg only, bounded by the
                // number of keys so a fully unauthorized provider still reaches
                // the other endpoint and later candidates. Each leg restarts at
                // the provider's first key.
                let mut key_attempts = 0usize;
                let key_count = state
                    .store
                    .provider_keys(provider.id)
                    .await
                    .unwrap_or_default()
                    .iter()
                    .filter(|key| {
                        key.endpoint_id
                            .is_none_or(|endpoint_id| endpoint_id == address.id)
                            && key
                                .models
                                .as_ref()
                                .is_none_or(|models| models.iter().any(|item| item == model))
                    })
                    .count();
                loop {
                    if let Some(lifecycle) = &response_state
                        && state
                            .store
                            .response(lifecycle.client_key, &lifecycle.id)
                            .await
                            .ok()
                            .flatten()
                            .is_some_and(|response| response.cancel_requested)
                    {
                        let _ = guard.complete("cancelled", 0, 0, 0).await;
                        return error(StatusCode::CONFLICT, "response was cancelled");
                    }
                    if attempts_used >= snapshot.max_attempts {
                        break 'candidates;
                    }
                    let (key_id, secret) = {
                        let health = state.key_health.lock().await;
                        match state
                            .store
                            .upstream_key_for_attempt(
                                provider.id,
                                Some(address.id),
                                model,
                                key_attempts,
                                &health.cooldowns,
                                &state.keys,
                            )
                            .await
                        {
                            Ok(Some(key)) => key,
                            _ => {
                                if last_leg {
                                    continue 'addresses;
                                }
                                continue 'legs;
                            }
                        }
                    };
                    attempts_used += 1;
                    let attempt_started = Instant::now();
                    let inflight = state.routing_stats.inflight_slot(provider.id);
                    // Built-in provider+route rewriting runs before the plugin
                    // upstream hook: prompt rules touch only system text, param
                    // rules rewrite the encoded body, and the effective upstream
                    // model drives the Gemini URL path. Key scopes, logs, strategy
                    // stats and quotas keep the original candidate model.
                    let rewrite = crate::rewrite::merge(&provider.rewrite, &route.rewrite);
                    let mut rewritten_request = request.clone();
                    crate::rewrite::apply_prompt(&mut rewritten_request, &rewrite.prompt_replace);
                    // Gemini carries the model in the URL path, not the body, so
                    // its `model` rules have to run on the model string itself;
                    // every other protocol rewrites the encoded body field.
                    let gemini_model = (*protocol == Protocol::Gemini)
                        .then(|| crate::rewrite::effective_model_for(model, &rewrite));
                    let encode_model = gemini_model.as_deref().unwrap_or(model);
                    let mut encoded_body = match crate::protocol::encode_request(
                        *protocol,
                        &rewritten_request,
                        encode_model,
                    ) {
                        Ok(body) => body,
                        Err(message) => {
                            let _ = guard.complete("failed", 0, 0, 0).await;
                            return error(StatusCode::BAD_GATEWAY, message);
                        }
                    };
                    crate::rewrite::apply_params(&mut encoded_body, &rewrite, *protocol);
                    let effective_model = match gemini_model {
                        Some(rewritten) => rewritten,
                        None => crate::rewrite::effective_model(&encoded_body, model),
                    };
                    let (out_body, plugin_headers) = match crate::plugin::rewrite_upstream(
                        &state,
                        &target,
                        model,
                        &encoded_body,
                        &[],
                    )
                    .await
                    {
                        Ok(value) => value,
                        Err(message) => {
                            let _ = guard.complete("failed", 0, 0, 0).await;
                            return error(StatusCode::BAD_GATEWAY, message);
                        }
                    };
                    let out_headers = crate::rewrite::merge_headers(
                        crate::rewrite::header_pairs(&rewrite),
                        plugin_headers,
                    );
                    let upstream = if provider.plugin_id.is_some() {
                        crate::plugin::provider_fetch(
                            &state,
                            &target,
                            &secret,
                            &rewritten_request,
                            &effective_model,
                            *protocol,
                            &rewrite,
                            &out_headers,
                        )
                        .await
                    } else {
                        crate::relay::call_with_overrides(
                            &state.http(),
                            &target,
                            &secret,
                            &rewritten_request,
                            &effective_model,
                            *protocol,
                            &crate::relay::Overrides {
                                body: Some(out_body),
                                headers: out_headers,
                            },
                            crate::relay::Timeouts {
                                first_byte_seconds: snapshot.first_byte_timeout_seconds,
                                stream_idle_seconds: snapshot.stream_idle_timeout_seconds,
                            },
                        )
                        .await
                    };
                    let response =
                        match upstream {
                            Ok(response) => response,
                            Err(message) => {
                                state.routing_stats.record(
                                    route.id,
                                    provider.id,
                                    model,
                                    false,
                                    attempt_started.elapsed().as_millis() as i64,
                                );
                                state.key_health.lock().await.cooldowns.insert(
                                    key_id,
                                    chrono::Utc::now() + chrono::Duration::seconds(60),
                                );
                                if let Some(response) = record_attempt(
                                    &state,
                                    crate::observability::NewAttempt {
                                        request_id: &request_id,
                                        provider_id: Some(provider.id),
                                        provider_key_id: Some(key_id),
                                        model,
                                        status: "retry",
                                        error: Some(&format!("{message} ({endpoint})")),
                                        input_tokens: 0,
                                        output_tokens: 0,
                                        latency_ms: attempt_started.elapsed().as_millis() as i64,
                                    },
                                )
                                .await
                                {
                                    return response;
                                }
                                if !last_leg {
                                    continue 'legs;
                                }
                                continue 'addresses;
                            }
                        };
                    let first_byte_ms = attempt_started.elapsed().as_millis() as i64;
                    if !response.status().is_success() {
                        state.routing_stats.record(
                            route.id,
                            provider.id,
                            model,
                            false,
                            first_byte_ms,
                        );
                    }
                    if !response.status().is_success() {
                        let status = response.status();
                        let retry = status == StatusCode::TOO_MANY_REQUESTS
                            || status == StatusCode::REQUEST_TIMEOUT
                            || matches!(status.as_u16(), 500 | 502 | 503 | 504)
                            || status == StatusCode::UNAUTHORIZED
                            || status == StatusCode::FORBIDDEN;
                        let endpoint_unsupported = matches!(status.as_u16(), 404 | 405 | 501);
                        if let Some(response) = record_attempt(
                            &state,
                            crate::observability::NewAttempt {
                                request_id: &request_id,
                                provider_id: Some(provider.id),
                                provider_key_id: Some(key_id),
                                model,
                                status: if retry
                                    || !last_leg
                                    || !last_address
                                    || endpoint_unsupported
                                {
                                    "retry"
                                } else {
                                    "failed"
                                },
                                error: Some(&format!("upstream HTTP {status} ({endpoint})")),
                                input_tokens: 0,
                                output_tokens: 0,
                                latency_ms: attempt_started.elapsed().as_millis() as i64,
                            },
                        )
                        .await
                        {
                            return response;
                        }
                        if retry {
                            if status == StatusCode::TOO_MANY_REQUESTS {
                                state.key_health.lock().await.cooldowns.insert(
                                    key_id,
                                    chrono::Utc::now() + chrono::Duration::seconds(60),
                                );
                                let delay = response
                                    .headers()
                                    .get("retry-after")
                                    .and_then(|value| value.to_str().ok())
                                    .and_then(|value| value.parse::<u64>().ok())
                                    .unwrap_or(0)
                                    .min(snapshot.retry_after_cap_seconds);
                                if delay > 0 {
                                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                                }
                            }
                            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
                            {
                                key_attempts += 1;
                                if key_count > 0 && key_attempts < key_count {
                                    continue;
                                }
                                if !last_leg {
                                    continue 'legs;
                                }
                                continue 'addresses;
                            }
                            if !last_leg {
                                continue 'legs;
                            }
                            continue 'addresses;
                        }
                        // An unsupported endpoint often surfaces as 404/405/501; try
                        // the other leg, then later candidates, before failing terminally.
                        if !last_leg {
                            continue 'legs;
                        }
                        if endpoint_unsupported {
                            continue 'addresses;
                        }
                        let _ = guard.complete("failed", 0, 0, 0).await;
                        return error(StatusCode::BAD_GATEWAY, "upstream request failed");
                    }
                    crate::plugin::observe_completion(
                        &state,
                        json!({
                            "request_id": request_id,
                            "model": model,
                            "provider_id": provider.id,
                            "success": true,
                            "stream": request.stream,
                        }),
                    );
                    if request.stream {
                        let start = StreamEvent::Start {
                            id: request_id.clone(),
                            model: request.model.clone(),
                        };
                        let stream = stream_response(
                            inbound,
                            *protocol,
                            response,
                            start,
                            crate::relay::StreamContext {
                                store: state.store.clone(),
                                routing_stats: state.routing_stats.clone(),
                                route_id: route.id,
                                request_id,
                                provider_id: provider.id,
                                provider_key_id: key_id,
                                model: model.clone(),
                                guard,
                                started,
                                first_byte_ms,
                                inflight: Some(inflight),
                                stream_idle_seconds: snapshot.stream_idle_timeout_seconds,
                                log_bodies: snapshot.log_bodies,
                                inbound_protocol,
                                response_context,
                                response_state: response_state
                                    .map(|lifecycle| (lifecycle.client_key, lifecycle.id)),
                            },
                        );
                        return stream;
                    }
                    let value = match response.json().await {
                        Ok(value) => value,
                        Err(_) => {
                            state.routing_stats.record(
                                route.id,
                                provider.id,
                                model,
                                false,
                                first_byte_ms,
                            );
                            if let Some(response) = record_attempt(
                                &state,
                                crate::observability::NewAttempt {
                                    request_id: &request_id,
                                    provider_id: Some(provider.id),
                                    provider_key_id: Some(key_id),
                                    model,
                                    status: "retry",
                                    error: Some(&format!("invalid upstream JSON ({endpoint})")),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    latency_ms: attempt_started.elapsed().as_millis() as i64,
                                },
                            )
                            .await
                            {
                                return response;
                            }
                            if !last_leg {
                                continue 'legs;
                            }
                            if !last_address {
                                continue 'addresses;
                            }
                            continue;
                        }
                    };
                    let value = match crate::plugin::rewrite_response(&state, model, &value).await {
                        Ok(value) => value,
                        Err(message) => {
                            state.routing_stats.record(
                                route.id,
                                provider.id,
                                model,
                                false,
                                first_byte_ms,
                            );
                            let _ = guard.complete("failed", 0, 0, 0).await;
                            return error(StatusCode::BAD_GATEWAY, message);
                        }
                    };
                    let decoded: ChatResponse = match decode_response(*protocol, value) {
                        Ok(response) => response,
                        Err(_) => {
                            state.routing_stats.record(
                                route.id,
                                provider.id,
                                model,
                                false,
                                first_byte_ms,
                            );
                            if !last_leg {
                                continue 'legs;
                            }
                            if !last_address {
                                continue 'addresses;
                            }
                            continue;
                        }
                    };
                    // Success telemetry only after the body decoded: headers alone
                    // must not mark the candidate healthy or sticky.
                    state
                        .routing_stats
                        .record(route.id, provider.id, model, true, first_byte_ms);
                    let _ = state
                        .store
                        .set_request_details(
                            &request_id,
                            first_byte_ms,
                            decoded.cached_tokens,
                            decoded.reasoning_tokens,
                        )
                        .await;
                    if let Some(response) = record_attempt(
                        &state,
                        crate::observability::NewAttempt {
                            request_id: &request_id,
                            provider_id: Some(provider.id),
                            provider_key_id: Some(key_id),
                            model,
                            status: "success",
                            error: None,
                            input_tokens: decoded.input_tokens,
                            output_tokens: decoded.output_tokens,
                            latency_ms: attempt_started.elapsed().as_millis() as i64,
                        },
                    )
                    .await
                    {
                        return response;
                    }
                    let cost = state
                        .store
                        .model_cost(
                            model,
                            decoded.input_tokens,
                            decoded.output_tokens,
                            decoded.cached_tokens,
                            *protocol != Protocol::Anthropic,
                        )
                        .await
                        .unwrap_or(0);
                    if guard
                        .complete("success", decoded.input_tokens, decoded.output_tokens, cost)
                        .await
                        .is_err()
                    {
                        return error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "request settlement unavailable",
                        );
                    }
                    return json_response(
                        inbound,
                        ChatResponse {
                            id: request_id.clone(),
                            ..decoded
                        },
                        response_context.as_ref(),
                        response_state
                            .as_ref()
                            .map(|lifecycle| (lifecycle.client_key, lifecycle.id.clone())),
                        &state,
                    )
                    .await;
                }
            }
        }
    }
    let _ = guard.complete("failed", 0, 0, 0).await;
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "no upstream candidate available",
    )
}
async fn record_attempt(
    state: &AppState,
    attempt: crate::observability::NewAttempt<'_>,
) -> Option<Response> {
    state.store.record_attempt(attempt).await.err().map(|_| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "observability unavailable",
        )
    })
}
pub fn glob(pattern: &str, value: &str) -> bool {
    let (pattern, value) = (pattern.as_bytes(), value.as_bytes());
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && pattern[p] == value[v] {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(position) = star {
            retry += 1;
            v = retry;
            p = position + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}
