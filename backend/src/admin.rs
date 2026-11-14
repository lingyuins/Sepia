use crate::updates::ProviderUpdate;
use crate::{NewProvider, NewRoute, UpsertClientKey, server::AppState};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

fn default_true() -> bool {
    true
}
fn default_group() -> String {
    "official".into()
}
#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}
#[derive(Deserialize)]
pub struct Bootstrap {
    username: String,
    password: String,
    token: String,
}
#[derive(Deserialize)]
pub struct ProviderInput {
    name: String,
    protocol: crate::ProviderProtocol,
    endpoint: String,
    models: Vec<String>,
    secret: String,
    #[serde(default)]
    secrets: Vec<String>,
    #[serde(default = "default_group")]
    group_id: String,
    #[serde(default = "default_true")]
    enabled: bool,
}
#[derive(Deserialize)]
pub struct ProviderPreviewInput {
    protocol: crate::ProviderProtocol,
    endpoint: String,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    secrets: Vec<String>,
}
#[derive(Deserialize, Default)]
pub struct ProviderTestInput {
    protocol: Option<crate::ProviderProtocol>,
    endpoint: Option<String>,
    secret: Option<String>,
}
#[derive(Deserialize)]
pub struct RouteInput {
    slug: String,
    name: String,
    description: String,
    patterns: Vec<String>,
    candidates: Vec<(i64, String)>,
    #[serde(default)]
    is_default: bool,
    #[serde(default = "default_true")]
    list_members: bool,
    #[serde(default = "default_true")]
    enabled: bool,
}
#[derive(Deserialize)]
pub struct PriceInput {
    provider_id: i64,
    model: String,
    input_price_micros: i64,
    output_price_micros: i64,
}
#[derive(Deserialize)]
pub struct KeyInput {
    name: String,
    models: Vec<String>,
    #[serde(default)]
    token_limit: i64,
    #[serde(default)]
    max_amount_micros: i64,
    #[serde(default)]
    rpm: i64,
    #[serde(default)]
    tpm: i64,
    expires_at: Option<String>,
}
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":{"message":message.into()}}))).into_response()
}
fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().strip_prefix("sepia_session="))
}
async fn status(State(state): State<AppState>) -> Response {
    match state.store.initialized().await {
        Ok(initialized) => Json(json!({"initialized":initialized})).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn bootstrap(State(state): State<AppState>, Json(input): Json<Bootstrap>) -> Response {
    match state
        .store
        .bootstrap(
            &input.username,
            &input.password,
            &input.token,
            &state.bootstrap_token,
        )
        .await
    {
        Ok(()) => {
            if let Err(error) = crate::bootstrap::remove_token(&state.data_directory).await {
                eprintln!("failed to remove bootstrap token file after initialization: {error}");
            }
            Json(json!({"initialized":true})).into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn login(State(state): State<AppState>, Json(input): Json<Credentials>) -> Response {
    match state.store.login(&input.username, &input.password).await {
        Ok(token) => Response::builder()
            .header(
                header::SET_COOKIE,
                format!(
                    "sepia_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{}",
                    if state.secure_cookie { "; Secure" } else { "" }
                ),
            )
            .body(
                Json(json!({"authenticated":true}))
                    .into_response()
                    .into_body(),
            )
            .unwrap(),
        Err(message) => error(StatusCode::UNAUTHORIZED, message),
    }
}
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = session_cookie(&headers) {
        let _ = state.store.logout(token).await;
    }
    Response::builder()
        .header(
            header::SET_COOKIE,
            format!(
                "sepia_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{}",
                if state.secure_cookie { "; Secure" } else { "" }
            ),
        )
        .body(axum::body::Body::empty())
        .unwrap()
}
async fn session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match session_cookie(&headers) {
        Some(token) => match state.store.session(token).await {
            Ok(Some(session)) => Json(json!({"username":session.username})).into_response(),
            _ => error(StatusCode::UNAUTHORIZED, "invalid session"),
        },
        None => error(StatusCode::UNAUTHORIZED, "missing session"),
    }
}
async fn auth(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if request.method() != axum::http::Method::GET
        && let Some(origin) = request
            .headers()
            .get("origin")
            .and_then(|value| value.to_str().ok())
    {
        let host = request
            .headers()
            .get("host")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let allowed = ["http://localhost:5173", "http://127.0.0.1:5173"].contains(&origin)
            || origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"))
                == Some(host);
        if !allowed {
            return error(StatusCode::FORBIDDEN, "management origin rejected");
        }
    }
    let Some(token) = session_cookie(request.headers()) else {
        return error(StatusCode::UNAUTHORIZED, "missing session");
    };
    match state.store.session(token).await {
        Ok(Some(_)) => next.run(request).await,
        _ => error(StatusCode::UNAUTHORIZED, "invalid session"),
    }
}
async fn providers(State(state): State<AppState>) -> Response {
    match state.store.providers().await { Ok(providers) => Json(json!(providers.into_iter().map(|p| json!({"id":p.id,"name":p.name,"protocol":p.protocol,"endpoint":p.endpoint,"models":p.models,"key_count":p.key_count,"group_id":p.group_id,"enabled":p.enabled})).collect::<Vec<_>>())).into_response(), Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message) }
}
async fn preview_provider_models(
    State(state): State<AppState>,
    Json(input): Json<ProviderPreviewInput>,
) -> Response {
    let secret = if input.secret.trim().is_empty() {
        input
            .secrets
            .iter()
            .find(|value| !value.trim().is_empty())
            .map(String::as_str)
            .unwrap_or("")
    } else {
        input.secret.as_str()
    };
    let provider = crate::Provider {
        id: 0,
        name: String::new(),
        protocol: input.protocol,
        endpoint: input.endpoint,
        models: Vec::new(),
        key_count: 0,
        group_id: String::new(),
        enabled: false,
    };
    match crate::discovery::discover_models(&state.http, &provider, secret).await {
        Ok(models) => Json(json!({"models":models})).into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, message),
    }
}
async fn create_provider(
    State(state): State<AppState>,
    Json(input): Json<ProviderInput>,
) -> Response {
    let ProviderInput {
        name,
        protocol,
        endpoint,
        mut models,
        secret,
        secrets,
        group_id,
        enabled,
    } = input;
    if models.is_empty() {
        let candidate = crate::Provider {
            id: 0,
            name: name.clone(),
            protocol,
            endpoint: endpoint.clone(),
            models: Vec::new(),
            key_count: 0,
            group_id: group_id.clone(),
            enabled,
        };
        let discovery_secret = if secret.trim().is_empty() {
            secrets
                .iter()
                .find(|value| !value.trim().is_empty())
                .map(String::as_str)
                .unwrap_or("")
        } else {
            secret.as_str()
        };
        match crate::discovery::discover_models(&state.http, &candidate, discovery_secret).await {
            Ok(discovered) => models = discovered,
            Err(message) => return error(StatusCode::BAD_GATEWAY, message),
        }
    }
    match state
        .store
        .create_provider(
            NewProvider {
                name,
                protocol,
                endpoint,
                models,
            },
            &secret,
            &state.keys,
        )
        .await
    {
        Ok(provider) => {
            if let Err(message) = state
                .store
                .update_provider(ProviderUpdate {
                    id: provider.id,
                    name: &provider.name,
                    protocol: provider.protocol,
                    endpoint: &provider.endpoint,
                    models: &provider.models,
                    group: &group_id,
                    enabled,
                })
                .await
            {
                return error(StatusCode::BAD_REQUEST, message);
            }
            for additional_secret in secrets.iter().filter(|additional_secret| {
                !additional_secret.is_empty() && **additional_secret != secret
            }) {
                if let Err(message) = state
                    .store
                    .add_provider_key(provider.id, additional_secret, &state.keys)
                    .await
                {
                    return error(StatusCode::BAD_REQUEST, message);
                }
            }
            if let Err(message) = state.snapshots.refresh(&state.store).await {
                return error(StatusCode::INTERNAL_SERVER_ERROR, message);
            }
            Json(json!({"id":provider.id,"key_count":provider.key_count})).into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn routes(State(state): State<AppState>) -> Response {
    match state.store.routes().await { Ok(routes) => Json(json!(routes.into_iter().map(|route| json!({"id":route.id,"slug":route.slug,"name":route.name,"description":route.description,"patterns":route.patterns,"is_default":route.is_default,"list_members":route.list_members,"enabled":route.enabled,"candidates":route.candidates})).collect::<Vec<_>>())).into_response(), Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message) }
}
async fn create_route(State(state): State<AppState>, Json(input): Json<RouteInput>) -> Response {
    let Some((provider_id, model)) = input.candidates.first().cloned() else {
        return error(StatusCode::BAD_REQUEST, "at least one candidate required");
    };
    match state
        .store
        .create_route(NewRoute {
            slug: input.slug,
            name: input.name,
            description: input.description,
            patterns: input.patterns,
            provider_id,
            model,
            is_default: input.is_default,
            list_members: input.list_members,
            candidates: input.candidates,
        })
        .await
    {
        Ok(route) => {
            if let Err(message) = state.store.set_route_enabled(route.id, input.enabled).await {
                return error(StatusCode::BAD_REQUEST, message);
            }
            if let Err(message) = state.snapshots.refresh(&state.store).await {
                return error(StatusCode::INTERNAL_SERVER_ERROR, message);
            }
            Json(json!({"id":route.id})).into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<ProviderInput>,
) -> Response {
    match state
        .store
        .update_provider(ProviderUpdate {
            id,
            name: &input.name,
            protocol: input.protocol,
            endpoint: &input.endpoint,
            models: &input.models,
            group: &input.group_id,
            enabled: input.enabled,
        })
        .await
    {
        Ok(()) => {
            let _ = state.snapshots.refresh(&state.store).await;
            StatusCode::OK.into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn update_route(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<RouteInput>,
) -> Response {
    match state
        .store
        .update_route(
            id,
            crate::updates::RouteUpdate {
                slug: &input.slug,
                name: &input.name,
                description: &input.description,
                patterns: &input.patterns,
                is_default: input.is_default,
                list_members: input.list_members,
                enabled: input.enabled,
                candidates: &input.candidates,
            },
        )
        .await
    {
        Ok(()) => {
            let _ = state.snapshots.refresh(&state.store).await;
            StatusCode::OK.into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn keys(State(state): State<AppState>) -> Response {
    match state.store.client_keys().await { Ok(keys) => Json(json!(keys.into_iter().map(|key| json!({"id":key.id,"name":key.name,"models":key.models,"token_limit":key.token_limit,"max_amount_micros":key.max_amount_micros,"rpm":key.rpm,"tpm":key.tpm,"expires_at":key.expires_at})).collect::<Vec<_>>())).into_response(), Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message) }
}
async fn create_key(State(state): State<AppState>, Json(input): Json<KeyInput>) -> Response {
    match state
        .store
        .create_client_key(UpsertClientKey {
            name: input.name,
            models: input.models,
            token_limit: input.token_limit,
            max_amount_micros: input.max_amount_micros,
            rpm: input.rpm,
            tpm: input.tpm,
            expires_at: input.expires_at,
        })
        .await
    {
        Ok(key) => Json(json!({"id":key.id,"plaintext":key.plaintext})).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn revoke_key(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.store.revoke_client_key(id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn requests(State(state): State<AppState>) -> Response {
    match state.store.requests().await {
        Ok(requests) => Json(json!(requests)).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn attempts(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.attempts(&id).await {
        Ok(attempts) => Json(json!(attempts)).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn test_provider(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    input: Option<Json<ProviderTestInput>>,
) -> Response {
    let mut provider = match state.store.providers().await {
        Ok(providers) => match providers.into_iter().find(|provider| provider.id == id) {
            Some(provider) => provider,
            None => return error(StatusCode::NOT_FOUND, "provider not found"),
        },
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let input = input.map(|Json(input)| input).unwrap_or_default();
    if let Some(protocol) = input.protocol {
        provider.protocol = protocol;
    }
    if let Some(endpoint) = input
        .endpoint
        .filter(|endpoint| !endpoint.trim().is_empty())
    {
        provider.endpoint = endpoint;
    }
    let secret = match input.secret.filter(|secret| !secret.trim().is_empty()) {
        Some(secret) => secret,
        None => match state.store.upstream_key(id, &state.keys).await {
            Ok(secret) => secret,
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        },
    };
    match crate::discovery::discover_models(&state.http, &provider, &secret).await {
        Ok(models) => Json(json!({"models":models})).into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, message),
    }
}
async fn add_key(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let Some(secret) = input.get("secret").and_then(serde_json::Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "secret required");
    };
    match state.store.add_provider_key(id, secret, &state.keys).await {
        Ok(id) => Json(json!({"id":id})).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn set_price(State(state): State<AppState>, Json(input): Json<PriceInput>) -> Response {
    match state
        .store
        .set_model_price(
            input.provider_id,
            &input.model,
            input.input_price_micros,
            input.output_price_micros,
        )
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn public_origin(request: axum::extract::Request, next: Next) -> Response {
    if let Some(origin) = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
    {
        let host = request
            .headers()
            .get("host")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let allowed = ["http://localhost:5173", "http://127.0.0.1:5173"].contains(&origin)
            || origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"))
                == Some(host);
        if !allowed {
            return error(StatusCode::FORBIDDEN, "management origin rejected");
        }
    }
    next.run(request).await
}
async fn set_policy(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(policy): Json<crate::policy::ClientPolicy>,
) -> Response {
    match state.store.set_client_policy(id, &policy).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn update_settings(
    State(state): State<AppState>,
    Json(input): Json<std::collections::HashMap<String, serde_json::Value>>,
) -> Response {
    for (key, value) in input {
        if let Err(message) = state.store.set_setting(&key, &value).await {
            return error(StatusCode::BAD_REQUEST, message);
        }
    }
    match state.snapshots.refresh(&state.store).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn settings(State(state): State<AppState>) -> Response {
    match state.store.settings().await {
        Ok(settings) => Json(json!(settings)).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn analytics(
    State(state): State<AppState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let period = query.get("period").map(String::as_str).unwrap_or("30 days");
    match state.store.analytics_for(period).await {
        Ok(analytics) => Json(analytics).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn groups(State(state): State<AppState>) -> Response {
    match state.store.channel_groups().await {
        Ok(groups) => Json(groups).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn set_group(
    State(state): State<AppState>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let Some(id) = input.get("id").and_then(serde_json::Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "group id required");
    };
    let Some(name) = input.get("name").and_then(serde_json::Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "group name required");
    };
    let description = input
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    match state.store.set_group(id, name, description).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn provider_enabled(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let enabled = input
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    match state.store.set_provider_enabled(id, enabled).await {
        Ok(()) => {
            let _ = state.snapshots.refresh(&state.store).await;
            StatusCode::OK.into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn route_enabled(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let enabled = input
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    match state.store.set_route_enabled(id, enabled).await {
        Ok(()) => {
            let _ = state.snapshots.refresh(&state.store).await;
            StatusCode::OK.into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn delete_route(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.store.delete_route(id).await {
        Ok(()) => {
            let _ = state.snapshots.refresh(&state.store).await;
            StatusCode::OK.into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn client_key_enabled(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let enabled = input
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    match state.store.set_client_key_enabled(id, enabled).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn clear_requests(State(state): State<AppState>) -> Response {
    match state.store.clear_requests().await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
pub fn router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/providers", get(providers).post(create_provider))
        .route("/providers/test", post(preview_provider_models))
        .route("/providers/{id}", axum::routing::put(update_provider))
        .route("/routes/{id}", axum::routing::put(update_route))
        .route("/providers/{id}/test", post(test_provider))
        .route("/providers/{id}/keys", post(add_key))
        .route("/providers/{id}/enabled", post(provider_enabled))
        .route("/routes/{id}/enabled", post(route_enabled))
        .route("/routes/{id}", axum::routing::delete(delete_route))
        .route("/models/pricing", post(set_price))
        .route("/routes", get(routes).post(create_route))
        .route("/keys", get(keys).post(create_key))
        .route("/keys/{id}/revoke", post(revoke_key))
        .route("/keys/{id}/policy", post(set_policy))
        .route("/keys/{id}/enabled", post(client_key_enabled))
        .route("/groups", get(groups).post(set_group))
        .route("/settings", get(settings).post(update_settings))
        .route("/analytics", get(analytics))
        .route("/requests", get(requests).delete(clear_requests))
        .route("/requests/{id}/attempts", get(attempts))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/auth/status", get(status))
        .route("/auth/bootstrap", post(bootstrap))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(session))
        .layer(middleware::from_fn(public_origin))
        .merge(protected)
}
