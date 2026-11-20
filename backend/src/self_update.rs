//! In-app self-update: check GitHub releases, download + SHA256-verify the
//! release archive for this platform, stage it under the data directory and
//! restart into the staged binary. Staging lives in `SEPIA_DATA_DIR` so an
//! update survives container restarts even when the image binary is read-only.

use crate::server::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_SOURCE: &str = "https://api.github.com/repos/lingyuins/Sepia/releases/latest";
const MAX_API_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SHA_BYTES: u64 = 4 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 1024 * 1024 * 1024;
/// Parent PID handed to the child spawned by a Windows in-app update restart.
/// The child waits for the parent to exit before binding the listen port, so
/// the handoff cannot lose a bind race against the process it replaces.
pub const HANDOFF_PARENT_PID_ENV: &str = "SEPIA_HANDOFF_PARENT_PID";
static APPLY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn current_version() -> semver::Version {
    semver::Version::parse(CURRENT_VERSION).expect("crate version is valid semver")
}
pub fn update_source() -> String {
    std::env::var("SEPIA_UPDATE_SOURCE").unwrap_or_else(|_| DEFAULT_SOURCE.into())
}
pub fn platform_artifact() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("macos", "x86_64") => Some("macos-x64"),
        ("macos", "aarch64") => Some("macos-arm64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        _ => None,
    }
}
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "sepia-gateway.exe"
    } else {
        "sepia-gateway"
    }
}
fn github_client() -> Result<reqwest::Client, String> {
    // GitHub release assets redirect to object storage, so this client must
    // follow redirects (the shared inference client deliberately does not).
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("Sepia/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone)]
pub struct Release {
    pub version: semver::Version,
    pub notes: String,
    pub published_at: Option<String>,
    pub url: String,
    pub assets: Vec<(String, String)>,
}
pub fn parse_release(body: &Value) -> Result<Release, String> {
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("release has no tag_name")?;
    let version = semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|error| format!("release tag {tag} is not semver: {error}"))?;
    let assets = body
        .get("assets")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("name")?.as_str()?.to_string(),
                        item.get("browser_download_url")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        version,
        notes: body
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        published_at: body
            .get("published_at")
            .and_then(Value::as_str)
            .map(String::from),
        url: body
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        assets,
    })
}
async fn capped_body(response: reqwest::Response, cap: u64) -> Result<Vec<u8>, String> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("update source unreadable: {error}"))?;
        if body.len() as u64 + chunk.len() as u64 > cap {
            return Err("update source response too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
pub async fn fetch_release(
    client: &reqwest::Client,
    source: &str,
    token: Option<&str>,
) -> Result<Release, String> {
    let mut request = client
        .get(source)
        .header("Accept", "application/vnd.github+json");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("update source unreachable: {error}"))?;
    let status = response.status();
    let bytes = capped_body(response, MAX_API_BYTES).await?;
    if !status.is_success() {
        let detail = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|body| {
                body.get("message")
                    .and_then(Value::as_str)
                    .map(|message| format!(": {message}"))
            })
            .unwrap_or_default();
        return Err(format!("update source HTTP {status}{detail}"));
    }
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("update source unreadable: {error}"))?;
    parse_release(&body)
}
pub fn parse_sha256_sidecar(text: &str) -> Result<String, String> {
    let hash = text
        .split_whitespace()
        .next()
        .ok_or("checksum file is empty")?
        .to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("checksum file does not contain a SHA256 hash".into());
    }
    Ok(hash)
}
fn release_asset_urls(release: &Release) -> Result<(String, String, String), String> {
    let artifact = platform_artifact().ok_or("self-update is not available on this platform")?;
    let archive_name = format!("sepia-{}-{artifact}.tar.gz", release.version);
    let checksum_name = format!("{archive_name}.sha256");
    let find = |name: &str| {
        release
            .assets
            .iter()
            .find(|(asset, _)| asset == name)
            .map(|(_, url)| url.clone())
            .ok_or_else(|| format!("release is missing asset {name}"))
    };
    Ok((
        find(&archive_name)?,
        find(&checksum_name)?,
        format!("sepia-{}-{artifact}", release.version),
    ))
}

