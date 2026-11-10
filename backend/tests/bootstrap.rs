use sepia_gateway::{KeyStore, Store};

#[tokio::test]
async fn bootstrap_secret_file_is_removed_after_successful_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("app.db"))
        .await
        .unwrap();
    let path = sepia_gateway::bootstrap::ensure_token(&store, directory.path(), "test-token")
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "test-token");
    store
        .bootstrap("admin", "correct horse", "test-token", "test-token")
        .await
        .unwrap();
    sepia_gateway::bootstrap::remove_token(directory.path())
        .await
        .unwrap();
    assert!(!path.exists());
    let _ = KeyStore::testing();
}
