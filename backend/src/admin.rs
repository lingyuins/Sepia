use crate::{NewProvider, NewRoute, UpsertClientKey, server::AppState};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

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
}
#[derive(Deserialize)]
pub struct RouteInput {
    slug: String,
    patterns: Vec<String>,
    candidates: Vec<(i64, String)>,
    #[serde(default)]
    is_default: bool,
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
        Ok(()) => Json(json!({"initialized":true})).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn login(State(state): State<AppState>, Json(input): Json<Credentials>) -> Response {
    match state.store.login(&input.username, &input.password).await {
        Ok(token) => Response::builder()
            .header(
                header::SET_COOKIE,
                format!("sepia_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200"),
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
            "sepia_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
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
    let Some(token) = session_cookie(request.headers()) else {
        return error(StatusCode::UNAUTHORIZED, "missing session");
    };
    match state.store.session(token).await {
        Ok(Some(_)) => next.run(request).await,
        _ => error(StatusCode::UNAUTHORIZED, "invalid session"),
    }
}
async fn providers(State(state): State<AppState>) -> Response {
    match state.store.providers().await { Ok(providers) => Json(json!(providers.into_iter().map(|p| json!({"id":p.id,"name":p.name,"protocol":p.protocol,"endpoint":p.endpoint,"models":p.models,"key_count":p.key_count})).collect::<Vec<_>>())).into_response(), Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message) }
}
async fn create_provider(
    State(state): State<AppState>,
    Json(input): Json<ProviderInput>,
) -> Response {
    match state
        .store
        .create_provider(
            NewProvider {
                name: input.name,
                protocol: input.protocol,
                endpoint: input.endpoint,
                models: input.models,
            },
            &input.secret,
            &state.keys,
        )
        .await
    {
        Ok(provider) => {
            Json(json!({"id":provider.id,"key_count":provider.key_count})).into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn routes(State(state): State<AppState>) -> Response {
    match state.store.routes().await { Ok(routes) => Json(json!(routes.into_iter().map(|route| json!({"id":route.id,"slug":route.slug,"patterns":route.patterns,"is_default":route.is_default,"candidates":route.candidates})).collect::<Vec<_>>())).into_response(), Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message) }
}
async fn create_route(State(state): State<AppState>, Json(input): Json<RouteInput>) -> Response {
    let Some((provider_id, model)) = input.candidates.first().cloned() else {
        return error(StatusCode::BAD_REQUEST, "at least one candidate required");
    };
    match state
        .store
        .create_route(NewRoute {
            slug: input.slug,
            patterns: input.patterns,
            provider_id,
            model,
            is_default: input.is_default,
            candidates: input.candidates,
        })
        .await
    {
        Ok(route) => Json(json!({"id":route.id})).into_response(),
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
async fn test_provider(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let provider = match state.store.providers().await {
        Ok(providers) => match providers.into_iter().find(|provider| provider.id == id) {
            Some(provider) => provider,
            None => return error(StatusCode::NOT_FOUND, "provider not found"),
        },
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let secret = match state.store.upstream_key(id, &state.keys).await {
        Ok(secret) => secret,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    match crate::discovery::discover_models(&state.http, &provider, &secret).await {
        Ok(models) => Json(json!({"models":models})).into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, message),
    }
}
pub fn router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/providers", get(providers).post(create_provider))
        .route("/providers/{id}/test", post(test_provider))
        .route("/routes", get(routes).post(create_route))
        .route("/keys", get(keys).post(create_key))
        .route("/keys/{id}/revoke", post(revoke_key))
        .route("/requests", get(requests))
        .route("/requests/{id}/attempts", get(attempts))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/auth/bootstrap", post(bootstrap))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(session))
        .merge(protected)
}