pub fn updates_directory(data_directory: &Path) -> PathBuf {
    data_directory.join("updates")
}
fn version_directory(data_directory: &Path, version: &str) -> PathBuf {
    updates_directory(data_directory).join(version)
}
pub fn staged_binary_path(data_directory: &Path, version: &str) -> PathBuf {
    version_directory(data_directory, version)
        .join("bin")
        .join(binary_name())
}
fn marker_path(data_directory: &Path) -> PathBuf {
    updates_directory(data_directory).join("current.json")
}
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateMarker {
    pub version: String,
    pub staged_at: String,
}
pub fn read_marker(data_directory: &Path) -> Option<UpdateMarker> {
    let bytes = std::fs::read(marker_path(data_directory)).ok()?;
    serde_json::from_slice(&bytes).ok()
}
fn write_marker(data_directory: &Path, version: &str) -> Result<(), String> {
    let updates = updates_directory(data_directory);
    std::fs::create_dir_all(&updates).map_err(|error| format!("updates directory: {error}"))?;
    let marker = UpdateMarker {
        version: version.into(),
        staged_at: chrono::Utc::now().to_rfc3339(),
    };
    let temporary = updates.join(format!(".current-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(
        &temporary,
        serde_json::to_vec(&marker).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("update marker: {error}"))?;
    replace_file(&temporary, &marker_path(data_directory))
}

/// Publish `source` at `destination`, replacing any existing file. On Unix
/// `rename` already replaces atomically; on Windows `rename` fails when the
/// destination exists, so the swap goes through `ReplaceFileW`.
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let replacing = destination.exists();
        if replacing && windows_replace_file(source, destination).is_ok() {
            return Ok(());
        }
        if replacing {
            // ReplaceFileW also fails on filesystems that lack its support
            // (some network shares), so fall back to the working pair.
            std::fs::remove_file(destination).map_err(|error| format!("update marker: {error}"))?;
        }
    }
    std::fs::rename(source, destination).map_err(|error| format!("update marker: {error}"))
}

/// `kernel32!ReplaceFileW` replaces `destination` with `source` in one
/// operation, deleting the old file and keeping its attributes.
#[cfg(windows)]
fn windows_replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut core::ffi::c_void,
            reserved: *mut core::ffi::c_void,
        ) -> i32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
        units.push(0);
        units
    }

    let replaced = wide(destination);
    let replacement = wide(source);
    // SAFETY: both arguments are NUL-terminated UTF-16 buffers that outlive the
    // call, no backup file is requested and the reserved pointers are NULL.
    let replaced_ok = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if replaced_ok == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

/// Staged update newer than the running binary, if any. An older marker means
/// the installation (e.g. a rebuilt container image) moved past the staged
/// update, so the stale staging is removed instead.
pub fn handoff_target(data_directory: &Path) -> Option<(PathBuf, PathBuf)> {
    let marker = read_marker(data_directory)?;
    let staged = semver::Version::parse(&marker.version).ok()?;
    let current = current_version();
    if staged < current {
        let _ = std::fs::remove_file(marker_path(data_directory));
        let _ = std::fs::remove_dir_all(version_directory(data_directory, &staged.to_string()));
        return None;
    }
    if staged == current {
        return None;
    }
    let version = staged.to_string();
    let binary = staged_binary_path(data_directory, &version);
    if !binary.is_file() {
        return None;
    }
    let frontend = version_directory(data_directory, &version).join("frontend/dist");
    Some((binary, frontend))
}
/// Replace the current process with the staged binary. On Unix `exec` keeps
/// the PID (required for container PID 1); on Windows the child is spawned and
/// the parent exits, so the child waits for the parent to release the listen
/// port before it binds. Only returns on failure.
pub fn restart_process(binary: &Path, frontend_directory: &Path) -> Result<(), String> {
    let mut command = std::process::Command::new(binary);
    command
        .args(std::env::args_os().skip(1))
        .env("SEPIA_FRONTEND_DIR", frontend_directory);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        Err(format!("exec {}: {error}", binary.display()))
    }
    #[cfg(windows)]
    {
        // The child binds the same listen address, so it must wait for this
        // process to exit and release the port before it starts listening.
        command.env(HANDOFF_PARENT_PID_ENV, std::process::id().to_string());
        command
            .spawn()
            .map_err(|error| format!("spawn {}: {error}", binary.display()))?;
        std::process::exit(0);
    }
}
pub fn schedule_restart(data_directory: PathBuf) {
    tokio::spawn(async move {
        // Let the HTTP response flush before the process is replaced.
        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        if let Some((binary, frontend)) = handoff_target(&data_directory)
            && let Err(message) = restart_process(&binary, &frontend)
        {
            eprintln!("self-update restart failed: {message}");
        }
    });
}

