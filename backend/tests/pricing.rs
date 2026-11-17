use sepia_gateway::{NewProvider, ProviderProtocol, Store, UpsertClientKey};

#[tokio::test]
async fn client_keys_reject_invalid_or_elapsed_expirations_when_created() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("expiry.db"))
        .await
        .unwrap();
    for expires_at in [
        Some("not-a-date".to_string()),
        Some("2000-01-01T00:00:00.000Z".to_string()),
    ] {
        assert!(
            store
                .create_client_key(UpsertClientKey {
                    name: "expired".into(),
                    models: vec!["*".into()],
                    token_limit: 0,
                    max_amount_micros: 0,
                    rpm: 0,
                    tpm: 0,
                    expires_at,
                })
                .await
                .is_err()
        );
    }
    assert!(store.client_keys().await.unwrap().is_empty());
}

#[tokio::test]
async fn amount_budget_reserves_and_charges_actual_model_usage() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("money.db"))
        .await
        .unwrap();
    let provider = store
        .create_provider(
            NewProvider {
                name: "priced".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://localhost/v1".into(),
                models: vec!["priced-model".into()],
            },
            "secret",
            &[],
            &sepia_gateway::KeyStore::testing(),
        )
        .await
        .unwrap();
    store
        .set_model_price(provider.id, "priced-model", 1_000_000, 2_000_000)
        .await
        .unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "budget".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 5,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap();
    let reservation = store
        .reserve_quota(key.id, "priced-model", 2)
        .await
        .unwrap();
    assert!(
        store
            .reserve_quota(key.id, "priced-model", 2)
            .await
            .is_err()
    );
    store.settle_quota_amount(&reservation, 1, 3).await.unwrap();
    assert_eq!(store.amount_used(key.id).await.unwrap(), 3);
    let unpriced = store
        .create_client_key(UpsertClientKey {
            name: "unpriced".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 1,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap();
    assert!(
        store
            .reserve_quota(unpriced.id, "missing-model", 1)
            .await
            .is_err()
    );
}
