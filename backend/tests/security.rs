use sepia_gateway::crypto::token_hash;
use sepia_gateway::server::glob;
use sepia_gateway::{KeyStore, NewProvider, ProviderProtocol, Store};

#[test]
fn route_globs_do_not_treat_suffix_as_literal() {
    assert!(glob("claude-*", "claude-sonnet"));
    assert!(glob("*", "anything"));
    assert!(glob("gpt-*-mini", "gpt-4-mini"));
    assert!(!glob("claude-*", "gpt-4"));
    assert!(!glob("exact", "exact-other"));
}

#[test]
fn secrets_and_client_tokens_never_have_equal_plaintext_storage() {
    let keys = KeyStore::testing();
    let (nonce, encrypted) = keys.encrypt("secret-value").unwrap();
    assert_ne!(encrypted, "secret-value");
    assert_eq!(&*keys.decrypt(&nonce, &encrypted).unwrap(), "secret-value");
    assert_ne!(token_hash("client-token"), "client-token");
}

#[tokio::test]
async fn provider_creation_rolls_back_invalid_models() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("atomic.db"))
        .await
        .unwrap();
    let result = store
        .create_provider(
            NewProvider {
                name: "bad".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "file:///secret".into(),
                models: vec!["x".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "secret",
            &[],
            &KeyStore::testing(),
        )
        .await;
    assert!(result.is_err());
    assert!(store.providers().await.unwrap().is_empty());
}
