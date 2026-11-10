use axum::{Router, routing::post};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::json;
use std::net::SocketAddr;
use tokio::net::TcpListener;

async fn serve(router: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );
    address
}
async fn provider(
    store: &Store,
    name: &str,
    protocol: ProviderProtocol,
    address: SocketAddr,
) -> i64 {
    let endpoint = if protocol == ProviderProtocol::Gemini {
        format!("http://{address}/v1beta")
    } else {
        format!("http://{address}")
    };
    store
        .create_provider(
            NewProvider {
                name: name.into(),
                protocol,
                endpoint,
                models: vec!["upstream-model".into()],
            },
            "upstream-secret",
            &KeyStore::testing(),
        )
        .await
        .unwrap()
        .id
}
#[tokio::test]
async fn failed_first_candidate_fails_over_and_records_attempts() {
    let failed = serve(Router::new().fallback(post(|| async {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    })))
    .await;
    let success = serve(Router::new().fallback(post(|| async { axum::Json(json!({"id":"msg_1","model":"upstream-model","content":[{"type":"text","text":"recovered"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":1}})) }))).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("failover.db"))
        .await
        .unwrap();
    let first = provider(&store, "failing", ProviderProtocol::OpenAI, failed).await;
    let second = provider(&store, "recovery", ProviderProtocol::Anthropic, success).await;
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            patterns: vec!["chat".into()],
            provider_id: first,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![
                (first, "upstream-model".into()),
                (second, "upstream-model".into()),
            ],
        })
        .await
        .unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "test".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap()
        .plaintext;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["choices"][0]["message"]["content"],
        "recovered"
    );
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "success");
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].status, "retry");
    assert_eq!(attempts[1].status, "success");
}
#[tokio::test]
async fn anthropic_stream_is_converted_to_openai_sse() {
    let upstream = serve(Router::new().fallback(post(|| async {
        axum::response::Response::builder().header("content-type", "text/event-stream").body(axum::body::Body::from(concat!("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"upstream-model\"}}\n\n", "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"streamed\"}}\n\n", "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}\n\n", "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"))).unwrap()
    }))).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("stream.db"))
        .await
        .unwrap();
    let provider = provider(&store, "stream", ProviderProtocol::Anthropic, upstream).await;
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            patterns: vec!["chat".into()],
            provider_id: provider,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
        })
        .await
        .unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "test".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap()
        .plaintext;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}],"stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(text.contains("streamed"), "{text}");
    assert!(text.contains("data: [DONE]"), "{text}");
    assert!(text.ends_with("data: [DONE]\n\n"), "{text}");
}
