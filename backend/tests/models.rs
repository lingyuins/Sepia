use axum::{Router, body::Body, http::Request, response::Json, routing::post};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::json;
use tokio::net::TcpListener;
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

#[tokio::test]
async fn a_candidate_permission_exposes_its_route_slug() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/chat/completions",
                post(|| async {
                    Json(json!({"id":"chat_test","model":"GLM-5.3-Premium","choices":[{"index":0,"message":{"role":"assistant","content":"Hello from route"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}))
                }),
            ),
        )
        .await
        .unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("models.db"))
        .await
        .unwrap();
    let provider = store
        .create_provider(
            NewProvider {
                name: "Provider".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["GLM-5.3-Premium".into()],
            },
            "upstream-secret",
            &KeyStore::testing(),
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "hbhh".into(),
            name: "Route".into(),
            description: String::new(),
            patterns: vec!["*".into()],
            provider_id: provider.id,
            model: "GLM-5.3-Premium".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
        })
        .await
        .unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "Candidate-only client".into(),
            models: vec!["GLM-5.3-Premium".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap();
    let gateway_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway = gateway_listener.local_addr().unwrap();
    let gateway_store = store.clone();
    tokio::spawn(async move {
        axum::serve(
            gateway_listener,
            app(AppState::for_testing(gateway_store).await)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let inference = reqwest::Client::new()
        .post(format!("http://{gateway}/v1/chat/completions"))
        .bearer_auth(&key.plaintext)
        .json(&json!({"model":"hbhh","messages":[{"role":"user","content":"hello"}]}))
        .send()
        .await
        .unwrap();
    let inference_status = inference.status();
    let inference_body = inference.bytes().await.unwrap();
    let inference_json: serde_json::Value = serde_json::from_slice(&inference_body).unwrap();
    assert_eq!(inference_status, 200, "{inference_json}");
    assert_eq!(
        inference_json["choices"][0]["message"]["content"],
        "Hello from route"
    );
    let response = app(AppState::for_testing(store).await)
        .oneshot(
            Request::get("/v1/models")
                .header("authorization", format!("Bearer {}", key.plaintext))
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
            {"id":"GLM-5.3-Premium","object":"model","owned_by":"sepia"},
            {"id":"hbhh","object":"model","owned_by":"sepia"}
        ])
    );
}
