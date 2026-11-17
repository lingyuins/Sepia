//! OpenAI-compatible auxiliary endpoints: embeddings, rerank, moderations,
//! image generation/editing, speech, transcription/translation, video and
//! music generation, and search. Bodies pass through verbatim (no protocol
//! conversion); routing, key scopes, quotas and observation match inference.
//! Only OpenAI/OpenAIResponses providers serve these endpoints. Async video
//! and Suno tasks are mapped to their submitting provider/key so polls reach
//! the same upstream channel.
use crate::{
    ProviderProtocol, Store,
    server::{
        AppState, ProtocolCredential, error, extract_credential, model_allowed, route_for_model,
    },
};
use axum::{
    Router,
    body::Bytes,
    extract::{Multipart, Path, State, connect_info::ConnectInfo},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};
use sqlx::Row;
use std::{net::SocketAddr, time::Instant};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/embeddings", post(embeddings))
        .route("/v1/rerank", post(rerank))
        .route("/v1/moderations", post(moderations))
        .route("/v1/images/generations", post(image_generations))
        .route("/v1/images/edits", post(image_edits))
        .route("/v1/audio/speech", post(speech))
        .route("/v1/audio/transcriptions", post(transcriptions))
        .route("/v1/audio/translations", post(translations))
        .route("/v1/videos", post(video_create))
        .route("/v1/videos/{task_id}", get(video_poll))
        .route("/v1/video/generations", post(video_generations))
        .route("/v1/search", post(search))
        .route("/suno/submit/{action}", post(suno_submit))
        .route("/suno/fetch", post(suno_fetch))
        .route("/suno/fetch/{task_id}", get(suno_poll))
}

/// A prepared client request, independent of which upstream attempt serves it.
struct Prepared {
    /// Path relative to the provider endpoint (`embeddings`) or, for Suno,
    /// relative to the endpoint with its trailing `/v1` stripped.
    path: String,
    suno: bool,
    get: bool,
    payload: Payload,
    model: Option<String>,
    /// Submit endpoints that create a pollable upstream task.
    task_kind: Option<&'static str>,
    /// Polls must resolve a task owned by this client.
    required_task: Option<String>,
    /// Suno batch fetch pins the first known task's channel when present.
    lookup_task: Option<String>,
    log_request: String,
}
enum Payload {
    None,
    Raw {
        bytes: Vec<u8>,
        content_type: String,
    },
    Parts(Vec<Part>),
}
struct Part {
    name: String,
    file_name: Option<String>,
    content_type: Option<String>,
    bytes: Vec<u8>,
}
struct TaskRow {
    provider_id: i64,
    provider_key_id: Option<i64>,
    model: String,
}

async fn json_body(body: Bytes) -> Result<Value, Box<Response>> {
    serde_json::from_slice(&body)
        .map_err(|_| Box::new(error(StatusCode::BAD_REQUEST, "invalid JSON body")))
}
fn body_model(value: &Value) -> Option<String> {
    value
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
}
fn raw_prepared(path: &str, task_kind: Option<&'static str>, body: Bytes) -> Prepared {
    Prepared {
        path: path.into(),
        suno: false,
        get: false,
        payload: Payload::Raw {
            bytes: body.to_vec(),
            content_type: "application/json".into(),
        },
        model: None,
        task_kind,
        required_task: None,
        lookup_task: None,
        log_request: String::from_utf8_lossy(&body).into_owned(),
    }
}

