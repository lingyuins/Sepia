use axum::{Json, Router, routing::get};
use sepia_gateway::{ProviderProtocol, discovery::discover_models};
use serde_json::json;
use tokio::net::TcpListener;

#[tokio::test]
async fn discovery_uses_protocol_credentials_and_returns_ids() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            Router::new().fallback(get(|| async {
                Json(json!({"data":[{"id":"example-model"}]}))
            })),
        )
        .into_future(),
    );
    let provider = sepia_gateway::Provider {
        id: 1,
        name: "test".into(),
        protocol: ProviderProtocol::OpenAI,
        endpoint: format!("http://{address}/v1"),
        models: vec![],
        key_count: 1,
        group_id: "official".into(),
        enabled: true,
        plugin_id: None,
    };
    assert_eq!(
        discover_models(&reqwest::Client::new(), &provider, "secret")
            .await
            .unwrap(),
        vec!["example-model"]
    );
}
