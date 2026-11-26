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
    // An in-app update restart on Windows spawns this process while the parent
    // still holds the listen port, so wait for the parent to exit before
    // touching the database or the port.
    await_handoff_parent();
    // A staged in-app update lives in the data directory, so it also takes
    // effect after container restarts where the image binary is read-only.
    if let Some((binary, frontend)) = sepia_gateway::self_update::handoff_target(&directory) {
        println!("Handing off to staged update {}", binary.display());
        return sepia_gateway::self_update::restart_process(&binary, &frontend);
    }
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
            KeyStore::production_with_override(
                std::env::var("SEPIA_MASTER_KEY").ok().as_deref(),
                &directory,
            )?
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
    let mut plugin_runtime = sepia_gateway::plugin::PluginRuntime::new()?;
    plugin_runtime.reload(&store).await?;
    let _epoch_ticker = plugin_runtime.spawn_epoch_ticker();
    let plugins = sepia_gateway::plugin::shared(plugin_runtime);
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
            http: std::sync::Arc::new(std::sync::Mutex::new((
                snapshots.get().connect_timeout_seconds,
                sepia_gateway::relay::protected_http_client(
                    snapshots.get().connect_timeout_seconds,
                )?,
            ))),
            bootstrap_token,
            data_directory: directory,
            frontend_directory,
            snapshots,
            key_health: Default::default(),
            secure_cookie,
            trusted_proxy,
            metadata_source: std::env::var("SEPIA_METADATA_SOURCE")
                .unwrap_or_else(|_| sepia_gateway::metadata::DEFAULT_SOURCE.into()),
            litellm_source: std::env::var("SEPIA_LITELLM_SOURCE")
                .unwrap_or_else(|_| sepia_gateway::metadata::DEFAULT_LITELLM_SOURCE.into()),
            openrouter_source: std::env::var("SEPIA_OPENROUTER_SOURCE")
                .unwrap_or_else(|_| sepia_gateway::metadata::DEFAULT_OPENROUTER_SOURCE.into()),
            plugins,
            routing_stats: sepia_gateway::strategy::RoutingStats::shared(),
            events: sepia_gateway::events::Events::new(),
        })
        .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Waits for the process that spawned this one during a Windows in-app update
/// restart, then clears the handoff variable so it cannot reach a later
/// generation. On Unix the restart uses `exec`, which keeps the PID, so there
/// is nothing to wait for.
fn await_handoff_parent() {
    let variable = sepia_gateway::self_update::HANDOFF_PARENT_PID_ENV;
    let Some(parent) = std::env::var(variable)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
    else {
        return;
    };
    // SAFETY: runs at the very start of `main`, before any application thread
    // reads the environment; the variable is only an internal handoff signal.
    unsafe { std::env::remove_var(variable) };
    wait_for_process_exit(parent, std::time::Duration::from_secs(15));
}

#[cfg(windows)]
fn wait_for_process_exit(pid: u32, limit: std::time::Duration) {
    if pid == std::process::id() {
        return;
    }
    let deadline = std::time::Instant::now() + limit;
    while process_running(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(not(windows))]
fn wait_for_process_exit(_pid: u32, _limit: std::time::Duration) {}

#[cfg(windows)]
fn process_running(pid: u32) -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 0x102;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(
            desired_access: u32,
            inherit_handle: i32,
            process_id: u32,
        ) -> *mut core::ffi::c_void;
        fn WaitForSingleObject(handle: *mut core::ffi::c_void, milliseconds: u32) -> u32;
        fn CloseHandle(object: *mut core::ffi::c_void) -> i32;
    }

    // SAFETY: `pid` is the PID of the process that spawned us; the handle is
    // closed before returning and never reused.
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        // Gone, or not ours to open: either way it cannot hold the port.
        return false;
    }
    let status = unsafe { WaitForSingleObject(handle, 0) };
    // SAFETY: `handle` came from OpenProcess and is not used afterwards.
    unsafe { CloseHandle(handle) };
    match status {
        WAIT_OBJECT_0 => false,
        WAIT_TIMEOUT => true,
        // WAIT_FAILED and anything unexpected must not hang startup.
        _ => false,
    }
}
