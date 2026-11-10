#[tokio::test]
async fn windows_absolute_database_path_is_supported() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested").join("windows.db");
    let store = sepia_gateway::Store::connect(&path).await.unwrap();
    assert!(!store.initialized().await.unwrap());
}
