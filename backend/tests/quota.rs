use sepia_gateway::{KeyStore, Store, UpsertClientKey};

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
