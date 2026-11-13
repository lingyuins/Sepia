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
    let keys =
        if cfg!(debug_assertions) && std::env::var_os("SEPIA_DEV_INSECURE_KEYSTORE").is_some() {
            KeyStore::at(&directory.join("development-master-key"))
                .map_err(|error| format!("development keystore: {error}"))?
        } else {
            KeyStore::production()?
        };
    let store = sepia_gateway::Store::connect(&directory.join("sepia.db"))
        .await
        .map_err(|error| format!("database {}: {error}", directory.display()))?;
    let address = std::env::var("SEPIA_LISTEN").unwrap_or_else(|_| "127.0.0.1:5000".into());
    let non_loopback = !address.starts_with("127.0.0.1:") && !address.starts_with("[::1]:");
    let trusted_proxy = std::env::var("SEPIA_TRUSTED_PROXY").ok().as_deref() == Some("true");
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
            snapshots,
            key_health: Default::default(),
            secure_cookie: non_loopback,
        })
        .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .map_err(|e| e.to_string())
}
