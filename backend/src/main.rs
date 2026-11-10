use sepia_gateway::{
    KeyStore, default_data_directory,
    server::{AppState, app},
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let directory = std::env::var_os("SEPIA_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_data_directory);
    let keys =
        if cfg!(debug_assertions) && std::env::var_os("SEPIA_DEV_INSECURE_KEYSTORE").is_some() {
            KeyStore::at(&directory.join("development-master-key"))?
        } else {
            KeyStore::production()?
        };
    let store = sepia_gateway::Store::connect(&directory.join("sepia.db")).await?;
    let address = std::env::var("SEPIA_LISTEN").unwrap_or_else(|_| "127.0.0.1:3425".into());
    if !address.starts_with("127.0.0.1:")
        && !address.starts_with("[::1]:")
        && std::env::var_os("SEPIA_TRUSTED_PROXY").is_none()
    {
        return Err("non-loopback listen requires SEPIA_TRUSTED_PROXY=true".into());
    }
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|e| e.to_string())?;
    let bootstrap_token = std::env::var("SEPIA_BOOTSTRAP_TOKEN")
        .unwrap_or_else(|_| sepia_gateway::crypto::random_token("sepia_bootstrap_"));
    if !store.initialized().await? {
        tokio::fs::write(directory.join("bootstrap-token"), &bootstrap_token)
            .await
            .map_err(|e| e.to_string())?;
        println!("Bootstrap token: {bootstrap_token}");
    }
    println!("Sepia gateway listening on http://{address}");
    axum::serve(
        listener,
        app(AppState {
            store,
            keys,
            http: reqwest::Client::new(),
            bootstrap_token,
        }),
    )
    .await
    .map_err(|e| e.to_string())
}
