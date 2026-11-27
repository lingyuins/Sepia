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
                    secret: None,
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
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "secret",
            &[],
            &sepia_gateway::KeyStore::testing(),
        )
        .await
        .unwrap();
    store
        .set_model_price(provider.id, "priced-model", 1_000_000, 2_000_000, 0)
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
            secret: None,
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
            secret: None,
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

#[tokio::test]
async fn model_cost_bills_cached_tokens_at_the_cache_read_price() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("cache-cost.db"))
        .await
        .unwrap();
    let provider = store
        .create_provider(
            NewProvider {
                name: "cached".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://localhost/v1".into(),
                models: vec!["cached-model".into(), "plain-model".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "secret",
            &[],
            &sepia_gateway::KeyStore::testing(),
        )
        .await
        .unwrap();
    // $1/M input, $2/M output, $0.10/M cache read.
    store
        .set_model_price(provider.id, "cached-model", 1_000_000, 2_000_000, 100_000)
        .await
        .unwrap();
    store
        .set_model_price(provider.id, "plain-model", 1_000_000, 2_000_000, 0)
        .await
        .unwrap();
    // Issue #6 acceptance: 10,000 input incl. 8,000 cached, 0 output →
    // 2,000×$1/M + 8,000×$0.10/M = $0.0028 = 2,800 micros.
    assert_eq!(
        store
            .model_cost("cached-model", 10_000, 0, 8_000, true)
            .await
            .unwrap(),
        2_800
    );
    // Output bills normally on top: +500×$2/M = 1,000 micros.
    assert_eq!(
        store
            .model_cost("cached-model", 10_000, 500, 8_000, true)
            .await
            .unwrap(),
        3_800
    );
    // Anthropic-style split usage: input excludes cache reads, so the
    // 2,000 uncached tokens bill at the normal price without subtraction.
    assert_eq!(
        store
            .model_cost("cached-model", 2_000, 0, 8_000, false)
            .await
            .unwrap(),
        2_800
    );
    // No cache price configured → cached tokens fall back to input price.
    assert_eq!(
        store
            .model_cost("plain-model", 10_000, 0, 8_000, true)
            .await
            .unwrap(),
        10_000
    );
    // No cached tokens → unchanged normal billing.
    assert_eq!(
        store
            .model_cost("cached-model", 10_000, 500, 0, true)
            .await
            .unwrap(),
        11_000
    );
    // Cached count above total input clamps instead of going negative.
    assert_eq!(
        store
            .model_cost("cached-model", 1_000, 0, 5_000, true)
            .await
            .unwrap(),
        500
    );
    // Negative cache prices are rejected at save time.
    assert!(
        store
            .set_model_price(provider.id, "cached-model", 1_000_000, 2_000_000, -1)
            .await
            .is_err()
    );
}
