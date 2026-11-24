use crate::store::UserRole;
use crate::updates::ProviderUpdate;
use crate::{NewProvider, NewRoute, UpsertClientKey, server::AppState};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
#[derive(Deserialize)]
struct UserInput {
    username: String,
    password: String,
    role: UserRole,
}
#[derive(Deserialize)]
struct RoleInput {
    role: UserRole,
}
#[derive(Deserialize)]
struct PasswordInput {
    password: String,
}

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
    #[serde(default)]
    key_models: Vec<KeyModelsInput>,
    #[serde(default = "default_group")]
    group_id: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    plugin_id: Option<String>,
    #[serde(default = "default_mode")]
    mode: String,
    /// Unified-channel providers: ordered addresses, each with its own keys.
    /// Ignored when `mode` is `single`.
    #[serde(default)]
    endpoints: Vec<ProviderEndpointInput>,
    /// Built-in outbound request rewriting (headers/params/prompt/defaults).
    #[serde(default)]
    headers: Vec<crate::rewrite::HeaderRule>,
    #[serde(default)]
    param_replace: Vec<crate::rewrite::ParamReplaceRule>,
    #[serde(default)]
    prompt_replace: Vec<crate::rewrite::PromptReplaceRule>,
    #[serde(default)]
    param_defaults: Vec<crate::rewrite::ParamDefaultRule>,
}
#[derive(Deserialize)]
pub struct ProviderPreviewInput {
    protocol: crate::ProviderProtocol,
    endpoint: String,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    secrets: Vec<String>,
    #[serde(default)]
    plugin_id: Option<String>,
}
#[derive(Deserialize, Clone)]
pub struct KeyModelsInput {
    secret: String,
    models: Vec<String>,
}
#[derive(Deserialize, Clone)]
pub struct ProviderEndpointInput {
    endpoint: String,
    #[serde(default)]
    secrets: Vec<String>,
    #[serde(default)]
    key_models: Vec<KeyModelsInput>,
}
fn default_mode() -> String {
    "single".into()
}
#[derive(Deserialize)]
pub struct ProviderKeyModelsUpdate {
    models: Option<Vec<String>>,
}
#[derive(Deserialize, Default)]
pub struct ProviderTestInput {
    protocol: Option<crate::ProviderProtocol>,
    endpoint: Option<String>,
    secret: Option<String>,
    #[serde(default)]
    key_index: Option<usize>,
    #[serde(default)]
    plugin_id: Option<String>,
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
    /// Ordering strategy id; omitted keeps the default `priority`.
    #[serde(default)]
    strategy: Option<String>,
    /// Per-candidate weights parallel to `candidates`; omitted defaults to 1.
    #[serde(default)]
    weights: Option<Vec<i64>>,
    /// Built-in outbound request rewriting; overrides the provider's rules.
    #[serde(default)]
    headers: Vec<crate::rewrite::HeaderRule>,
    #[serde(default)]
    param_replace: Vec<crate::rewrite::ParamReplaceRule>,
    #[serde(default)]
    prompt_replace: Vec<crate::rewrite::PromptReplaceRule>,
    #[serde(default)]
    param_defaults: Vec<crate::rewrite::ParamDefaultRule>,
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
    #[serde(default)]
    secret: Option<String>,
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
        Ok((token, role)) => Response::builder()
            .header(
                header::SET_COOKIE,
                format!(
                    "sepia_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{}",
                    if state.secure_cookie { "; Secure" } else { "" }
                ),
            )
            .body(
                Json(json!({"authenticated":true,"username":input.username,"role":role}))
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
            Ok(Some(session)) => Json(json!({
                "username":session.username,
                "role":session.role,
                "can_edit":session.role.can_edit()
            }))
            .into_response(),
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
        Ok(Some(session)) => {
            if request.method() != axum::http::Method::GET {
                // The router is nested under /api/v1, so axum strips that prefix
                // from the URI seen here; accept both forms defensively.
                let raw = request.uri().path();
                let path = raw.strip_prefix("/api/v1").unwrap_or(raw);
                let admin_only = path.starts_with("/users")
                    || path.starts_with("/settings")
                    || path.starts_with("/update")
                    || path.starts_with("/ai/pool")
                    || path == "/keys"
                    || path.starts_with("/keys/")
                    || path.starts_with("/plugins/")
                    || (path.starts_with("/requests")
                        && request.method() == axum::http::Method::DELETE);
                if admin_only && session.role != UserRole::Admin {
                    return error(StatusCode::FORBIDDEN, "administrator role required");
                }
                // Passkey self-service: every role manages its own
                // credentials (register/delete own passkeys).
                let self_service = path.starts_with("/auth/passkey/register/")
                    || path == "/passkeys"
                    || path.starts_with("/passkeys/");
                if !admin_only && !self_service && !session.role.can_edit() {
                    return error(StatusCode::FORBIDDEN, "read-only role");
                }
            }
            let mut request = request;
            request.extensions_mut().insert(session);
            next.run(request).await
        }
        _ => error(StatusCode::UNAUTHORIZED, "invalid session"),
    }
}
async fn users(State(state): State<AppState>) -> Response {
    match state.store.users().await {
        Ok(users) => Json(json!({
            "users": users.iter().map(|user| json!({
                "id":user.id,"username":user.username,"role":user.role,"created_at":user.created_at
            })).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn create_user(State(state): State<AppState>, Json(input): Json<UserInput>) -> Response {
    match state
        .store
        .create_user(&input.username, &input.password, input.role)
        .await
    {
        Ok(user) => Json(json!({
            "id":user.id,"username":user.username,"role":user.role,"created_at":user.created_at
        }))
        .into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn set_user_role(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<RoleInput>,
) -> Response {
    match state.store.set_user_role(id, input.role).await {
        Ok(()) => Json(json!({"updated":true})).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn reset_user_password(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<PasswordInput>,
) -> Response {
    match state.store.reset_user_password(id, &input.password).await {
        Ok(()) => Json(json!({"updated":true})).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn delete_user(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.store.delete_user(id).await {
        Ok(()) => Json(json!({"deleted":true})).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn providers(State(state): State<AppState>) -> Response {
    let providers = match state.store.providers().await {
        Ok(providers) => providers,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut output = Vec::new();
    for provider in providers {
        let keys = match state.store.provider_keys(provider.id).await {
            Ok(keys) => keys,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        let mode = match state.store.provider_mode(provider.id).await {
            Ok(mode) => mode,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        let endpoints = match state.store.provider_endpoints(provider.id).await {
            Ok(endpoints) => endpoints,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        output.push(json!({"id":provider.id,"name":provider.name,"protocol":provider.protocol,"endpoint":provider.endpoint,"models":provider.models,"key_count":provider.key_count,"group_id":provider.group_id,"enabled":provider.enabled,"plugin_id":provider.plugin_id,"mode":mode,"headers":provider.rewrite.headers,"param_replace":provider.rewrite.param_replace,"prompt_replace":provider.rewrite.prompt_replace,"param_defaults":provider.rewrite.param_defaults,"keys":keys.iter().map(|key| json!({"id":key.id,"models":key.models,"endpoint_id":key.endpoint_id})).collect::<Vec<_>>(),"endpoints":endpoints.iter().map(|endpoint| json!({"id":endpoint.id,"endpoint":endpoint.endpoint,"keys":keys.iter().filter(|key| key.endpoint_id == Some(endpoint.id)).map(|key| json!({"id":key.id,"models":key.models})).collect::<Vec<_>>()})).collect::<Vec<_>>()}));
    }
    Json(output).into_response()
}
async fn sync_model_metadata(State(state): State<AppState>) -> Response {
    let source = state.metadata_source.clone();
    let body = match state.http().get(&source).send().await {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(body) => body,
            Err(message) => {
                return error(
                    StatusCode::BAD_GATEWAY,
                    format!("metadata source unreadable: {message}"),
                );
            }
        },
        Err(message) => {
            return error(
                StatusCode::BAD_GATEWAY,
                format!("metadata source unreachable: {message}"),
            );
        }
    };
    let entries: Vec<(String, crate::metadata::ModelMeta)> = body
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(crate::metadata::parse_entry)
                .collect()
        })
        .unwrap_or_default();
    if entries.is_empty() {
        return error(
            StatusCode::BAD_GATEWAY,
            "metadata source returned no models".to_string(),
        );
    }
    let prices_filled = match state.store.replace_model_metadata(&source, &entries).await {
        Ok(filled) => filled,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    // Prices/contexts feed cost-optimized and context-fit ordering.
    let _ = state.snapshots.refresh(&state.store).await;
    let (_, fetched_at) = state.store.catalog_meta().await.unwrap_or((None, None));
    Json(json!({"count": entries.len(), "prices_filled": prices_filled, "source": source, "fetched_at": fetched_at}))
        .into_response()
}
async fn model_catalog(State(state): State<AppState>) -> Response {
    let providers = match state.store.providers().await {
        Ok(providers) => providers,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let routes = match state.store.routes().await {
        Ok(routes) => routes,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let prices = match state.store.model_prices().await {
        Ok(prices) => prices,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let metadata = match state.store.model_metadata().await {
        Ok(metadata) => metadata,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let (source, fetched_at) = match state.store.catalog_meta().await {
        Ok(meta) => meta,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut models: Vec<(String, serde_json::Value)> = Vec::new();
    for provider in &providers {
        let keys = match state.store.provider_keys(provider.id).await {
            Ok(keys) => keys,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        for model in &provider.models {
            let price = prices
                .iter()
                .find(|(provider_id, priced, _, _)| *provider_id == provider.id && priced == model);
            let entry = json!({
                "provider_id": provider.id,
                "provider": provider.name,
                "group_id": provider.group_id,
                "protocol": provider.protocol,
                "enabled": provider.enabled,
                "key_count": keys.iter().filter(|key| key.models.as_ref().is_none_or(|models| models.iter().any(|item| item == model))).count(),
                "input_price_micros": price.map(|(_, _, input, _)| *input),
                "output_price_micros": price.map(|(_, _, _, output)| *output),
                "routes": routes.iter().filter(|route| route.enabled && route.candidates.iter().any(|(_, candidate)| candidate == model)).map(|route| json!({"slug":route.slug,"name":route.name,"strategy":route.strategy})).collect::<Vec<_>>(),
            });
            match models.iter_mut().find(|(name, _)| name == model) {
                Some((_, serde_json::Value::Array(channels))) => channels.push(entry),
                _ => models.push((model.clone(), json!([entry]))),
            }
        }
    }
    models.sort_by(|(left, _), (right, _)| left.cmp(right));
    Json(json!({
        "source": source,
        "fetched_at": fetched_at,
        "models": models.into_iter().map(|(model, channels)| {
            let meta = crate::metadata::match_meta(&metadata, &model);
            json!({
                "model": model,
                "base_model": crate::metadata::base_model(&model),
                "metadata": meta,
                "channels": channels,
            })
        }).collect::<Vec<_>>(),
    }))
    .into_response()
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
        plugin_id: input.plugin_id.clone(),
        rewrite: Default::default(),
    };
    let result = if provider.plugin_id.is_some() {
        crate::plugin::provider_models(&state, &provider, secret).await
    } else {
        crate::discovery::discover_models(&state.http(), &provider, secret).await
    };
    match result {
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
        key_models,
        group_id,
        enabled,
        plugin_id,
        mode,
        endpoints,
        headers,
        param_replace,
        prompt_replace,
        param_defaults,
    } = input;
    let rewrite = crate::rewrite::RewriteConfig {
        headers,
        param_replace,
        prompt_replace,
        param_defaults,
    };
    if let Err(message) = crate::rewrite::validate(&rewrite) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    if mode != "single" && mode != "unified" {
        return error(StatusCode::BAD_REQUEST, "mode must be single or unified");
    }
    let unified = mode == "unified";
    let (endpoint, secret, secrets, key_models, extra_endpoints) = if unified {
        let mut entries = endpoints;
        for entry in &mut entries {
            entry.secrets = entry
                .secrets
                .iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect();
        }
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|entry| !entry.endpoint.trim().is_empty())
            .collect();
        if entries.is_empty() {
            return error(StatusCode::BAD_REQUEST, "at least one endpoint required");
        }
        for entry in &entries {
            if entry.secrets.is_empty() {
                return error(
                    StatusCode::BAD_REQUEST,
                    "each unified endpoint needs at least one key",
                );
            }
        }
        let first = &entries[0];
        (
            first.endpoint.clone(),
            first.secrets[0].clone(),
            first.secrets[1..].to_vec(),
            first.key_models.clone(),
            entries[1..].to_vec(),
        )
    } else {
        (endpoint, secret, secrets, key_models, Vec::new())
    };
    let key_scopes: Vec<(String, Vec<String>)> = key_models
        .into_iter()
        .map(|entry| (entry.secret, entry.models))
        .collect();
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
            plugin_id: plugin_id.clone(),
            rewrite: rewrite.clone(),
        };
        let mut discovered = Vec::new();
        let mut first_error = None;
        let mut seen = Vec::new();
        for candidate_secret in std::iter::once(secret.as_str())
            .chain(secrets.iter().map(String::as_str))
            .filter(|value| !value.trim().is_empty())
        {
            if seen.contains(&candidate_secret) {
                continue;
            }
            seen.push(candidate_secret);
            let discovery = if candidate.plugin_id.is_some() {
                crate::plugin::provider_models(&state, &candidate, candidate_secret).await
            } else {
                crate::discovery::discover_models(&state.http(), &candidate, candidate_secret).await
            };
            match discovery {
                Ok(models) => {
                    for model in models {
                        if !discovered.contains(&model) {
                            discovered.push(model);
                        }
                    }
                }
                Err(message) => {
                    first_error.get_or_insert(message);
                }
            };
        }
        if discovered.is_empty() {
            return error(
                StatusCode::BAD_GATEWAY,
                first_error.unwrap_or_else(|| "provider returned no models".into()),
            );
        }
        models = discovered;
    }
    match state
        .store
        .create_provider(
            NewProvider {
                name,
                protocol,
                endpoint,
                models,
                plugin_id: plugin_id.clone(),
                mode: mode.clone(),
                rewrite: rewrite.clone(),
            },
            &secret,
            &key_scopes,
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
                    plugin_id: plugin_id.as_deref(),
                    mode: &mode,
                    rewrite: &rewrite,
                })
                .await
            {
                return error(StatusCode::BAD_REQUEST, message);
            }
            for additional_secret in secrets.iter().filter(|additional_secret| {
                !additional_secret.is_empty() && **additional_secret != secret
            }) {
                let scope = key_scopes
                    .iter()
                    .find(|(key, _)| key == additional_secret)
                    .map(|(_, models)| models.as_slice());
                if let Err(message) = state
                    .store
                    .add_provider_key(provider.id, additional_secret, scope, None, &state.keys)
                    .await
                {
                    return error(StatusCode::BAD_REQUEST, message);
                }
            }
            for entry in &extra_endpoints {
                let endpoint_id = match state
                    .store
                    .append_provider_endpoint(provider.id, &entry.endpoint)
                    .await
                {
                    Ok(id) => id,
                    Err(message) => return error(StatusCode::BAD_REQUEST, message),
                };
                for entry_secret in &entry.secrets {
                    let scope = entry
                        .key_models
                        .iter()
                        .find(|scoped| scoped.secret == *entry_secret)
                        .map(|scoped| scoped.models.as_slice());
                    if let Err(message) = state
                        .store
                        .add_provider_key(
                            provider.id,
                            entry_secret,
                            scope,
                            Some(endpoint_id),
                            &state.keys,
                        )
                        .await
                    {
                        return error(StatusCode::BAD_REQUEST, message);
                    }
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
    match state.store.routes().await { Ok(routes) => Json(json!(routes.into_iter().map(|route| json!({"id":route.id,"slug":route.slug,"name":route.name,"description":route.description,"patterns":route.patterns,"is_default":route.is_default,"list_members":route.list_members,"enabled":route.enabled,"candidates":route.candidates,"strategy":route.strategy,"weights":route.weights,"headers":route.rewrite.headers,"param_replace":route.rewrite.param_replace,"prompt_replace":route.rewrite.prompt_replace,"param_defaults":route.rewrite.param_defaults})).collect::<Vec<_>>())).into_response(), Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message) }
}
async fn create_route(State(state): State<AppState>, Json(input): Json<RouteInput>) -> Response {
    let rewrite = crate::rewrite::RewriteConfig {
        headers: input.headers,
        param_replace: input.param_replace,
        prompt_replace: input.prompt_replace,
        param_defaults: input.param_defaults,
    };
    if let Err(message) = crate::rewrite::validate(&rewrite) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let Some((provider_id, model)) = input.candidates.first().cloned() else {
        return error(StatusCode::BAD_REQUEST, "at least one candidate required");
    };
    let strategy = input.strategy.as_deref().unwrap_or("priority");
    if crate::strategy::Strategy::parse(strategy).is_none() {
        return error(
            StatusCode::BAD_REQUEST,
            format!("unknown route strategy: {strategy}"),
        );
    }
    let weights = input.weights.unwrap_or_default();
    if !weights.is_empty() && weights.len() != input.candidates.len() {
        return error(StatusCode::BAD_REQUEST, "weights must match candidates");
    }
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
            rewrite,
        })
        .await
    {
        Ok(route) => {
            if let Err(message) = state.store.set_route_enabled(route.id, input.enabled).await {
                return error(StatusCode::BAD_REQUEST, message);
            }
            if let Err(message) = state
                .store
                .set_route_strategy(route.id, strategy, &weights)
                .await
            {
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
    let rewrite = crate::rewrite::RewriteConfig {
        headers: input.headers,
        param_replace: input.param_replace,
        prompt_replace: input.prompt_replace,
        param_defaults: input.param_defaults,
    };
    if let Err(message) = crate::rewrite::validate(&rewrite) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    if input.mode != "single" && input.mode != "unified" {
        return error(StatusCode::BAD_REQUEST, "mode must be single or unified");
    }
    let mut endpoint = input.endpoint.clone();
    if input.mode == "unified" {
        let addresses: Vec<String> = input
            .endpoints
            .iter()
            .map(|entry| entry.endpoint.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        if addresses.is_empty() {
            return error(StatusCode::BAD_REQUEST, "at least one endpoint required");
        }
        endpoint = addresses[0].clone();
        let synced_endpoints = match state.store.sync_provider_endpoints(id, &addresses).await {
            Ok(endpoints) => endpoints,
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        };
        for (entry, stored) in input.endpoints.iter().zip(&synced_endpoints) {
            for entry_secret in entry
                .secrets
                .iter()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                let scope = entry
                    .key_models
                    .iter()
                    .find(|scoped| scoped.secret == entry_secret)
                    .map(|scoped| scoped.models.as_slice());
                if let Err(message) = state
                    .store
                    .add_provider_key(id, entry_secret, scope, Some(stored.id), &state.keys)
                    .await
                {
                    return error(StatusCode::BAD_REQUEST, message);
                }
            }
        }
    }
    match state
        .store
        .update_provider(ProviderUpdate {
            id,
            name: &input.name,
            protocol: input.protocol,
            endpoint: &endpoint,
            models: &input.models,
            group: &input.group_id,
            enabled: input.enabled,
            plugin_id: input.plugin_id.as_deref(),
            mode: &input.mode,
            rewrite: &rewrite,
        })
        .await
    {
        Ok(()) => {
            if !input.key_models.is_empty() {
                let keys = match state.store.provider_keys(id).await {
                    Ok(keys) => keys,
                    Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
                };
                for entry in &input.key_models {
                    for key in &keys {
                        let Ok(secret) = state
                            .store
                            .upstream_key_by_id(id, key.id, &state.keys)
                            .await
                        else {
                            continue;
                        };
                        if secret == entry.secret {
                            if let Err(message) = state
                                .store
                                .set_provider_key_models(id, key.id, Some(&entry.models))
                                .await
                            {
                                return error(StatusCode::BAD_REQUEST, message);
                            }
                            break;
                        }
                    }
                }
            }
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
    let rewrite = crate::rewrite::RewriteConfig {
        headers: input.headers,
        param_replace: input.param_replace,
        prompt_replace: input.prompt_replace,
        param_defaults: input.param_defaults,
    };
    if let Err(message) = crate::rewrite::validate(&rewrite) {
        return error(StatusCode::BAD_REQUEST, message);
    }
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
                strategy: input.strategy.as_deref(),
                weights: input.weights.as_deref(),
                rewrite: &rewrite,
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
    let keys = match state.store.client_keys().await {
        Ok(keys) => keys,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut records = Vec::new();
    for key in keys {
        let model_groups = match state.store.client_policy(key.id).await {
            Ok(policy) => policy.model_groups,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        records.push(json!({"id":key.id,"name":key.name,"models":key.models,"token_limit":key.token_limit,"max_amount_micros":key.max_amount_micros,"rpm":key.rpm,"tpm":key.tpm,"expires_at":key.expires_at,"model_groups":model_groups}));
    }
    Json(records).into_response()
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
            secret: input.secret,
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
async fn request_bodies(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.request_bodies(&id).await {
        Ok(Some((request_body, response_body))) => {
            Json(json!({"request_body":request_body,"response_body":response_body})).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "no bodies recorded for this request"),
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
    if input.plugin_id.is_some() {
        provider.plugin_id = input.plugin_id.clone();
    }
    let secret = match resolve_test_secret(&state, id, input.secret, input.key_index).await {
        Ok(secret) => secret,
        Err(response) => return *response,
    };
    let result = if provider.plugin_id.is_some() {
        crate::plugin::provider_models(&state, &provider, &secret).await
    } else {
        crate::discovery::discover_models(&state.http(), &provider, &secret).await
    };
    match result {
        Ok(models) => Json(json!({"models":models})).into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, message),
    }
}
async fn resolve_test_secret(
    state: &AppState,
    provider_id: i64,
    input_secret: Option<String>,
    key_index: Option<usize>,
) -> Result<String, Box<Response>> {
    if let Some(secret) = input_secret.filter(|secret| !secret.trim().is_empty()) {
        return Ok(secret);
    }
    // Detection targets the provider's first address only: unified channels
    // share one model catalog across addresses, and key_index addresses the
    // first address's keys (the list the dialog shows for detection).
    let first_endpoint = state
        .store
        .provider_endpoints(provider_id)
        .await
        .ok()
        .and_then(|endpoints| endpoints.first().map(|endpoint| endpoint.id));
    let keys = state
        .store
        .provider_keys(provider_id)
        .await
        .map_err(|message| Box::new(error(StatusCode::INTERNAL_SERVER_ERROR, message)))?;
    let keys: Vec<_> = keys
        .into_iter()
        .filter(|key| {
            first_endpoint.is_none_or(|endpoint| key.endpoint_id.is_none_or(|id| id == endpoint))
        })
        .collect();
    match key_index {
        Some(index) => {
            let Some(key) = keys.get(index) else {
                return Err(Box::new(error(
                    StatusCode::BAD_REQUEST,
                    "provider key not found",
                )));
            };
            state
                .store
                .upstream_key_by_id(provider_id, key.id, &state.keys)
                .await
                .map_err(|message| Box::new(error(StatusCode::BAD_REQUEST, message)))
        }
        None => match keys.first() {
            Some(key) => state
                .store
                .upstream_key_by_id(provider_id, key.id, &state.keys)
                .await
                .map_err(|message| Box::new(error(StatusCode::BAD_REQUEST, message))),
            None => Err(Box::new(error(
                StatusCode::BAD_REQUEST,
                "provider has no enabled key",
            ))),
        },
    }
}
async fn test_route(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let route = match state.store.routes().await {
        Ok(routes) => match routes.into_iter().find(|route| route.id == id) {
            Some(route) => route,
            None => return error(StatusCode::NOT_FOUND, "route not found"),
        },
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let providers = match state.store.providers().await {
        Ok(providers) => providers,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut results = Vec::new();
    for (provider_id, model) in &route.candidates {
        let Some(provider) = providers
            .iter()
            .find(|provider| provider.id == *provider_id)
        else {
            results.push(json!({"provider_id":provider_id,"provider":null,"model":model,"enabled":false,"result":{"ok":false,"latency_ms":0,"endpoint":null,"error":"provider not found","reply":null}}));
            continue;
        };
        if !provider.enabled {
            results.push(json!({"provider_id":provider.id,"provider":provider.name,"model":model,"enabled":false,"result":{"ok":false,"latency_ms":0,"endpoint":null,"error":"provider disabled","reply":null}}));
            continue;
        }
        let secret = match state
            .store
            .upstream_key_for_attempt(
                provider.id,
                None,
                model,
                0,
                &Default::default(),
                &state.keys,
            )
            .await
        {
            Ok(Some((_, secret))) => secret,
            Ok(None) => {
                results.push(json!({"provider_id":provider.id,"provider":provider.name,"model":model,"enabled":true,"result":{"ok":false,"latency_ms":0,"endpoint":null,"error":"no key serves this model","reply":null}}));
                continue;
            }
            Err(message) => {
                results.push(json!({"provider_id":provider.id,"provider":provider.name,"model":model,"enabled":true,"result":{"ok":false,"latency_ms":0,"endpoint":null,"error":message,"reply":null}}));
                continue;
            }
        };
        let legs = crate::server::candidate_protocols(
            provider,
            &crate::server::InboundProtocol::Provider(crate::protocol::Protocol::OpenAI),
        );
        let outcome =
            crate::probe::probe_model_for(&state, provider, &secret, model, &legs, &route.rewrite)
                .await;
        results.push(json!({"provider_id":provider.id,"provider":provider.name,"model":model,"enabled":true,"result":outcome.to_json()}));
    }
    Json(json!({"route":route.slug,"candidates":results})).into_response()
}
async fn add_key(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let Some(secret) = input.get("secret").and_then(serde_json::Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "secret required");
    };
    let models = match input.get("models") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => match serde_json::from_value::<Vec<String>>(value.clone()) {
            Ok(models) => Some(models),
            Err(_) => return error(StatusCode::BAD_REQUEST, "models must be a string array"),
        },
    };
    let endpoint_id = input.get("endpoint_id").and_then(serde_json::Value::as_i64);
    match state
        .store
        .add_provider_key(id, secret, models.as_deref(), endpoint_id, &state.keys)
        .await
    {
        Ok(id) => Json(json!({"id":id})).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn list_provider_keys(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.store.provider_keys(id).await {
        Ok(keys) => Json(json!(
            keys.into_iter()
                .map(|key| json!({"id":key.id,"models":key.models,"endpoint_id":key.endpoint_id}))
                .collect::<Vec<_>>()
        ))
        .into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn update_provider_key_models(
    State(state): State<AppState>,
    Path((id, key_id)): Path<(i64, i64)>,
    Json(input): Json<ProviderKeyModelsUpdate>,
) -> Response {
    match state
        .store
        .set_provider_key_models(id, key_id, input.models.as_deref())
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
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
        Ok(()) => {
            let _ = state.snapshots.refresh(&state.store).await;
            StatusCode::OK.into_response()
        }
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
async fn model_groups(State(state): State<AppState>) -> Response {
    match state.store.model_groups().await {
        Ok(groups) => Json(json!({"groups":groups})).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
#[derive(Deserialize)]
struct ModelGroupBody {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    members: Vec<String>,
}
async fn create_model_group(
    State(state): State<AppState>,
    Json(body): Json<ModelGroupBody>,
) -> Response {
    match state
        .store
        .create_model_group(&body.name, &body.description, &body.members)
        .await
    {
        Ok(group) => Json(group).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn update_model_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ModelGroupBody>,
) -> Response {
    match state
        .store
        .update_model_group(id, &body.name, &body.description, &body.members)
        .await
    {
        Ok(group) => Json(group).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}
async fn delete_model_group(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.store.delete_model_group(id).await {
        Ok(()) => Json(json!({"deleted":true})).into_response(),
        Err(message) if message.contains("referenced") => error(StatusCode::CONFLICT, message),
        Err(message) => error(StatusCode::NOT_FOUND, message),
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
async fn list_plugins(State(state): State<AppState>) -> Response {
    match state.store.plugin_records().await {
        Ok(records) => Json(json!(
            records
                .into_iter()
                .map(|record| {
                    json!({
                        "id": record.id,
                        "name": record.name,
                        "version": record.version,
                        "enabled": record.enabled,
                        "status": record.status,
                        "description": record.manifest.description,
                        "events": record.manifest.events,
                        "permissions": record.manifest.permissions,
                        "priority": record.manifest.priority,
                        "has_ui": record.manifest.ui.is_some(),
                        "ui_title": record.manifest.ui.as_ref().map(|ui| ui.title.clone()).unwrap_or_default(),
                        "config_schema": record.manifest.config_schema,
                        "config": record.config,
                        "sha256": record.sha256,
                        "installed_at": record.installed_at,
                        "updated_at": record.updated_at,
                    })
                })
                .collect::<Vec<_>>()
        ))
        .into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
async fn install_plugin(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let mut package: Option<Vec<u8>> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("package") {
            match field.bytes().await {
                Ok(bytes) => package = Some(bytes.to_vec()),
                Err(message) => return error(StatusCode::BAD_REQUEST, message.to_string()),
            }
        }
    }
    let Some(package) = package else {
        return error(StatusCode::BAD_REQUEST, "package field required");
    };
    let parsed = match crate::plugin::parse_package(&package) {
        Ok(parsed) => parsed,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let existing = match state.store.plugin(&parsed.manifest.id).await {
        Ok(existing) => existing,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let now = chrono::Utc::now().to_rfc3339();
    let mut ui_bundle = serde_json::Map::new();
    for (path, bytes) in &parsed.ui {
        use base64::Engine;
        ui_bundle.insert(
            path.clone(),
            json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
        );
    }
    let previous_version = existing.as_ref().map(|record| record.version.clone());
    let (record, replace) = match existing {
        Some(existing) => {
            let old = semver::Version::parse(&existing.version).ok();
            let new = semver::Version::parse(&parsed.manifest.version).ok();
            if let (Some(old), Some(new)) = (old, new)
                && new < old
            {
                return error(StatusCode::BAD_REQUEST, "downgrade not allowed");
            }
            let mut record = existing.clone();
            record.name = parsed.manifest.name.clone();
            record.version = parsed.manifest.version.clone();
            record.manifest = parsed.manifest.clone();
            record.wasm = parsed.wasm.clone();
            record.ui_bundle = Value::Object(ui_bundle);
            record.sha256 = parsed.sha256.clone();
            record.updated_at = now.clone();
            record.status = if record.enabled {
                "enabled"
            } else {
                "disabled"
            }
            .into();
            (record, true)
        }
        None => (
            crate::store::PluginRecord {
                id: parsed.manifest.id.clone(),
                name: parsed.manifest.name.clone(),
                version: parsed.manifest.version.clone(),
                enabled: false,
                status: "disabled".into(),
                manifest: parsed.manifest.clone(),
                config: json!({}),
                wasm: parsed.wasm.clone(),
                ui_bundle: Value::Object(ui_bundle),
                sha256: parsed.sha256.clone(),
                installed_at: now.clone(),
                updated_at: now.clone(),
            },
            false,
        ),
    };
    if let Err(message) = state.store.insert_plugin(&record, replace).await {
        return error(StatusCode::BAD_REQUEST, message);
    }
    if replace && record.enabled {
        let runtime = state.plugins.read().await;
        let _ = runtime
            .invoke(
                &state.store,
                &state.keys,
                &state.http(),
                &record,
                "plugin.migrate",
                json!({"from_version": previous_version.clone().unwrap_or_default()}),
                None,
            )
            .await;
    }
    if let Err(message) = state.plugins.write().await.reload(&state.store).await {
        return error(StatusCode::INTERNAL_SERVER_ERROR, message);
    }
    Json(json!({"id": record.id, "version": record.version})).into_response()
}
async fn set_plugin_enabled(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let enabled = input
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let Some(record) = state.store.plugin(&id).await.ok().flatten() else {
        return error(StatusCode::NOT_FOUND, "plugin not found");
    };
    if record.status == "uninstalling" {
        return error(StatusCode::CONFLICT, "plugin is uninstalling");
    }
    let status = if enabled { "enabled" } else { "disabled" };
    if let Err(message) = state.store.set_plugin_enabled(&id, enabled, status).await {
        return error(StatusCode::BAD_REQUEST, message);
    }
    if let Err(message) = state.plugins.write().await.reload(&state.store).await {
        return error(StatusCode::INTERNAL_SERVER_ERROR, message);
    }
    StatusCode::OK.into_response()
}
async fn set_plugin_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(config): Json<serde_json::Value>,
) -> Response {
    if !config.is_object() {
        return error(StatusCode::BAD_REQUEST, "config must be an object");
    }
    if let Err(message) = state.store.set_plugin_config(&id, &config).await {
        return error(StatusCode::BAD_REQUEST, message);
    }
    StatusCode::OK.into_response()
}
async fn delete_plugin(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if let Err(message) = state
        .store
        .set_plugin_enabled(&id, false, "uninstalling")
        .await
    {
        return error(StatusCode::BAD_REQUEST, message);
    }
    if let Err(message) = state.plugins.write().await.reload(&state.store).await {
        return error(StatusCode::INTERNAL_SERVER_ERROR, message);
    }
    if let Err(message) = state.store.delete_plugin(&id).await {
        return error(StatusCode::BAD_REQUEST, message);
    }
    StatusCode::OK.into_response()
}
async fn plugin_ui_bundle(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(record) = state.store.plugin(&id).await.ok().flatten() else {
        return error(StatusCode::NOT_FOUND, "plugin not found");
    };
    if record.manifest.ui.is_none() {
        return error(StatusCode::NOT_FOUND, "plugin has no ui");
    }
    let session = crate::crypto::random_token("sepia_plugin_");
    let document = match crate::plugin::ui_document(&record, &session) {
        Ok(document) => document,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff",
            ),
        ],
        Json(json!({"session": session, "document": document})),
    )
        .into_response()
}
async fn plugin_rpc(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    headers: HeaderMap,
    Json(input): Json<serde_json::Value>,
) -> Response {
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment.chars().all(|character| character == '.'))
        || path.contains(['\\', '%'])
        || path.chars().any(char::is_control)
    {
        return error(StatusCode::BAD_REQUEST, "invalid plugin RPC path");
    }
    let Some(record) = state.store.plugin(&id).await.ok().flatten() else {
        return error(StatusCode::NOT_FOUND, "plugin not found");
    };
    if !record.enabled {
        return error(StatusCode::CONFLICT, "plugin is disabled");
    }
    if !record.manifest.has_event("http.rpc") {
        return error(StatusCode::FORBIDDEN, "plugin does not expose rpc");
    }
    // An admin-authenticated RPC may borrow the invoking session so a plugin
    // with the `admin` permission can drive the management API (e.g. imports).
    let admin = if record.manifest.has_permission("admin") {
        let cookie = headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let forwarded_proto = headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let scheme = if forwarded_proto == "https" || host.ends_with(":443") {
            "https"
        } else {
            "http"
        };
        if host.is_empty() || cookie.is_empty() {
            None
        } else {
            Some(crate::plugin::AdminContext {
                base_url: format!("{scheme}://{host}"),
                cookie,
            })
        }
    } else {
        None
    };
    let runtime = state.plugins.read().await;
    match runtime
        .invoke(
            &state.store,
            &state.keys,
            &state.http(),
            &record,
            "http.rpc",
            json!({"path": path, "body": input}),
            admin,
        )
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, message),
    }
}
/// Management mutation path prefixes → workspace domains the console re-reads.
/// Mirrors the `run(...)` domain sets used by the frontend call sites; paths
/// outside this table (`/auth`, `/users`, `/update`, `/ai/pool`) do not change
/// workspace data, so they publish nothing.
const MUTATION_DOMAINS: &[(&str, &[&str])] = &[
    ("/providers", &["providers", "routes"]),
    ("/routes", &["routes"]),
    ("/models/pricing", &["providers", "routes"]),
    ("/models/metadata", &["providers", "routes"]),
    ("/keys", &["keys"]),
    ("/groups", &["groups"]),
    ("/model-groups", &["modelGroups"]),
    ("/settings", &["settings"]),
    ("/plugins", &["plugins"]),
];

fn mutation_domains(path: &str, method: &axum::http::Method) -> Option<&'static [&'static str]> {
    if path == "/requests" && method == axum::http::Method::DELETE {
        return Some(crate::events::REQUESTS_CHANGED);
    }
    // `/providers/test` and `/routes/{id}/test` only probe upstreams.
    if path.ends_with("/test") {
        return None;
    }
    MUTATION_DOMAINS.iter().find_map(|(prefix, domains)| {
        (path == *prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/')))
        .then_some(*domains)
    })
}

/// Announces the workspace domains a successful management mutation invalidated.
async fn publish_mutation(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let raw = request.uri().path().to_string();
    let response = next.run(request).await;
    // As in `auth`, the router may or may not have stripped the nest prefix.
    let path = raw.strip_prefix("/api/v1").unwrap_or(&raw);
    if method != axum::http::Method::GET
        && response.status().is_success()
        && let Some(domains) = mutation_domains(path, &method)
    {
        state.events.publish(domains);
    }
    response
}

/// Server-Sent Events stream of workspace change hints (`{domains:[...]}`).
/// An empty `domains` list means "refresh everything"; it is also sent when a
/// subscriber lags behind the broadcast channel. Keep-alive comments hold the
/// connection through idle proxies.
async fn events(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let mut keep_alive = tokio::time::interval(std::time::Duration::from_secs(15));
        keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the interval's immediate first tick so a quiet stream waits a
        // full 15 seconds before its first keep-alive.
        keep_alive.tick().await;
        loop {
            tokio::select! {
                received = receiver.recv() => match received {
                    Ok(domains) => {
                        yield Ok::<_, Infallible>(
                            Event::default().data(json!({"domains": domains}).to_string()),
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        yield Ok::<_, Infallible>(
                            Event::default().data(json!({"domains": []}).to_string()),
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                _ = keep_alive.tick() => {
                    yield Ok::<_, Infallible>(Event::default().comment("keep-alive"));
                }
            }
        }
    };
    Sse::new(stream)
}

pub fn router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/providers", get(providers).post(create_provider))
        .route("/events", get(events))
        .route("/providers/test", post(preview_provider_models))
        .route("/providers/{id}", axum::routing::put(update_provider))
        .route("/routes/{id}", axum::routing::put(update_route))
        .route("/providers/{id}/test", post(test_provider))
        .route("/routes/{id}/test", post(test_route))
        .route(
            "/providers/{id}/keys",
            get(list_provider_keys).post(add_key),
        )
        .route(
            "/providers/{id}/keys/{key_id}/models",
            axum::routing::put(update_provider_key_models),
        )
        .route("/providers/{id}/enabled", post(provider_enabled))
        .route("/routes/{id}/enabled", post(route_enabled))
        .route("/routes/{id}", axum::routing::delete(delete_route))
        .route("/models/pricing", post(set_price))
        .route("/models/catalog", get(model_catalog))
        .route("/models/metadata/sync", post(sync_model_metadata))
        .route("/routes", get(routes).post(create_route))
        .route("/keys", get(keys).post(create_key))
        .route("/keys/{id}/revoke", post(revoke_key))
        .route("/keys/{id}/policy", post(set_policy))
        .route("/keys/{id}/enabled", post(client_key_enabled))
        .route("/groups", get(groups).post(set_group))
        .route("/model-groups", get(model_groups).post(create_model_group))
        .route(
            "/model-groups/{id}",
            axum::routing::put(update_model_group).delete(delete_model_group),
        )
        .route("/settings", get(settings).post(update_settings))
        .route("/analytics", get(analytics))
        .route("/requests", get(requests).delete(clear_requests))
        .route("/requests/{id}/attempts", get(attempts))
        .route("/requests/{id}/bodies", get(request_bodies))
        .route("/plugins", get(list_plugins))
        .route("/plugins/install", post(install_plugin))
        .route("/plugins/{id}/enabled", post(set_plugin_enabled))
        .route(
            "/plugins/{id}/config",
            axum::routing::put(set_plugin_config),
        )
        .route("/plugins/{id}", axum::routing::delete(delete_plugin))
        .route("/plugins/{id}/ui-bundle", get(plugin_ui_bundle))
        .route("/plugins/{id}/rpc/{*path}", post(plugin_rpc))
        .route("/update/check", get(crate::self_update::check_update))
        .route("/update", post(crate::self_update::apply_update))
        .route("/users", get(users).post(create_user))
        .route("/users/{id}/role", post(set_user_role))
        .route("/users/{id}/password", post(reset_user_password))
        .route("/users/{id}", axum::routing::delete(delete_user))
        .merge(crate::webauthn::protected_routes())
        .merge(crate::ai::routes())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            publish_mutation,
        ))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/auth/status", get(status))
        .route("/auth/bootstrap", post(bootstrap))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(session))
        .route("/version", get(crate::self_update::version))
        .route(
            "/auth/passkey/login/options",
            post(crate::webauthn::login_options),
        )
        .route(
            "/auth/passkey/login/finish",
            post(crate::webauthn::login_finish),
        )
        .layer(middleware::from_fn(public_origin))
        .merge(protected)
}
