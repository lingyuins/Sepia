use axum::{Router, routing::post};
use sepia_gateway::{
    Provider, ProviderProtocol, protocol::ChatRequest, relay::call_with_overrides,
};
use tokio::net::TcpListener;

#[tokio::test]
async fn upstream_redirects_do_not_forward_provider_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            Router::new().fallback(post(|| async { axum::http::StatusCode::FOUND })),
        )
        .into_future(),
    );
    let provider = Provider {
        id: 1,
        name: "redirect".into(),
        protocol: ProviderProtocol::Anthropic,
        endpoint: format!("http://{address}"),
        models: vec!["x".into()],
        key_count: 1,
        group_id: "official".into(),
        enabled: true,
        plugin_id: None,
        rewrite: Default::default(),
    };
    let request = ChatRequest {
        model: "x".into(),
        ..ChatRequest::default()
    };
    let response = call_with_overrides(
        &sepia_gateway::relay::protected_http_client(10).unwrap(),
        &provider,
        "secret",
        &request,
        "x",
        sepia_gateway::protocol::Protocol::Anthropic,
        &sepia_gateway::relay::Overrides::default(),
        sepia_gateway::relay::Timeouts::default(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 302);
}
