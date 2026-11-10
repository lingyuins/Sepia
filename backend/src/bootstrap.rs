use crate::Store;
use std::path::{Path, PathBuf};

pub fn token_path(directory: &Path) -> PathBuf {
    directory.join("bootstrap-token")
}
pub async fn ensure_token(store: &Store, directory: &Path, token: &str) -> Result<PathBuf, String> {
    let path = token_path(directory);
    if store.initialized().await? {
        return Ok(path);
    }
    tokio::fs::write(&path, token)
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(path)
}
pub async fn remove_token(directory: &Path) -> Result<(), String> {
    match tokio::fs::remove_file(token_path(directory)).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
