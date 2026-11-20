use crate::{server::AppState, store::AdminSession};
use axum::{
    Extension, Json, Router,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use url::Url;
use webauthn_rs_core::{
    WebauthnCore,
    proto::{
        AuthenticationState, Credential, CredentialID, PublicKeyCredential,
        RegisterPublicKeyCredential, RegistrationState, UserVerificationPolicy,
    },
};

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":{"message":message.into()}}))).into_response()
}

/// Build the WebAuthn relying party from the request host so passkeys work on
/// localhost (development) and on the deployment domain without configuration.
fn core(headers: &HeaderMap) -> Result<WebauthnCore, String> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or("missing host header")?;
    // Bracketed IPv6 authorities ([::1]:5000) keep their address; a plain
    // host:port drops only the trailing port.
    let rp_id = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    let mut origins = Vec::new();
    for candidate in [
        format!("http://{host}"),
        format!("https://{host}"),
        "http://localhost:5173".to_string(),
        "http://127.0.0.1:5173".to_string(),
    ] {
        let url = Url::parse(&candidate).map_err(|e| e.to_string())?;
        if !origins.contains(&url) {
            origins.push(url);
        }
    }
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        && (origin.starts_with("http://") || origin.starts_with("https://"))
    {
        let url = Url::parse(origin).map_err(|e| e.to_string())?;
        if !origins.contains(&url) {
            origins.push(url);
        }
    }
    Ok(WebauthnCore::new_unsafe_experts_only(
        "Sepia",
        rp_id,
        origins,
        Duration::from_secs(60),
        Some(false),
        Some(true),
    ))
}

