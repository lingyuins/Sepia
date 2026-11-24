use sepia_gateway::{KeyStore, NewProvider, ProviderProtocol, Store};
use std::collections::HashMap;

#[tokio::test]
async fn second_provider_key_is_used_after_authentication_failure() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("keys.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "multi".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://localhost/v1".into(),
                models: vec!["x".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "first",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "second", None, None, &keys)
        .await
        .unwrap();
    let first = store
        .upstream_key_for_attempt(provider.id, None, "x", 0, &HashMap::new(), &keys)
        .await
        .unwrap()
        .unwrap();
    let second = store
        .upstream_key_for_attempt(provider.id, None, "x", 1, &HashMap::new(), &keys)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.0, second.0);
    assert_eq!(first.1, "first");
    assert_eq!(second.1, "second");
}
