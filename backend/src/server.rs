use crate::{
    KeyStore, ProviderProtocol, Store,
    protocol::{ChatResponse, Protocol, StreamEvent, decode_request, decode_response},
    relay::{call, json_response, outbound_protocol, stream_response},
};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub keys: KeyStore,
    pub http: reqwest::Client,
    pub bootstrap_token: String,
}
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(openai))
        .route("/v1/messages", post(anthropic))
        .route("/v1beta/models/{*path}", post(gemini))
        .nest("/api/v1", crate::admin::router(state.clone()))
        .with_state(state)
}
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":{"message":message.into()}}))).into_response()
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
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = credential(&headers, Protocol::OpenAI) else {
        return error(StatusCode::UNAUTHORIZED, "missing API key");
    };
    let Ok(Some(client)) = state.store.authenticate_client(&token).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    match state.store.routes().await { Ok(routes) => Json(json!({"object":"list","data":routes.into_iter().filter(|route| client.models.iter().any(|pattern| glob(pattern, &route.slug))).map(|route| json!({"id":route.slug,"object":"model","owned_by":"sepia"})).collect::<Vec<_>>()})).into_response(), Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "configuration unavailable") }
}
async fn openai(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    relay(state, headers, Protocol::OpenAI, None, body).await
}
async fn anthropic(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    relay(state, headers, Protocol::Anthropic, None, body).await
}
async fn gemini(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(path): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some((model, action)) = path.split_once(':') else {
        return error(StatusCode::NOT_FOUND, "unknown Gemini action");
    };
    if action != "generateContent" && action != "streamGenerateContent" {
        return error(StatusCode::NOT_FOUND, "unknown Gemini action");
    }
    relay(
        state,
        headers,
        Protocol::Gemini,
        Some((model, action == "streamGenerateContent")),
        body,
    )
    .await
}
async fn relay(
    state: AppState,
    headers: HeaderMap,
    inbound: Protocol,
    path: Option<(&str, bool)>,
    body: Value,
) -> Response {
    let Some(token) = credential(&headers, inbound) else {
        return error(StatusCode::UNAUTHORIZED, "missing API key");
    };
    let client = match state.store.authenticate_client(&token).await {
        Ok(Some(client)) => client,
        _ => return error(StatusCode::UNAUTHORIZED, "invalid API key"),
    };
    let mut request = match decode_request(inbound, body, path.map(|p| p.0)) {
        Ok(request) => request,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    if path.is_some_and(|p| p.1) {
        request.stream = true;
    }
    if !client
        .models
        .iter()
        .any(|pattern| glob(pattern, &request.model))
    {
        return error(StatusCode::FORBIDDEN, "model is not allowed by this key");
    }
    let reservation = match state
        .store
        .reserve_quota(
            client.id,
            &request.model,
            request.max_tokens.unwrap_or(1024) as i64,
        )
        .await
    {
        Ok(id) => id,
        Err(_) => return error(StatusCode::TOO_MANY_REQUESTS, "quota exceeded"),
    };
    let request_id = match state
        .store
        .begin_request(
            client.id,
            match inbound {
                Protocol::OpenAI => ProviderProtocol::OpenAI,
                Protocol::Anthropic => ProviderProtocol::Anthropic,
                Protocol::Gemini => ProviderProtocol::Gemini,
            },
            &request.model,
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
    let started = Instant::now();
    let routes = match state.store.routes().await {
        Ok(routes) => routes,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "configuration unavailable",
            );
        }
    };
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
        return error(StatusCode::NOT_FOUND, "no route for model");
    };
    let providers = match state.store.providers().await {
        Ok(providers) => providers,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "configuration unavailable",
            );
        }
    };
    for (provider_id, model) in &route.candidates {
        let Some(provider) = providers
            .iter()
            .find(|provider| provider.id == *provider_id)
        else {
            continue;
        };
        let secret = match state.store.upstream_key(provider.id, &state.keys).await {
            Ok(secret) => secret,
            Err(_) => continue,
        };
        let attempt_started = Instant::now();
        let response = match call(&state.http, provider, &secret, &request, model).await {
            Ok(response) => response,
            Err(message) => {
                if let Some(response) = record_attempt(
                    &state,
                    crate::observability::NewAttempt {
                        request_id: &request_id,
                        provider_id: Some(provider.id),
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
                continue;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let retry = status == StatusCode::TOO_MANY_REQUESTS
                || status == StatusCode::REQUEST_TIMEOUT
                || status.is_server_error()
                || status == StatusCode::UNAUTHORIZED
                || status == StatusCode::FORBIDDEN;
            if let Some(response) = record_attempt(
                &state,
                crate::observability::NewAttempt {
                    request_id: &request_id,
                    provider_id: Some(provider.id),
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
                continue;
            }
            let _ = state
                .store
                .finish_request(
                    &request_id,
                    "failed",
                    0,
                    0,
                    started.elapsed().as_millis() as i64,
                )
                .await;
            let _ = state.store.settle_quota(&reservation, 0).await;
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
                    model: model.clone(),
                    reservation,
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
        if state
            .store
            .finish_request(
                &request_id,
                "success",
                decoded.input_tokens,
                decoded.output_tokens,
                started.elapsed().as_millis() as i64,
            )
            .await
            .is_err()
        {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "observability unavailable",
            );
        }
        if state
            .store
            .settle_quota(
                &reservation,
                (decoded.input_tokens + decoded.output_tokens) as i64,
            )
            .await
            .is_err()
        {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "quota settlement unavailable",
            );
        }
        return json_response(inbound, decoded);
    }
    let _ = state
        .store
        .finish_request(
            &request_id,
            "failed",
            0,
            0,
            started.elapsed().as_millis() as i64,
        )
        .await;
    let _ = state.store.settle_quota(&reservation, 0).await;
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
