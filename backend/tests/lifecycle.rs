use sepia_gateway::{KeyStore, Store};

#[tokio::test]
async fn dropping_request_guard_releases_quota_and_marks_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("drop.db"))
        .await
        .unwrap();
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
            name: "limited".into(),
            models: vec!["*".into()],
            token_limit: 10,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 10,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    let reservation = store.reserve_quota(key.id, "model", 10).await.unwrap();
    let request = store
        .begin_request(key.id, sepia_gateway::ProviderProtocol::OpenAI, "model")
        .await
        .unwrap();
    {
        let _guard = sepia_gateway::lifecycle::RequestGuard::new(
            store.clone(),
            request.clone(),
            reservation,
        );
    }
    for _ in 0..30 {
        if store.requests().await.unwrap()[0].status == "cancelled" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(store.requests().await.unwrap()[0].status, "cancelled");
    assert!(store.reserve_quota(key.id, "model", 1).await.is_ok());
    let _ = KeyStore::testing();
}