async fn embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("embeddings", None, body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn rerank(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("rerank", None, body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn moderations(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("moderations", None, body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn image_generations(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("images/generations", None, body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn speech(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("audio/speech", None, body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("search", None, body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn video_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("videos", Some("video"), body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn video_generations(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("video/generations", Some("video"), body);
    prepared.model = body_model(&value);
    forward(state, headers, peer.ip(), prepared).await
}
async fn video_poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(task_id): Path<String>,
) -> Response {
    let prepared = Prepared {
        path: format!("videos/{task_id}"),
        suno: false,
        get: true,
        payload: Payload::None,
        model: None,
        task_kind: None,
        required_task: Some(task_id),
        lookup_task: None,
        log_request: String::new(),
    };
    forward(state, headers, peer.ip(), prepared).await
}
async fn suno_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(action): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(response) = json_body(body.clone()).await {
        return *response;
    }
    let mut prepared = raw_prepared(&format!("suno/submit/{action}"), Some("suno"), body);
    prepared.suno = true;
    prepared.model = Some(if action == "lyrics" {
        "suno_lyrics".into()
    } else {
        "suno_music".into()
    });
    forward(state, headers, peer.ip(), prepared).await
}
async fn suno_fetch(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let value = match json_body(body.clone()).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let mut prepared = raw_prepared("suno/fetch", None, body);
    prepared.suno = true;
    prepared.model = Some("suno_music".into());
    prepared.lookup_task = value
        .get("ids")
        .and_then(Value::as_array)
        .and_then(|ids| ids.first())
        .and_then(Value::as_str)
        .map(str::to_owned);
    forward(state, headers, peer.ip(), prepared).await
}
async fn suno_poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(task_id): Path<String>,
) -> Response {
    let prepared = Prepared {
        path: format!("suno/fetch/{task_id}"),
        suno: true,
        get: true,
        payload: Payload::None,
        model: None,
        task_kind: None,
        required_task: Some(task_id),
        lookup_task: None,
        log_request: String::new(),
    };
    forward(state, headers, peer.ip(), prepared).await
}

async fn multipart_prepared(
    path: &str,
    mut multipart: Multipart,
) -> Result<Prepared, Box<Response>> {
    let mut parts = Vec::new();
    let mut model = None;
    let mut fields = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().map(str::to_owned);
        let content_type = field.content_type().map(str::to_owned);
        let bytes = match field.bytes().await {
            Ok(bytes) => bytes.to_vec(),
            Err(_) => {
                return Err(Box::new(error(
                    StatusCode::BAD_REQUEST,
                    "invalid multipart field",
                )));
            }
        };
        if name == "model" && file_name.is_none() {
            model = String::from_utf8(bytes.clone())
                .ok()
                .filter(|m| !m.is_empty());
        }
        fields.push(name.clone());
        parts.push(Part {
            name,
            file_name,
            content_type,
            bytes,
        });
    }
    Ok(Prepared {
        path: path.into(),
        suno: false,
        get: false,
        payload: Payload::Parts(parts),
        model: model.clone(),
        task_kind: None,
        required_task: None,
        lookup_task: None,
        log_request: json!({"model":model,"fields":fields}).to_string(),
    })
}
async fn image_edits(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    multipart: Multipart,
) -> Response {
    let prepared = match multipart_prepared("images/edits", multipart).await {
        Ok(prepared) => prepared,
        Err(response) => return *response,
    };
    forward(state, headers, peer.ip(), prepared).await
}
async fn transcriptions(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    multipart: Multipart,
) -> Response {
    let prepared = match multipart_prepared("audio/transcriptions", multipart).await {
        Ok(prepared) => prepared,
        Err(response) => return *response,
    };
    forward(state, headers, peer.ip(), prepared).await
}
async fn translations(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    multipart: Multipart,
) -> Response {
    let prepared = match multipart_prepared("audio/translations", multipart).await {
        Ok(prepared) => prepared,
        Err(response) => return *response,
    };
    forward(state, headers, peer.ip(), prepared).await
}

async fn task_row(store: &Store, client_key_id: i64, task_id: &str) -> Option<TaskRow> {
    let row = sqlx::query(
        "SELECT provider_id, provider_key_id, model FROM auxiliary_tasks WHERE task_id = ? AND client_key_id = ?",
    )
    .bind(task_id)
    .bind(client_key_id)
    .fetch_optional(&store.pool)
    .await
    .ok()??;
    Some(TaskRow {
        provider_id: row.get("provider_id"),
        provider_key_id: row.get("provider_key_id"),
        model: row.get("model"),
    })
}
async fn insert_task(
    store: &Store,
    task_id: &str,
    client_key_id: i64,
    provider_id: i64,
    provider_key_id: i64,
    kind: &str,
    model: &str,
) {
    let _ = sqlx::query(
        "INSERT INTO auxiliary_tasks (task_id, client_key_id, provider_id, provider_key_id, kind, model, created_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(task_id) DO NOTHING",
    )
    .bind(task_id)
    .bind(client_key_id)
    .bind(provider_id)
    .bind(provider_key_id)
    .bind(kind)
    .bind(model)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&store.pool)
    .await;
}
fn extract_task_id(value: &Value) -> Option<String> {
    let candidate = value
        .get("id")
        .or_else(|| value.get("task_id"))
        .or_else(|| value.get("data"));
    match candidate {
        Some(Value::String(id)) if !id.is_empty() => Some(id.clone()),
        Some(data) => data
            .get("task_id")
            .or_else(|| data.get("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned),
        _ => None,
    }
}
fn usage_tokens(value: &Value) -> (u64, u64) {
    let Some(usage) = value.get("usage") else {
        return (0, 0);
    };
    let token = |keys: &[&str]| -> u64 {
        keys.iter()
            .find_map(|key| usage.get(key).and_then(Value::as_u64))
            .unwrap_or(0)
    };
    let input = token(&["prompt_tokens", "input_tokens"]);
    let output = token(&["completion_tokens", "output_tokens"]);
    if input == 0 && output == 0 {
        (token(&["total_tokens"]), 0)
    } else {
        (input, output)
    }
}
fn upstream_url(provider: &crate::Provider, prepared: &Prepared) -> String {
    let endpoint = provider.endpoint.trim_end_matches('/');
    if prepared.suno {
        let base = endpoint.strip_suffix("/v1").unwrap_or(endpoint);
        format!("{base}/{}", prepared.path)
    } else {
        format!("{endpoint}/{}", prepared.path)
    }
}
fn passthrough(status: StatusCode, content_type: Option<&str>, bytes: Vec<u8>) -> Response {
    let mut response = Response::builder().status(status);
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    response
        .body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response unavailable"))
}
fn build_request(
    state: &AppState,
    provider: &crate::Provider,
    secret: &str,
    prepared: &Prepared,
) -> reqwest::RequestBuilder {
    let url = upstream_url(provider, prepared);
    let builder = if prepared.get {
        state.http.get(url)
    } else {
        let builder = state.http.post(url);
        match &prepared.payload {
            Payload::None => builder,
            Payload::Raw {
                bytes,
                content_type,
            } => builder
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(bytes.clone()),
            Payload::Parts(parts) => {
                let mut form = reqwest::multipart::Form::new();
                for part in parts {
                    let mut piece = reqwest::multipart::Part::bytes(part.bytes.clone());
                    if let Some(file_name) = &part.file_name {
                        piece = piece.file_name(file_name.clone());
                    }
                    if let Some(content_type) = &part.content_type {
                        piece = piece.mime_str(content_type).unwrap_or_else(|_| {
                            let mut fallback = reqwest::multipart::Part::bytes(part.bytes.clone());
                            if let Some(file_name) = &part.file_name {
                                fallback = fallback.file_name(file_name.clone());
                            }
                            fallback
                        });
                    }
                    form = form.part(part.name.clone(), piece);
                }
                builder.multipart(form)
            }
        }
    };
    crate::relay::authorize(builder, crate::protocol::Protocol::OpenAI, secret)
        .timeout(std::time::Duration::from_secs(300))
}

async fn forward(
    state: AppState,
    headers: HeaderMap,
    peer: std::net::IpAddr,
    prepared: Prepared,
) -> Response {
    let token = match extract_credential(&headers, ProtocolCredential::OpenAI, None) {
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
    // Polls are authorized by task ownership; batch fetch pins the first
    // known task's channel so results come from the submitting upstream.
    let mut pinned = None;
    if let Some(task_id) = &prepared.required_task {
        match task_row(&state.store, client.id, task_id).await {
            Some(task) => pinned = Some(task),
            None => return error(StatusCode::NOT_FOUND, "unknown task"),
        }
    } else if let Some(task_id) = &prepared.lookup_task {
        pinned = task_row(&state.store, client.id, task_id).await;
    }
    let model = match pinned.as_ref().map(|task| task.model.clone()) {
        Some(model) => model,
        None => match prepared.model.clone() {
            Some(model) => model,
            None => return error(StatusCode::BAD_REQUEST, "model is required"),
        },
    };
    let request_id = match state
        .store
        .begin_request(client.id, ProviderProtocol::OpenAI, &model)
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
    let _ = state
        .store
        .set_request_client_ip(&request_id, &peer.to_string())
        .await;
    let snapshot = state.snapshots.get();
    if snapshot.log_bodies && !prepared.log_request.is_empty() {
        let _ = state
            .store
            .save_request_body(
                &request_id,
                &crate::relay::capped_body(&prepared.log_request),
            )
            .await;
    }
    if pinned.is_none() && !model_allowed(&client, &snapshot.routes, &model) {
        let _ = state
            .store
            .finish_request(&request_id, "denied", 0, 0, 0)
            .await;
        return error(StatusCode::FORBIDDEN, "model is not allowed by this key");
    }
    let route = route_for_model(&snapshot.routes, &model);
    if pinned.is_none() && route.is_none() {
        let _ = state
            .store
            .finish_request(&request_id, "no_route", 0, 0, 0)
            .await;
        return error(StatusCode::NOT_FOUND, "no route for model");
    }
    let route_label = route
        .map(|route| route.slug.clone())
        .unwrap_or(model.clone());
    let targets: Vec<(i64, String, Option<i64>)> = match &pinned {
        Some(task) => vec![(task.provider_id, task.model.clone(), task.provider_key_id)],
        None => route
            .unwrap()
            .candidates
            .iter()
            .map(|(provider_id, model)| (*provider_id, model.clone(), None))
            .collect(),
    };
    let quota_model = targets
        .first()
        .map(|(_, model, _)| model.as_str())
        .unwrap_or(&model);
    if state
        .store
        .set_request_route(&request_id, &route_label)
        .await
        .is_err()
    {
        let _ = state
            .store
            .finish_request(&request_id, "failed", 0, 0, 0)
            .await;
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "observability unavailable",
        );
    }
    let reservation = match state
        .store
        .reserve_quota(client.id, quota_model, 1024)
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
    'candidates: for (provider_id, model, pinned_key) in &targets {
        let Some(provider) = providers
            .iter()
            .find(|provider| provider.id == *provider_id)
        else {
            continue;
        };
        if !matches!(
            provider.protocol,
            ProviderProtocol::OpenAI | ProviderProtocol::OpenAIResponses
        ) {
            continue;
        }
        if !state
            .store
            .provider_allowed(client.id, provider.id)
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let key_count = if pinned_key.is_some() {
            1
        } else {
            state
                .store
                .provider_keys(provider.id)
                .await
                .unwrap_or_default()
                .iter()
                .filter(|key| {
                    key.models
                        .as_ref()
                        .is_none_or(|models| models.iter().any(|item| item == model))
                })
                .count()
                .max(1)
        };
        let mut key_attempts = 0usize;
        loop {
            if attempts_used >= snapshot.max_attempts {
                break 'candidates;
            }
            let (key_id, secret) = match pinned_key {
                Some(key_id) => match state
                    .store
                    .upstream_key_by_id(provider.id, *key_id, &state.keys)
                    .await
                {
                    Ok(secret) => (*key_id, secret),
                    Err(_) => continue 'candidates,
                },
                None => {
                    let health = state.key_health.lock().await;
                    match state
                        .store
                        .upstream_key_for_attempt(
                            provider.id,
                            model,
                            key_attempts,
                            &health.cooldowns,
                            &state.keys,
                        )
                        .await
                    {
                        Ok(Some(key)) => key,
                        _ => continue 'candidates,
                    }
                }
            };
            attempts_used += 1;
            let attempt_started = Instant::now();
            let response = match build_request(&state, provider, &secret, &prepared)
                .send()
                .await
            {
                Ok(response) => response,
                Err(_) => {
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
                            error: Some("upstream unavailable"),
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
            let first_byte_ms = attempt_started.elapsed().as_millis() as i64;
            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            if !status.is_success() {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0)
                    .min(5);
                let bytes = response.bytes().await.unwrap_or_default().to_vec();
                let retry = matches!(
                    status.as_u16(),
                    429 | 408 | 500 | 502 | 503 | 504 | 401 | 403
                );
                let endpoint_unsupported = matches!(status.as_u16(), 404 | 405 | 501);
                if let Some(response) = record_attempt(
                    &state,
                    crate::observability::NewAttempt {
                        request_id: &request_id,
                        provider_id: Some(provider.id),
                        provider_key_id: Some(key_id),
                        model,
                        status: if retry || endpoint_unsupported {
                            "retry"
                        } else {
                            "failed"
                        },
                        error: Some(&format!("upstream HTTP {status} ({})", prepared.path)),
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
                        if retry_after > 0 {
                            tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                        }
                    }
                    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                        key_attempts += 1;
                        if key_attempts < key_count {
                            continue;
                        }
                    }
                    continue 'candidates;
                }
                if endpoint_unsupported {
                    continue 'candidates;
                }
                // Terminal upstream errors pass through verbatim so clients
                // see the provider's own status and error body.
                let _ = guard.complete("failed", 0, 0, 0).await;
                return passthrough(status, content_type.as_deref(), bytes);
            }
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes.to_vec(),
                Err(_) => {
                    if let Some(response) = record_attempt(
                        &state,
                        crate::observability::NewAttempt {
                            request_id: &request_id,
                            provider_id: Some(provider.id),
                            provider_key_id: Some(key_id),
                            model,
                            status: "retry",
                            error: Some("upstream body unavailable"),
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
            let value: Option<Value> = serde_json::from_slice(&bytes).ok();
            let (input_tokens, output_tokens) = value.as_ref().map(usage_tokens).unwrap_or((0, 0));
            let _ = state
                .store
                .set_request_details(&request_id, first_byte_ms, 0, 0)
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
                    input_tokens,
                    output_tokens,
                    latency_ms: attempt_started.elapsed().as_millis() as i64,
                },
            )
            .await
            {
                return response;
            }
            let cost = state
                .store
                .model_cost(model, input_tokens, output_tokens)
                .await
                .unwrap_or(0);
            if guard
                .complete("success", input_tokens, output_tokens, cost)
                .await
                .is_err()
            {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "request settlement unavailable",
                );
            }
            if let (Some(kind), Some(value)) = (prepared.task_kind, value.as_ref())
                && let Some(task_id) = extract_task_id(value)
            {
                insert_task(
                    &state.store,
                    &task_id,
                    client.id,
                    provider.id,
                    key_id,
                    kind,
                    model,
                )
                .await;
            }
            if snapshot.log_bodies
                && content_type
                    .as_deref()
                    .is_some_and(|kind| kind.contains("json") || kind.starts_with("text/"))
            {
                let _ = state
                    .store
                    .save_response_body(
                        &request_id,
                        &crate::relay::capped_body(&String::from_utf8_lossy(&bytes)),
                    )
                    .await;
            }
            return passthrough(StatusCode::OK, content_type.as_deref(), bytes);
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
