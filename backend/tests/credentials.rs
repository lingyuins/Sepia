use axum::http::{HeaderMap, HeaderValue};
use sepia_gateway::server::{ProtocolCredential, extract_credential};

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
