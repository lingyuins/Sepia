use axum::http::{HeaderMap, HeaderValue};
use sepia_gateway::{
    KeyStore,
    server::{ProtocolCredential, extract_credential},
};

#[test]
fn production_master_key_override_validates_and_encrypts() {
    let keys =
        KeyStore::production_with_override(Some("MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDA="))
            .unwrap();
    let (nonce, ciphertext) = keys.encrypt("upstream-secret").unwrap();
    assert_eq!(
        &*keys.decrypt(&nonce, &ciphertext).unwrap(),
        "upstream-secret"
    );

    for invalid in ["not-base64", "AA=="] {
        let error = match KeyStore::production_with_override(Some(invalid)) {
            Ok(_) => panic!("invalid master key accepted"),
            Err(error) => error,
        };
        assert_eq!(error, "invalid master key");
    }
}

#[tokio::test]
async fn keystore_marker_rejects_a_changed_master_key() {
    let directory = tempfile::tempdir().unwrap();
    let store = sepia_gateway::Store::connect(&directory.path().join("keystore.db"))
        .await
        .unwrap();
    let original =
        KeyStore::production_with_override(Some("MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDA="))
            .unwrap();
    store.verify_keystore(&original).await.unwrap();
    store.verify_keystore(&original).await.unwrap();
    let replacement =
        KeyStore::production_with_override(Some("MTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTE="))
            .unwrap();
    assert_eq!(
        store.verify_keystore(&replacement).await.unwrap_err(),
        "master key cannot decrypt existing credentials"
    );
}

#[test]
fn native_protocol_credentials_validate_version_and_transport() {
    let mut headers = HeaderMap::new();
    headers.insert("x-goog-api-key", HeaderValue::from_static("header-key"));
    assert_eq!(
        extract_credential(&headers, ProtocolCredential::Gemini, None).unwrap(),
        "header-key"
    );
    assert_eq!(
        extract_credential(
            &HeaderMap::new(),
            ProtocolCredential::Gemini,
            Some("key=query-key")
        )
        .unwrap(),
        "query-key"
    );
    headers.insert("x-api-key", HeaderValue::from_static("anthropic-key"));
    assert!(extract_credential(&headers, ProtocolCredential::Anthropic, None).is_err());
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    assert_eq!(
        extract_credential(&headers, ProtocolCredential::Anthropic, None).unwrap(),
        "anthropic-key"
    );
}
