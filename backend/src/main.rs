use sepia_gateway::{
    KeyStore, default_data_directory,
    server::{AppState, app},
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let directory = std::env::var_os("SEPIA_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_data_directory);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("data directory {}: {error}", directory.display()))?;
    let frontend_directory = if let Some(directory) = std::env::var_os("SEPIA_FRONTEND_DIR") {
        std::path::PathBuf::from(directory)
    } else {
        let working_directory =
            std::env::current_dir().map_err(|error| format!("working directory: {error}"))?;
        let local = working_directory.join("frontend/dist");
        if local.is_dir() {
            local
        } else {
            std::env::current_exe()
                .map_err(|error| format!("executable path: {error}"))?
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or("cannot locate packaged frontend directory")?
                .join("frontend/dist")
        }
    };
    std::env::set_current_dir(&directory)
        .map_err(|error| format!("data directory {}: {error}", directory.display()))?;
    let keys =
        if cfg!(debug_assertions) && std::env::var_os("SEPIA_DEV_INSECURE_KEYSTORE").is_some() {
            KeyStore::at(&directory.join("development-master-key"))
                .map_err(|error| format!("development keystore: {error}"))?
        } else {
            KeyStore::production_with_override(std::env::var("SEPIA_MASTER_KEY").ok().as_deref())?
        };
    let store = sepia_gateway::Store::connect(&directory.join("sepia.db"))
        .await
        .map_err(|error| format!("database {}: {error}", directory.display()))?;
    store.verify_keystore(&keys).await?;
    let address = std::env::var("SEPIA_LISTEN").unwrap_or_else(|_| "127.0.0.1:5000".into());
    let non_loopback = !address.starts_with("127.0.0.1:") && !address.starts_with("[::1]:");
    let trusted_proxy = std::env::var("SEPIA_TRUSTED_PROXY").ok().as_deref() == Some("true");
    let secure_cookie = non_loopback
        && !(address == "0.0.0.0:5000"
            && std::env::var("SEPIA_HTTP_LOOPBACK_COOKIE").ok().as_deref() == Some("true"));
    if non_loopback && !trusted_proxy {
        return Err("non-loopback listen requires SEPIA_TRUSTED_PROXY=true".into());
    }
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|e| e.to_string())?;
    let snapshots = std::sync::Arc::new(
        sepia_gateway::snapshot::Snapshots::load(&store)
            .await
            .map_err(|error| format!("configuration snapshot: {error}"))?,
    );
    let bootstrap_token = std::env::var("SEPIA_BOOTSTRAP_TOKEN")
        .unwrap_or_else(|_| sepia_gateway::crypto::random_token("sepia_bootstrap_"));
    if !store.initialized().await? {
        sepia_gateway::bootstrap::ensure_token(&store, &directory, &bootstrap_token).await?;
        println!("Bootstrap token: {bootstrap_token}");
    }
    println!("Sepia gateway listening on http://{address}");
    axum::serve(
        listener,
        app(AppState {
            store,
            keys,
            http: sepia_gateway::relay::protected_http_client()?,
            bootstrap_token,
            data_directory: directory,
            frontend_directory,
            snapshots,
            key_health: Default::default(),
            secure_cookie,
        })
        .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .map_err(|e| e.to_string())
}
