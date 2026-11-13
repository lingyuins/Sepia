use crate::{
    KeyStore, ProviderProtocol, Store,
    protocol::{ChatResponse, Protocol, StreamEvent, decode_request, decode_response},
    relay::{call, json_response, outbound_protocol, stream_response},
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

#[derive(Default)]
pub struct KeyHealth {
    pub next: std::collections::HashMap<i64, usize>,
    pub cooldowns: std::collections::HashMap<i64, chrono::DateTime<chrono::Utc>>,
}
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub keys: KeyStore,
    pub http: reqwest::Client,
    pub bootstrap_token: String,
    pub data_directory: std::path::PathBuf,
    pub snapshots: std::sync::Arc<crate::snapshot::Snapshots>,
    pub key_health: std::sync::Arc<tokio::sync::Mutex<KeyHealth>>,
    pub secure_cookie: bool,
}
impl AppState {
    pub async fn for_testing(store: Store) -> Self {
        Self {
            store: store.clone(),
            keys: KeyStore::testing(),
            http: crate::relay::protected_http_client().unwrap(),
            bootstrap_token: "test".into(),
            data_directory: std::env::temp_dir(),
            snapshots: std::sync::Arc::new(crate::snapshot::Snapshots::load(&store).await.unwrap()),
            key_health: Default::default(),
            secure_cookie: false,
        }
    }
}
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(openai))
        .route("/v1beta/models", get(gemini_models))
        .route("/v1/messages", post(anthropic))
        .route("/v1beta/models/{*path}", post(gemini))
        .nest("/api/v1", crate::admin::router(state.clone()))
        .fallback_service(
            tower_http::services::ServeDir::new("frontend/dist").fallback(
                tower_http::services::ServeFile::new("frontend/dist/index.html"),
            ),
        )
        .with_state(state)
}
fn error(status: StatusCode, message: impl Into<String>) -> Response {
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
        Protocol::OpenAI => headers
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
    let anthropic = headers.contains_key("x-api-key");
    let protocol = if anthropic {
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
            if anthropic {
                json!({"id":model,"type":"model","display_name":model})
            } else {
                json!({"id":model,"object":"model","owned_by":"sepia"})
            }
        })
        .collect::<Vec<_>>();
    Json(if anthropic {
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
        Protocol::OpenAI,
        None,
        None,
        peer.ip(),
        body,
    )
    .await
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
        Protocol::Anthropic,
        None,
        None,
        peer.ip(),
        body,
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
        Protocol::Gemini,
        Some((model, action == "streamGenerateContent")),
        credential_query.as_deref(),
        peer.ip(),
        body,
    )
    .await
}
async fn relay(
    state: AppState,
    headers: HeaderMap,
    inbound: Protocol,
    path: Option<(&str, bool)>,
    query: Option<&str>,
    peer: std::net::IpAddr,
    body: Value,
) -> Response {
    let protocol_credential = match inbound {
        Protocol::OpenAI => ProtocolCredential::OpenAI,
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
    let request_id = match state
        .store
        .begin_request(
            client.id,
            match inbound {
                Protocol::OpenAI => ProviderProtocol::OpenAI,
                Protocol::Anthropic => ProviderProtocol::Anthropic,
                Protocol::Gemini => ProviderProtocol::Gemini,
            },
            &initial_model,
        )
        .await
    {
        Ok(id) => id,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "observability unavailable",
            );
        }
    };
    let mut request = match decode_request(inbound, body, path.map(|p| p.0)) {
        Ok(request) => request,
        Err(message) => {
            let _ = state
                .store
                .finish_request(&request_id, "invalid", 0, 0, 0)
                .await;
            return error(StatusCode::BAD_REQUEST, message);
        }
    };
    if path.is_some_and(|p| p.1) {
        request.stream = true;
    }
    if initial_model.is_empty() {
        let _ = state
            .store
            .set_request_model(&request_id, &request.model)
            .await;
    }
    if !client
        .models
        .iter()
        .any(|pattern| glob(pattern, &request.model))
    {
        let _ = state
            .store
            .finish_request(&request_id, "denied", 0, 0, 0)
            .await;
        return error(StatusCode::FORBIDDEN, "model is not allowed by this key");
    }
    let _ = state
        .store
        .finish_request(&request_id, "routing", 0, 0, 0)
        .await;
    let started = Instant::now();
    let snapshot = state.snapshots.get();
    let routes = &snapshot.routes;
    let Some(route) = routes
        .iter()
        .find(|route| route.slug == request.model)
        .or_else(|| {
            routes.iter().find(|route| {
                route
                    .patterns
                    .iter()
                    .any(|pattern| glob(pattern, &request.model))
            })
        })
        .or_else(|| routes.iter().find(|route| route.is_default))
    else {
        let _ = state
            .store
            .finish_request(&request_id, "no_route", 0, 0, 0)
            .await;
        return error(StatusCode::NOT_FOUND, "no route for model");
    };
    let quota_model = route
        .candidates
        .first()
        .map(|candidate| candidate.1.as_str())
        .unwrap_or(&request.model);
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
            let _ = state
                .store
                .finish_request(&request_id, "quota_denied", 0, 0, 0)
                .await;
            return error(StatusCode::TOO_MANY_REQUESTS, message);
        }
        Err(_) => {
            let _ = state
                .store
                .finish_request(&request_id, "failed", 0, 0, 0)
                .await;
            return error(StatusCode::INTERNAL_SERVER_ERROR, "quota unavailable");
        }
    };
    let guard =
        crate::lifecycle::RequestGuard::new(state.store.clone(), request_id.clone(), reservation);
    let providers = &snapshot.providers;
    let mut attempts_used = 0;
    'candidates: for (provider_id, model) in &route.candidates {
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
        loop {
            if attempts_used >= snapshot.max_attempts {
                break 'candidates;
            }
            let (key_id, secret) = {
                let health = state.key_health.lock().await;
                match state
                    .store
                    .upstream_key_for_attempt(
                        provider.id,
                        health.next.get(&provider.id).copied().unwrap_or(0),
                        &health.cooldowns,
                        &state.keys,
                    )
                    .await
                {
                    Ok(Some(key)) => key,
                    _ => continue 'candidates,
                }
            };
            attempts_used += 1;
            state
                .key_health
                .lock()
                .await
                .next
                .insert(provider.id, key_id as usize + 1);
            let attempt_started = Instant::now();
            let response = match call(&state.http, provider, &secret, &request, model).await {
                Ok(response) => response,
                Err(message) => {
                    state
                        .key_health
                        .lock()
                        .await
                        .cooldowns
                        .insert(key_id, chrono::Utc::now() + chrono::Duration::seconds(60));
                    if let Some(response) = record_attempt(
                        &state,
                        crate::observability::NewAttempt {
                            request_id: &request_id,
                            provider_id: Some(provider.id),
                            provider_key_id: Some(key_id),
                            model,
                            status: "retry",
                            error: Some(&message),
                            input_tokens: 0,
                            output_tokens: 0,
                            latency_ms: attempt_started.elapsed().as_millis() as i64,
                        },
                    )
                    .await
                    {
                        return response;
                    }
                    continue 'candidates;
                }
            };
            if !response.status().is_success() {
                let status = response.status();
                let retry = status == StatusCode::TOO_MANY_REQUESTS
                    || status == StatusCode::REQUEST_TIMEOUT
                    || matches!(status.as_u16(), 500 | 502 | 503 | 504)
                    || status == StatusCode::UNAUTHORIZED
                    || status == StatusCode::FORBIDDEN;
                if let Some(response) = record_attempt(
                    &state,
                    crate::observability::NewAttempt {
                        request_id: &request_id,
                        provider_id: Some(provider.id),
                        provider_key_id: Some(key_id),
                        model,
                        status: if retry { "retry" } else { "failed" },
                        error: Some(&format!("upstream HTTP {status}")),
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
                        state
                            .key_health
                            .lock()
                            .await
                            .cooldowns
                            .insert(key_id, chrono::Utc::now() + chrono::Duration::seconds(60));
                        let delay = response
                            .headers()
                            .get("retry-after")
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| value.parse::<u64>().ok())
                            .unwrap_or(0)
                            .min(5);
                        if delay > 0 {
                            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        }
                    }
                    if status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN {
                        continue 'candidates;
                    }
                    continue;
                }
                let _ = guard.complete("failed", 0, 0, 0).await;
                return error(StatusCode::BAD_GATEWAY, "upstream request failed");
            }
            if request.stream {
                let start = StreamEvent::Start {
                    id: request_id.clone(),
                    model: request.model.clone(),
                };
                let stream = stream_response(
                    inbound,
                    outbound_protocol(provider),
                    response,
                    start,
                    crate::relay::StreamContext {
                        store: state.store.clone(),
                        request_id,
                        provider_id: provider.id,
                        provider_key_id: key_id,
                        model: model.clone(),
                        guard,
                        started,
                    },
                );
                return stream;
            }
            let value = match response.json().await {
                Ok(value) => value,
                Err(_) => {
                    if let Some(response) = record_attempt(
                        &state,
                        crate::observability::NewAttempt {
                            request_id: &request_id,
                            provider_id: Some(provider.id),
                            provider_key_id: Some(key_id),
                            model,
                            status: "retry",
                            error: Some("invalid upstream JSON"),
                            input_tokens: 0,
                            output_tokens: 0,
                            latency_ms: attempt_started.elapsed().as_millis() as i64,
                        },
                    )
                    .await
                    {
                        return response;
                    }
                    continue;
                }
            };
            let decoded: ChatResponse = match decode_response(outbound_protocol(provider), value) {
                Ok(response) => response,
                Err(_) => continue,
            };
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
                .model_cost(model, decoded.input_tokens, decoded.output_tokens)
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
            return json_response(inbound, decoded);
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
