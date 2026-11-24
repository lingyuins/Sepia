use sepia_gateway::{KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey};

#[tokio::test]
async fn bootstrap_authenticates_sessions_and_persists_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sepia.db");
    let key_store = KeyStore::testing();
    let store = Store::connect(&database).await.unwrap();
    assert!(!store.initialized().await.unwrap());
    store
        .bootstrap(
            "admin",
            "correct horse",
            "bootstrap-token",
            "bootstrap-token",
        )
        .await
        .unwrap();
    assert!(store.initialized().await.unwrap());
    store
        .bootstrap("other", "password", "bootstrap-token", "bootstrap-token")
        .await
        .unwrap();
    assert!(store.login("other", "password").await.is_err());
    assert!(store.login("admin", "wrong").await.is_err());
    let (session, _) = store.login("admin", "correct horse").await.unwrap();
    assert_eq!(
        store.session(&session).await.unwrap().unwrap().username,
        "admin"
    );

    let provider = NewProvider {
        name: "Example".into(),
        protocol: ProviderProtocol::OpenAI,
        endpoint: "https://example.test/v1".into(),
        models: vec!["example-1".into()],
        plugin_id: None,
        mode: "single".into(),
        rewrite: Default::default(),
    };
    let provider = store
        .create_provider(provider, "sk-upstream-secret", &[], &key_store)
        .await
        .unwrap();
    let providers = store.providers().await.unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].key_count, 1);
    assert_eq!(
        store.upstream_key(provider.id, &key_store).await.unwrap(),
        "sk-upstream-secret"
    );
    store
        .create_route(NewRoute {
            slug: "example".into(),
            name: "example".into(),
            description: String::new(),
            patterns: vec!["example-*".into()],
            provider_id: provider.id,
            model: "example-1".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
            rewrite: Default::default(),
        })
        .await
        .unwrap();

    let client = store
        .create_client_key(UpsertClientKey {
            name: "local".into(),
            models: vec!["example-*".into()],
            token_limit: 100,
            max_amount_micros: 1_000_000,
            rpm: 10,
            tpm: 1000,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    assert!(client.plaintext.starts_with("sk-sepia-"));
    drop(store);
    let reopened = Store::connect(&database).await.unwrap();
    let key = reopened
        .authenticate_client(&client.plaintext)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(key.name, "local");
    assert_eq!(key.token_limit, 100);
    assert_eq!(reopened.routes().await.unwrap()[0].slug, "example");
    let raw = std::fs::read(database).unwrap();
    assert!(
        !raw.windows(b"sk-upstream-secret".len())
            .any(|window| window == b"sk-upstream-secret")
    );
}
