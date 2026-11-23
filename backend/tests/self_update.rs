use axum::{Json, Router, extract::State, response::IntoResponse, routing::get};
use sepia_gateway::{
    KeyStore, Store, self_update,
    self_update::{
        CURRENT_VERSION, current_version, platform_artifact, read_marker, staged_binary_path,
        updates_directory,
    },
    server::{AppState, app},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use tokio::net::TcpListener;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn serve(router: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );
    address
}

fn newer_version() -> String {
    let mut version = current_version();
    version.minor += 1;
    version.to_string()
}

fn package_prefix(version: &str) -> String {
    format!("sepia-{version}-{}", platform_artifact().unwrap())
}

fn archive_bytes(version: &str, binary: &[u8], frontend: &[u8]) -> Vec<u8> {
    let prefix = package_prefix(version);
    let mut builder = tar::Builder::new(Vec::new());
    for (path, contents) in [
        (
            format!("{prefix}/bin/{}", self_update::binary_name()),
            binary,
        ),
        (format!("{prefix}/frontend/dist/index.html"), frontend),
        (format!("{prefix}/LICENSE"), b"license".as_slice()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, contents).unwrap();
    }
    let raw = builder.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &raw).unwrap();
    encoder.finish().unwrap()
}

#[derive(Clone)]
struct MockRelease {
    version: String,
    archive: Vec<u8>,
    checksum: String,
    notes: String,
}

impl MockRelease {
    fn new(version: &str, binary: &[u8], frontend: &[u8], checksum_override: Option<&str>) -> Self {
        let archive = archive_bytes(version, binary, frontend);
        let checksum = checksum_override
            .map(str::to_string)
            .unwrap_or_else(|| hex::encode(Sha256::digest(&archive)));
        Self {
            version: version.into(),
            archive,
            checksum,
            notes: "self-update test release".into(),
        }
    }

    fn body(&self, base: &str) -> Value {
        let prefix = package_prefix(&self.version);
        let archive_name = format!("{prefix}.tar.gz");
        json!({
            "tag_name": format!("v{}", self.version),
            "body": self.notes,
            "published_at": "2026-11-19T00:00:00Z",
            "html_url": format!("{base}/release"),
            "assets": [
                {"name": archive_name, "browser_download_url": format!("{base}/download/{archive_name}")},
                {"name": format!("{archive_name}.sha256"), "browser_download_url": format!("{base}/download/{archive_name}.sha256")},
            ]
        })
    }
}

async fn mock_release_server(release: MockRelease) -> SocketAddr {
    let handler = get(
        move |State(release): State<MockRelease>, request: axum::extract::Request| {
            let release = release.clone();
            async move {
                let path = request.uri().path().to_string();
                let prefix = package_prefix(&release.version);
                if path == "/latest" {
                    let base = format!(
                        "http://{}",
                        request
                            .headers()
                            .get("host")
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("127.0.0.1")
                    );
                    Json(release.body(&base)).into_response()
                } else if path == format!("/download/{prefix}.tar.gz") {
                    (
                        [(axum::http::header::CONTENT_TYPE, "application/gzip")],
                        release.archive.clone(),
                    )
                        .into_response()
                } else if path == format!("/download/{prefix}.tar.gz.sha256") {
                    format!("{}  {prefix}.tar.gz\n", release.checksum).into_response()
                } else {
                    axum::http::StatusCode::NOT_FOUND.into_response()
                }
            }
        },
    );
    serve(Router::new().fallback(handler).with_state(release)).await
}