fn safe_relative_path(path: &Path, package_prefix: &str) -> Result<PathBuf, String> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(top)) if top == package_prefix => {}
        _ => {
            return Err(format!(
                "archive entry outside package directory: {}",
                path.display()
            ));
        }
    }
    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => return Err(format!("unsafe archive path: {}", path.display())),
        }
    }
    Ok(relative)
}
fn extract_archive(
    archive_path: &Path,
    destination: &Path,
    package_prefix: &str,
) -> Result<(), String> {
    let file =
        std::fs::File::open(archive_path).map_err(|error| format!("update archive: {error}"))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);
    let mut total = 0u64;
    for entry in archive
        .entries()
        .map_err(|error| format!("update archive: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("update archive: {error}"))?;
        let kind = entry.header().entry_type();
        if matches!(
            kind,
            tar::EntryType::XGlobalHeader | tar::EntryType::XHeader
        ) {
            continue;
        }
        let path = entry
            .path()
            .map_err(|error| format!("update archive: {error}"))?
            .into_owned();
        let relative = safe_relative_path(&path, package_prefix)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(&relative);
        match kind {
            tar::EntryType::Directory => {
                std::fs::create_dir_all(&target)
                    .map_err(|error| format!("extract {}: {error}", target.display()))?;
            }
            tar::EntryType::Regular => {
                let size = entry.header().size().map_err(|error| error.to_string())?;
                if size > MAX_ENTRY_BYTES {
                    return Err(format!("archive entry too large: {}", path.display()));
                }
                total += size;
                if total > MAX_EXTRACTED_BYTES {
                    return Err("update archive expands beyond the size limit".into());
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("extract {}: {error}", parent.display()))?;
                }
                entry
                    .unpack(&target)
                    .map_err(|error| format!("extract {}: {error}", target.display()))?;
            }
            _ => return Err(format!("unsupported archive entry: {}", path.display())),
        }
    }
    Ok(())
}
async fn download_archive(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("update download unreachable: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("update download HTTP {status}"));
    }
    let mut file = tokio::fs::File::create(destination)
        .await
        .map_err(|error| format!("update download: {error}"))?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("update download: {error}"))?;
        total += chunk.len() as u64;
        if total > MAX_ARCHIVE_BYTES {
            return Err("update archive exceeds the size limit".into());
        }
        hasher.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|error| format!("update download: {error}"))?;
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|error| format!("update download: {error}"))?;
    Ok(hex::encode(hasher.finalize()))
}
/// Download, verify and extract the release for this platform into
/// `<data>/updates/<version>/`, then atomically publish the update marker.
pub async fn stage_release(
    client: &reqwest::Client,
    data_directory: &Path,
    release: &Release,
) -> Result<(), String> {
    let (archive_url, checksum_url, package_prefix) = release_asset_urls(release)?;
    let version = release.version.to_string();
    let updates = updates_directory(data_directory);
    tokio::fs::create_dir_all(&updates)
        .await
        .map_err(|error| format!("updates directory: {error}"))?;
    let checksum_response = client
        .get(&checksum_url)
        .send()
        .await
        .map_err(|error| format!("checksum download unreachable: {error}"))?;
    if !checksum_response.status().is_success() {
        return Err(format!(
            "checksum download HTTP {}",
            checksum_response.status()
        ));
    }
    let checksum_bytes = capped_body(checksum_response, MAX_SHA_BYTES).await?;
    let expected = parse_sha256_sidecar(&String::from_utf8_lossy(&checksum_bytes))?;
    let temporary = updates.join(format!(".download-{}.tar.gz", uuid::Uuid::new_v4()));
    let result = async {
        let digest = download_archive(client, &archive_url, &temporary).await?;
        if digest != expected {
            return Err(format!(
                "update archive SHA256 mismatch: expected {expected}, got {digest}"
            ));
        }
        let destination = version_directory(data_directory, &version);
        if destination.exists() {
            tokio::fs::remove_dir_all(&destination)
                .await
                .map_err(|error| format!("staging directory: {error}"))?;
        }
        tokio::fs::create_dir_all(&destination)
            .await
            .map_err(|error| format!("staging directory: {error}"))?;
        let archive_path = temporary.clone();
        let prefix = package_prefix.clone();
        let target = destination.clone();
        tokio::task::spawn_blocking(move || extract_archive(&archive_path, &target, &prefix))
            .await
            .map_err(|error| format!("update extraction: {error}"))??;
        let binary = staged_binary_path(data_directory, &version);
        if !binary.is_file() {
            return Err("update archive has no gateway binary".into());
        }
        if !destination.join("frontend/dist/index.html").is_file() {
            return Err("update archive has no frontend bundle".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
                .map_err(|error| format!("binary permissions: {error}"))?;
        }
        write_marker(data_directory, &version)
    }
    .await;
    let _ = tokio::fs::remove_file(&temporary).await;
    result
}

fn error(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    (status, Json(json!({"error":{"message":message.into()}}))).into_response()
}
pub async fn version() -> axum::response::Response {
    Json(json!({"version": CURRENT_VERSION})).into_response()
}
pub async fn check_update() -> axum::response::Response {
    let client = match github_client() {
        Ok(client) => client,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let token = std::env::var("SEPIA_GITHUB_TOKEN").ok();
    match fetch_release(&client, &update_source(), token.as_deref()).await {
        Ok(release) => Json(json!({
            "current_version": CURRENT_VERSION,
            "latest_version": release.version.to_string(),
            "update_available": release.version > current_version(),
            "release_notes": release.notes,
            "published_at": release.published_at,
            "release_url": release.url,
            "platform_supported": platform_artifact().is_some(),
        }))
        .into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, message),
    }
}
pub async fn apply_update(State(state): State<AppState>) -> axum::response::Response {
    let Ok(_guard) = APPLY_LOCK.try_lock() else {
        return error(StatusCode::CONFLICT, "an update is already in progress");
    };
    let client = match github_client() {
        Ok(client) => client,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let token = std::env::var("SEPIA_GITHUB_TOKEN").ok();
    let release = match fetch_release(&client, &update_source(), token.as_deref()).await {
        Ok(release) => release,
        Err(message) => return error(StatusCode::BAD_GATEWAY, message),
    };
    if release.version <= current_version() {
        return error(StatusCode::BAD_REQUEST, "no newer release is available");
    }
    if platform_artifact().is_none() {
        return error(
            StatusCode::BAD_REQUEST,
            "self-update is not available on this platform",
        );
    }
    if let Err(message) = stage_release(&client, &state.data_directory, &release).await {
        return error(StatusCode::BAD_GATEWAY, message);
    }
    let version = release.version.to_string();
    schedule_restart(state.data_directory.clone());
    Json(json!({"version": version, "restarting": true})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *contents).unwrap();
        }
        let raw = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &raw).unwrap();
        encoder.finish().unwrap()
    }

    // The tar writer refuses `..` paths, so craft the hostile header bytes
    // directly to prove the reader-side guard rejects a malicious archive.
    fn raw_path_archive_bytes(path: &str, contents: &[u8]) -> Vec<u8> {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        let name = header.as_old_mut().name.as_mut_slice();
        name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_cksum();
        let mut builder = tar::Builder::new(Vec::new());
        builder.append(&header, contents).unwrap();
        let raw = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &raw).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn safe_relative_path_strips_prefix_and_rejects_traversal() {
        let prefix = "sepia-1.2.3-linux-x64";
        assert_eq!(
            safe_relative_path(Path::new("sepia-1.2.3-linux-x64/bin/sepia-gateway"), prefix)
                .unwrap(),
            PathBuf::from("bin/sepia-gateway")
        );
        assert!(
            safe_relative_path(Path::new("sepia-1.2.3-linux-x64/../../etc/passwd"), prefix)
                .is_err()
        );
        assert!(safe_relative_path(Path::new("/etc/passwd"), prefix).is_err());
        assert!(safe_relative_path(Path::new("other-package/bin/sepia-gateway"), prefix).is_err());
    }

    #[test]
    fn write_marker_replaces_an_existing_marker() {
        let directory = tempfile::tempdir().unwrap();
        write_marker(directory.path(), "9.8.7").unwrap();
        assert_eq!(read_marker(directory.path()).unwrap().version, "9.8.7");
        // A second update rewrites current.json in place. `rename` cannot
        // replace an existing file on Windows, so this is the regression: the
        // second in-app update used to fail while the first one succeeded.
        write_marker(directory.path(), "9.8.8").unwrap();
        assert_eq!(read_marker(directory.path()).unwrap().version, "9.8.8");
        let leftovers = std::fs::read_dir(updates_directory(directory.path()))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".current-"))
            .count();
        assert_eq!(leftovers, 0, "temporary marker files must not survive");
    }

    #[test]
    fn parse_sha256_sidecar_accepts_sum_format_and_rejects_garbage() {
        let hash = "a".repeat(64);
        assert_eq!(
            parse_sha256_sidecar(&format!(
                "{}  sepia-1.2.3-linux-x64.tar.gz\n",
                hash.to_uppercase()
            ))
            .unwrap(),
            hash
        );
        assert!(parse_sha256_sidecar("").is_err());
        assert!(parse_sha256_sidecar("not-a-hash").is_err());
        assert!(parse_sha256_sidecar(&"a".repeat(63)).is_err());
    }

    #[test]
    fn parse_release_reads_tag_notes_and_assets() {
        let body = json!({
            "tag_name": "v1.2.3",
            "body": "release notes",
            "published_at": "2026-11-19T00:00:00Z",
            "html_url": "https://example.test/release",
            "assets": [
                {"name": "sepia-1.2.3-linux-x64.tar.gz", "browser_download_url": "https://example.test/archive"},
                {"name": "sepia-1.2.3-linux-x64.tar.gz.sha256", "browser_download_url": "https://example.test/checksum"},
                {"name": "incomplete"}
            ]
        });
        let release = parse_release(&body).unwrap();
        assert_eq!(release.version.to_string(), "1.2.3");
        assert_eq!(release.notes, "release notes");
        assert_eq!(release.assets.len(), 2);
        assert!(parse_release(&json!({"tag_name": "not-semver"})).is_err());
        assert!(parse_release(&json!({})).is_err());
    }

    #[test]
    fn extract_archive_rejects_entries_outside_the_package() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("escape.tar.gz");
        std::fs::write(&archive, raw_path_archive_bytes("../outside.txt", b"pwned")).unwrap();
        let destination = directory.path().join("staged");
        let error = extract_archive(&archive, &destination, "sepia-1.2.3-linux-x64").unwrap_err();
        assert!(error.contains("outside package directory"), "{error}");
        assert!(!directory.path().join("outside.txt").exists());
    }

    #[test]
    fn extract_archive_unpacks_regular_entries() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("package.tar.gz");
        std::fs::write(
            &archive,
            archive_bytes(&[
                ("sepia-1.2.3-linux-x64/bin/sepia-gateway", b"binary"),
                ("sepia-1.2.3-linux-x64/frontend/dist/index.html", b"<html>"),
            ]),
        )
        .unwrap();
        let destination = directory.path().join("staged");
        extract_archive(&archive, &destination, "sepia-1.2.3-linux-x64").unwrap();
        assert_eq!(
            std::fs::read(destination.join("bin/sepia-gateway")).unwrap(),
            b"binary"
        );
        assert_eq!(
            std::fs::read(destination.join("frontend/dist/index.html")).unwrap(),
            b"<html>"
        );
    }
}