async fn register_options(
    State(state): State<AppState>,
    Extension(session): Extension<AdminSession>,
    headers: HeaderMap,
) -> Response {
    let user = match state.store.user_by_name(&session.username).await {
        Ok(Some(user)) => user,
        _ => return error(StatusCode::UNAUTHORIZED, "invalid session"),
    };
    let (username, handle, existing) = match state.store.passkey_user(user.id).await {
        Ok(value) => value,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let wa = match core(&headers) {
        Ok(wa) => wa,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let builder = match wa.new_challenge_register_builder(handle.as_bytes(), &username, &username) {
        Ok(builder) => builder,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let exclude: Vec<CredentialID> = existing;
    let (options, registration) = match wa.generate_challenge_register(
        builder
            .user_verification_policy(UserVerificationPolicy::Required)
            .require_resident_key(true)
            .exclude_credentials(Some(exclude)),
    ) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let serialized = match serde_json::to_string(&registration) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    match state
        .store
        .create_passkey_challenge("register", Some(user.id), &serialized)
        .await
    {
        Ok(challenge) => Json(json!({"challenge":challenge,"options":options})).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

#[derive(Deserialize)]
struct RegisterFinish {
    challenge: String,
    name: String,
    credential: RegisterPublicKeyCredential,
}

async fn register_finish(
    State(state): State<AppState>,
    Extension(session): Extension<AdminSession>,
    headers: HeaderMap,
    Json(input): Json<RegisterFinish>,
) -> Response {
    let user = match state.store.user_by_name(&session.username).await {
        Ok(Some(user)) => user,
        _ => return error(StatusCode::UNAUTHORIZED, "invalid session"),
    };
    let taken = state
        .store
        .take_passkey_challenge(&input.challenge, "register")
        .await;
    let (owner, serialized) = match taken {
        Ok(Some((owner, serialized))) if owner == Some(user.id) => (owner, serialized),
        Ok(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "passkey challenge expired or invalid",
            );
        }
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let _ = owner;
    let registration: RegistrationState = match serde_json::from_str(&serialized) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let wa = match core(&headers) {
        Ok(wa) => wa,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let credential = match wa.register_credential(&input.credential, &registration, None) {
        Ok(credential) => credential,
        Err(err) => return error(StatusCode::BAD_REQUEST, err.to_string()),
    };
    if let Ok((_, _, existing)) = state.store.passkey_user(user.id).await
        && existing.iter().any(|id| id == &credential.cred_id)
    {
        return error(StatusCode::CONFLICT, "passkey already registered");
    }
    let serialized = match serde_json::to_string(&credential) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let name = if input.name.trim().is_empty() {
        "Passkey"
    } else {
        input.name.trim()
    };
    match state.store.add_passkey(user.id, name, &serialized).await {
        Ok(info) => Json(json!({
            "id":info.id,"name":info.name,"created_at":info.created_at,"last_used_at":info.last_used_at
        }))
        .into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

async fn list_passkeys(
    State(state): State<AppState>,
    Extension(session): Extension<AdminSession>,
) -> Response {
    let user = match state.store.user_by_name(&session.username).await {
        Ok(Some(user)) => user,
        _ => return error(StatusCode::UNAUTHORIZED, "invalid session"),
    };
    match state.store.passkeys(user.id).await {
        Ok(passkeys) => Json(json!({
            "passkeys": passkeys.iter().map(|info| json!({
                "id":info.id,"name":info.name,"created_at":info.created_at,"last_used_at":info.last_used_at
            })).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

async fn delete_passkey(
    State(state): State<AppState>,
    Extension(session): Extension<AdminSession>,
    Path(id): Path<i64>,
) -> Response {
    let user = match state.store.user_by_name(&session.username).await {
        Ok(Some(user)) => user,
        _ => return error(StatusCode::UNAUTHORIZED, "invalid session"),
    };
    match state.store.delete_passkey(user.id, id).await {
        Ok(()) => Json(json!({"deleted":true})).into_response(),
        Err(message) => error(StatusCode::NOT_FOUND, message),
    }
}

pub(crate) async fn login_options(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
) -> Response {
    // Public endpoint: cap challenge creation per client so it cannot be
    // used to flood the challenge table.
    {
        use std::sync::{LazyLock, Mutex};
        static HITS: LazyLock<
            Mutex<std::collections::HashMap<std::net::IpAddr, (u32, std::time::Instant)>>,
        > = LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
        let mut hits = HITS.lock().unwrap();
        let now = std::time::Instant::now();
        hits.retain(|_, (_, started)| {
            now.duration_since(*started) < std::time::Duration::from_secs(60)
        });
        let entry = hits.entry(peer.ip()).or_insert((0, now));
        if entry.0 >= 30 {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "too many passkey login attempts",
            );
        }
        entry.0 += 1;
    }
    let wa = match core(&headers) {
        Ok(wa) => wa,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let builder = match wa
        .new_challenge_authenticate_builder(Vec::new(), Some(UserVerificationPolicy::Required))
    {
        Ok(builder) => builder.allow_backup_eligible_upgrade(true),
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let (options, authentication) = match wa.generate_challenge_authenticate(builder) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let serialized = match serde_json::to_string(&authentication) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    match state
        .store
        .create_passkey_challenge("login", None, &serialized)
        .await
    {
        Ok(challenge) => Json(json!({"challenge":challenge,"options":options})).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

#[derive(Deserialize)]
pub(crate) struct LoginFinish {
    pub(crate) challenge: String,
    pub(crate) credential: PublicKeyCredential,
}

pub(crate) async fn login_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<LoginFinish>,
) -> Response {
    let serialized = match state
        .store
        .take_passkey_challenge(&input.challenge, "login")
        .await
    {
        Ok(Some((_, serialized))) => serialized,
        Ok(None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "passkey challenge expired or invalid",
            );
        }
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut authentication: AuthenticationState = match serde_json::from_str(&serialized) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    let handle = match input
        .credential
        .get_user_unique_id()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
    {
        Some(handle) => handle,
        None => return error(StatusCode::BAD_REQUEST, "passkey did not identify a user"),
    };
    let (user, stored) = match state.store.passkey_user_by_handle(handle).await {
        Ok(Some(value)) => value,
        Ok(None) => return error(StatusCode::UNAUTHORIZED, "unknown passkey"),
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut credentials: Vec<Credential> = Vec::new();
    for text in &stored {
        match serde_json::from_str::<Credential>(text) {
            Ok(credential) => credentials.push(credential),
            Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
        }
    }
    authentication.set_allowed_credentials(credentials.clone());
    let wa = match core(&headers) {
        Ok(wa) => wa,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let result = match wa.authenticate_credential(&input.credential, &authentication) {
        Ok(result) => result,
        Err(err) => return error(StatusCode::UNAUTHORIZED, err.to_string()),
    };
    if let Some(mut credential) = credentials
        .iter()
        .find(|credential| credential.cred_id == *result.cred_id())
        .cloned()
    {
        credential.counter = result.counter();
        credential.backup_state = result.backup_state();
        if let Ok(serialized) = serde_json::to_string(&credential)
            && let Err(message) = state
                .store
                .update_passkey(user.id, &credential.cred_id, &serialized)
                .await
        {
            eprintln!("failed to update passkey counter: {message}");
        }
    }
    let token = match state.store.login_token(&user.username).await {
        Ok(token) => token,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    Response::builder()
        .header(
            header::SET_COOKIE,
            format!(
                "sepia_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{}",
                if state.secure_cookie { "; Secure" } else { "" }
            ),
        )
        .body(
            Json(json!({"authenticated":true,"username":user.username,"role":user.role}))
                .into_response()
                .into_body(),
        )
        .unwrap()
}

/// Authenticated passkey management routes; merged into the admin router behind
/// the session middleware (which injects the `AdminSession` extension).
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/passkey/register/options", post(register_options))
        .route("/auth/passkey/register/finish", post(register_finish))
        .route("/passkeys", get(list_passkeys))
        .route("/passkeys/{id}", axum::routing::delete(delete_passkey))
}