// Keep the TempDir alive in the caller: dropping it deletes the SQLite files
// while pooled connections still reference them (disk I/O errors on Linux).
async fn gateway(directory: &std::path::Path) -> (SocketAddr, String) {
    let store = Store::connect(&directory.join("self-update.db"))
        .await
        .unwrap();
    let state = AppState {
        store: store.clone(),
        keys: KeyStore::testing(),
        http: std::sync::Arc::new(std::sync::Mutex::new((10, reqwest::Client::new()))),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.to_path_buf(),
        frontend_directory: "frontend/dist".into(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        routing_stats: sepia_gateway::strategy::RoutingStats::shared(),
        secure_cookie: false,
        trusted_proxy: false,
        metadata_source: sepia_gateway::metadata::DEFAULT_SOURCE.into(),
        plugins: sepia_gateway::plugin::shared(
            sepia_gateway::plugin::PluginRuntime::new().unwrap(),
        ),
    };
    let address = serve(app(state)).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    (address, cookie)
}

fn stage_fixture(directory: &std::path::Path, version: &str) {
    let binary = staged_binary_path(directory, version);
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"staged binary").unwrap();
    let frontend = updates_directory(directory)
        .join(version)
        .join("frontend/dist");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), b"<html>").unwrap();
    let marker = json!({"version": version, "staged_at": "2026-11-19T00:00:00Z"});
    std::fs::create_dir_all(updates_directory(directory)).unwrap();
    std::fs::write(
        updates_directory(directory).join("current.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn version_is_public_and_update_requires_a_session() {
    let directory = tempfile::tempdir().unwrap();
    let (address, cookie) = gateway(directory.path()).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    let version: Value = client
        .get(url("/api/v1/version"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(version["version"], CURRENT_VERSION);
    let denied = client.post(url("/api/v1/update")).send().await.unwrap();
    assert_eq!(denied.status(), 401);
    let denied_check = client
        .get(url("/api/v1/update/check"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_check.status(), 401);
    let source_guard = ENV_LOCK.lock().await;
    // SAFETY: single-threaded env access while the lock is held.
    unsafe { std::env::set_var("SEPIA_UPDATE_SOURCE", "http://127.0.0.1:1/latest") };
    let unreachable = client
        .get(url("/api/v1/update/check"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    unsafe { std::env::remove_var("SEPIA_UPDATE_SOURCE") };
    drop(source_guard);
    assert_eq!(unreachable.status(), 502);
    let body: Value = unreachable.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("update source unreachable")
    );
}

#[tokio::test]
async fn check_reports_a_newer_release_from_the_configured_source() {
    let version = newer_version();
    let release = MockRelease::new(&version, b"binary", b"<html>", None);
    let source_address = mock_release_server(release).await;
    let directory = tempfile::tempdir().unwrap();
    let (address, cookie) = gateway(directory.path()).await;
    let source_guard = ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var(
            "SEPIA_UPDATE_SOURCE",
            format!("http://{source_address}/latest"),
        )
    };
    let response = reqwest::Client::new()
        .get(format!("http://{address}/api/v1/update/check"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    unsafe { std::env::remove_var("SEPIA_UPDATE_SOURCE") };
    drop(source_guard);
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["current_version"], CURRENT_VERSION);
    assert_eq!(body["latest_version"], version);
    assert_eq!(body["update_available"], true);
    assert_eq!(body["platform_supported"], true);
    assert_eq!(body["release_notes"], "self-update test release");
    assert_eq!(body["published_at"], "2026-11-19T00:00:00Z");
}

#[tokio::test]
async fn stage_release_verifies_checksum_and_stages_binary_and_frontend() {
    let version = newer_version();
    let release = MockRelease::new(&version, b"new binary", b"<html>new</html>", None);
    let source_address = mock_release_server(release.clone()).await;
    let base = format!("http://{source_address}");
    let parsed = self_update::parse_release(&release.body(&base)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    self_update::stage_release(&client, directory.path(), &parsed)
        .await
        .unwrap();
    let binary = staged_binary_path(directory.path(), &version);
    assert_eq!(std::fs::read(&binary).unwrap(), b"new binary");
    let frontend = updates_directory(directory.path())
        .join(&version)
        .join("frontend/dist/index.html");
    assert_eq!(std::fs::read(frontend).unwrap(), b"<html>new</html>");
    let marker = read_marker(directory.path()).unwrap();
    assert_eq!(marker.version, version);
    assert!(
        !updates_directory(directory.path())
            .join(".download-")
            .exists()
    );
}

#[tokio::test]
async fn staging_a_second_release_replaces_the_marker() {
    // The second in-app update used to fail on Windows because publishing the
    // marker with `rename` cannot replace the existing current.json there.
    let client = reqwest::Client::new();
    let directory = tempfile::tempdir().unwrap();
    for offset in [1u64, 2] {
        let mut version = current_version();
        version.minor += offset;
        let release = MockRelease::new(&version.to_string(), b"binary", b"<html>", None);
        let source_address = mock_release_server(release.clone()).await;
        let base = format!("http://{source_address}");
        let parsed = self_update::parse_release(&release.body(&base)).unwrap();
        self_update::stage_release(&client, directory.path(), &parsed)
            .await
            .unwrap();
        let marker = read_marker(directory.path()).unwrap();
        assert_eq!(marker.version, version.to_string());
    }
    let leftovers = std::fs::read_dir(updates_directory(directory.path()))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".current-"))
        .count();
    assert_eq!(leftovers, 0, "temporary marker files must not survive");
}

#[tokio::test]
async fn stage_release_rejects_a_checksum_mismatch_without_staging() {
    let version = newer_version();
    let release = MockRelease::new(&version, b"binary", b"<html>", Some(&"0".repeat(64)));
    let source_address = mock_release_server(release.clone()).await;
    let base = format!("http://{source_address}");
    let parsed = self_update::parse_release(&release.body(&base)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let error = self_update::stage_release(&client, directory.path(), &parsed)
        .await
        .unwrap_err();
    assert!(error.contains("SHA256 mismatch"), "{error}");
    assert!(read_marker(directory.path()).is_none());
    assert!(!staged_binary_path(directory.path(), &version).exists());
    assert!(
        !updates_directory(directory.path())
            .join(".download-")
            .exists()
    );
}

#[tokio::test]
async fn handoff_selects_newer_staging_and_cleans_stale_versions() {
    let directory = tempfile::tempdir().unwrap();
    assert!(self_update::handoff_target(directory.path()).is_none());

    let newer = newer_version();
    stage_fixture(directory.path(), &newer);
    let (binary, frontend) = self_update::handoff_target(directory.path()).unwrap();
    assert_eq!(binary, staged_binary_path(directory.path(), &newer));
    assert!(frontend.ends_with("frontend/dist"));
    assert!(binary.is_file());

    stage_fixture(directory.path(), CURRENT_VERSION);
    assert!(self_update::handoff_target(directory.path()).is_none());
    assert!(read_marker(directory.path()).is_some());

    let mut older = current_version();
    older.minor = older.minor.saturating_sub(1);
    let older = older.to_string();
    stage_fixture(directory.path(), &older);
    assert!(self_update::handoff_target(directory.path()).is_none());
    assert!(read_marker(directory.path()).is_none());
    assert!(!updates_directory(directory.path()).join(&older).exists());
}

#[tokio::test]
async fn apply_update_stages_the_release_and_reports_the_restart() {
    let version = newer_version();
    let release = MockRelease::new(&version, b"new binary", b"<html>new</html>", None);
    let source_address = mock_release_server(release).await;
    let directory = tempfile::tempdir().unwrap();
    let (address, cookie) = gateway(directory.path()).await;
    let source_guard = ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var(
            "SEPIA_UPDATE_SOURCE",
            format!("http://{source_address}/latest"),
        )
    };
    let response = reqwest::Client::new()
        .post(format!("http://{address}/api/v1/update"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    unsafe { std::env::remove_var("SEPIA_UPDATE_SOURCE") };
    drop(source_guard);
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["version"], version);
    assert_eq!(body["restarting"], true);
    assert_eq!(
        std::fs::read(staged_binary_path(directory.path(), &version)).unwrap(),
        b"new binary"
    );
    assert_eq!(read_marker(directory.path()).unwrap().version, version);
    // schedule_restart fires after 750ms; the staged fixture bytes are not a
    // real executable, so spawn/exec fails harmlessly and the test process is
    // never replaced. Real end-to-end restart is exercised separately.
}
