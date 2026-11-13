use axum::{body::Body, http::Request};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use tower::ServiceExt;

#[tokio::test]
async fn anthropic_model_listing_does_not_require_a_version_header() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("models.db"))
        .await
        .unwrap();
    let provider = store
        .create_provider(
            NewProvider {
                name: "Anthropic".into(),
                protocol: ProviderProtocol::Anthropic,
                endpoint: "http://localhost".into(),
                models: vec!["claude-sonnet".into()],
            },
            "upstream-secret",
            &KeyStore::testing(),
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "claude".into(),
            name: "Claude".into(),
            description: String::new(),
            patterns: vec!["claude-*".into()],
            provider_id: provider.id,
            model: "claude-sonnet".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
        })
        .await
        .unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "Anthropic client".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap();
    let state = AppState::for_testing(store).await;
    let response = app(state)
        .oneshot(
            Request::get("/v1/models")
                .header("x-api-key", key.plaintext)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let models: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        models["data"],
        serde_json::json!([
            {"id":"claude","type":"model","display_name":"claude"},
            {"id":"claude-sonnet","type":"model","display_name":"claude-sonnet"}
        ])
    );
}
