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
                plugin_id: None,
            },
            "upstream-secret",
            &[],
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
            name: "chat".into(),
            description: String::new(),
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
    assert_eq!(requests[0].route.as_deref(), Some("chat"));
    assert_eq!(requests[0].model, "upstream-model");
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0].status, "retry");
    assert_eq!(attempts[1].status, "retry");
    assert_eq!(attempts[2].status, "success");
}
#[tokio::test]
async fn anthropic_stream_is_converted_to_openai_sse() {
    assert_anthropic_stream_is_converted_to_openai_sse("\n").await;
}

#[tokio::test]
async fn anthropic_stream_accepts_crlf_sse_frames() {
    assert_anthropic_stream_is_converted_to_openai_sse("\r\n").await;
}

async fn assert_anthropic_stream_is_converted_to_openai_sse(line_ending: &str) {
    let separator = format!("{line_ending}{line_ending}");
    let wire = [
        format!("event: message_start{line_ending}data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_1\",\"model\":\"upstream-model\"}}}}"),
        format!("event: content_block_delta{line_ending}data: {{\"type\":\"content_block_delta\",\"delta\":{{\"text\":\"streamed\"}}}}"),
        format!("event: message_delta{line_ending}data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":2,\"output_tokens\":1}}}}"),
        format!("event: message_stop{line_ending}data: {{\"type\":\"message_stop\"}}"),
    ]
    .join(&separator);
    let upstream = serve(Router::new().fallback(post(move || async move {
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(wire.clone()))
            .unwrap()
    })))
    .await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("stream.db"))
        .await
        .unwrap();
    let provider = provider(&store, "stream", ProviderProtocol::Anthropic, upstream).await;
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
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
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "success");
    assert_eq!(requests[0].model, "upstream-model");
    assert_eq!(requests[0].route.as_deref(), Some("chat"));
}

#[tokio::test]
async fn responses_stream_is_converted_to_openai_sse() {
    let wire = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"upstream-model\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"stream\"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"ed\"}\n\n",
        "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"output_index\":0,\"text\":\"streamed\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"upstream-model\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}}\n\n"
    );
    let upstream = serve(Router::new().fallback(post(move || async move {
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(wire))
            .unwrap()
    })))
    .await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("responses-stream.db"))
        .await
        .unwrap();
    let provider = provider(
        &store,
        "responses-stream",
        ProviderProtocol::OpenAIResponses,
        upstream,
    )
    .await;
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
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
    let streamed: String = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
        .filter_map(|value| {
            value["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(streamed, "streamed", "{text}");
    assert!(text.contains("\"prompt_tokens\":2"), "{text}");
    assert!(text.ends_with("data: [DONE]\n\n"), "{text}");
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "success");
    assert_eq!(requests[0].model, "upstream-model");
    assert_eq!(requests[0].route.as_deref(), Some("chat"));
}

#[tokio::test]
async fn failing_chat_candidate_fails_over_to_responses_only_upstream() {
    let failed = serve(Router::new().fallback(post(|| async {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    })))
    .await;
    let success = serve(Router::new().fallback(post(|| async {
        axum::Json(json!({"id":"resp_1","object":"response","status":"completed","model":"upstream-model","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"recovered"}]}],"usage":{"input_tokens":2,"output_tokens":1}}))
    })))
    .await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("responses-failover.db"))
        .await
        .unwrap();
    let first = provider(&store, "failing-chat", ProviderProtocol::OpenAI, failed).await;
    let second = provider(
        &store,
        "responses-only",
        ProviderProtocol::OpenAIResponses,
        success,
    )
    .await;
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
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
    assert_eq!(requests[0].route.as_deref(), Some("chat"));
    assert_eq!(requests[0].model, "upstream-model");
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0].status, "retry");
    assert_eq!(attempts[1].status, "retry");
    assert_eq!(attempts[2].status, "success");
}

/// SSE upstream that sends one delta then closes WITHOUT a stop frame or
/// [DONE]: the stream is a failure, so routing telemetry must not mark the
/// candidate healthy/sticky and the attempt must be recorded as failed.
async fn truncated_sse_upstream() -> SocketAddr {
    serve(Router::new().fallback(post(|| async {
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(
                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n",
            ))
            .unwrap()
    })))
    .await
}

#[tokio::test]
async fn truncated_stream_is_failed_not_sticky_success() {
    let upstream = truncated_sse_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("truncated.db"))
        .await
        .unwrap();
    let provider_id = provider(&store, "truncating", ProviderProtocol::OpenAI, upstream).await;
    let route = store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![(provider_id, "upstream-model".into())],
        })
        .await
        .unwrap();
    store
        .set_route_strategy(route.id, "sticky", &[])
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
    let state = AppState::for_testing(store.clone()).await;
    let address = serve(app(state.clone())).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","stream":true,"messages":[{"role":"user","content":"Hello"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    // Consume the whole SSE body so the stream's terminal handling runs.
    let body = response.text().await.unwrap();
    assert!(body.contains("partial"), "{body}");
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "failed");
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, "failed");
}
