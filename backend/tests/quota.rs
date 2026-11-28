use sepia_gateway::{KeyStore, ProviderProtocol, Store, UpsertClientKey};

#[tokio::test]
async fn quota_reservation_is_atomic_and_settles_usage() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("quota.db"))
        .await
        .unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "limited".into(),
            models: vec!["allowed-*".into()],
            token_limit: 10,
            max_amount_micros: 0,
            rpm: 1,
            tpm: 10,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    assert!(
        store
            .authorize_model(key.id, "allowed-model")
            .await
            .unwrap()
    );
    assert!(
        !store
            .authorize_model(key.id, "forbidden-model")
            .await
            .unwrap()
    );
    let reservation = store
        .reserve_quota(key.id, "allowed-model", 10)
        .await
        .unwrap();
    assert!(
        store
            .reserve_quota(key.id, "allowed-model", 1)
            .await
            .is_err()
    );
    store.settle_quota(&reservation, 8).await.unwrap();
    assert!(
        store
            .reserve_quota(key.id, "allowed-model", 2)
            .await
            .is_err()
    );
    let token_key = store
        .create_client_key(UpsertClientKey {
            name: "tokens".into(),
            models: vec!["*".into()],
            token_limit: 10,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    let reservation = store
        .reserve_quota(token_key.id, "allowed-model", 10)
        .await
        .unwrap();
    store.settle_quota(&reservation, 8).await.unwrap();
    assert!(
        store
            .reserve_quota(token_key.id, "allowed-model", 2)
            .await
            .is_ok()
    );
    let key_store = KeyStore::testing();
    assert!(
        key_store
            .decrypt(
                &key_store.encrypt("secret").unwrap().0,
                &key_store.encrypt("secret").unwrap().1
            )
            .is_err()
    );
}

#[tokio::test]
async fn missing_reservation_reports_false_instead_of_erroring() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("missing-reservation.db"))
        .await
        .unwrap();
    assert!(!store.settle_quota("gone", 0).await.unwrap());
    assert!(!store.settle_quota_amount("gone", 5, 7).await.unwrap());
}

#[tokio::test]
async fn guard_settles_real_outcome_after_key_delete_but_fails_lost_reservations() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("guard-delete.db"))
        .await
        .unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "doomed".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    let events = sepia_gateway::events::Events::new();
    // Deleting the key clears its open reservation and detaches the request.
    let request_id = store
        .begin_request(key.id, ProviderProtocol::OpenAI, "gpt-x")
        .await
        .unwrap();
    let reservation = store.reserve_quota(key.id, "gpt-x", 100).await.unwrap();
    assert!(store.delete_client_key(key.id).await.unwrap());
    let guard = sepia_gateway::lifecycle::RequestGuard::new(
        store.clone(),
        request_id.clone(),
        reservation,
        events.clone(),
    );
    // The upstream already succeeded: keep the real outcome, tokens and cost.
    guard.complete("success", 100, 20, 2_800).await.unwrap();
    let record = store.requests().await.unwrap().remove(0);
    assert_eq!(record.status, "success");
    assert_eq!(record.input_tokens, 100);
    assert_eq!(record.output_tokens, 20);
    assert_eq!(record.cost_micros, 2_800);
    assert!(record.key.is_none());
    // A key that still exists must never silently skip quota settlement.
    let live_key = store
        .create_client_key(UpsertClientKey {
            name: "live".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    let request_id = store
        .begin_request(live_key.id, ProviderProtocol::OpenAI, "gpt-x")
        .await
        .unwrap();
    let guard = sepia_gateway::lifecycle::RequestGuard::new(
        store.clone(),
        request_id,
        "lost-reservation".into(),
        events,
    );
    assert_eq!(
        guard.complete("success", 100, 20, 2_800).await.unwrap_err(),
        "reservation not found"
    );
    assert_eq!(store.requests().await.unwrap().remove(0).status, "pending");
}
