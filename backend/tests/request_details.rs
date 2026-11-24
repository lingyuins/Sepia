//! Rich request observation: client IP, first-byte latency, cached/reasoning
//! token detail, and opt-in request/response bodies (`log_bodies`, default off).
use axum::{Router, routing::any};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::{Value, json};
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

/// OpenAI-compatible upstream: JSON replies carry cached/reasoning usage
/// detail; `stream:true` answers with SSE chunks carrying the same detail.
async fn upstream() -> SocketAddr {
    let handler = |request: axum::extract::Request| async move {
        let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
        let model = value["model"].as_str().unwrap_or("gpt-test");
        let usage = json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "prompt_tokens_details": {"cached_tokens": 80},
            "completion_tokens_details": {"reasoning_tokens": 7}
        });
        if value["stream"].as_bool() == Some(true) {
            let frames = format!(
                "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"id":"chat_s","model":model,"choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}),
                json!({"id":"chat_s","model":model,"choices":[{"index":0,"delta":{"content":"streamed reply"},"finish_reason":null}]}),
                json!({"id":"chat_s","model":model,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                json!({"id":"chat_s","model":model,"choices":[],"usage":usage}),
            );
            return axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from(frames))
                .unwrap();
        }
        axum::response::Json(json!({
            "id":"chat_1","model":model,
            "choices":[{"index":0,"message":{"role":"assistant","content":"detailed reply"},"finish_reason":"stop"}],
            "usage":usage
        }))
        .into_response()
    };
    use axum::response::IntoResponse;
    serve(Router::new().fallback(any(handler))).await
}

// Keep the TempDir alive in the caller: dropping it deletes the SQLite files
// while pooled connections still reference them (disk I/O errors on Linux).
async fn seeded(db: &str, address: SocketAddr) -> (Store, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let keys = KeyStore::testing();
    store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{address}/v1"),
                models: vec!["gpt-test".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "upstream-secret",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "gpt-test".into(),
            name: "gpt-test".into(),
            description: String::new(),
            patterns: vec!["gpt-test".into()],
            provider_id: 1,
            model: "gpt-test".into(),
            is_default: false,
            list_members: true,
            candidates: vec![],
            rewrite: Default::default(),
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
            secret: None,
        })
        .await
        .unwrap()
        .plaintext;
    (store, key, directory)
}

async fn gateway(store: &Store) -> SocketAddr {
    serve(app(AppState::for_testing(store.clone()).await)).await
}

async fn chat(address: SocketAddr, key: &str, stream: bool, prompt: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(key)
        .json(&json!({"model":"gpt-test","stream":stream,"messages":[{"role":"user","content":prompt}]}))
        .send()
        .await
        .unwrap()
}

async fn admin_cookie(client: &reqwest::Client, address: SocketAddr, store: &Store) -> String {
    let url = |path: &str| format!("http://{address}{path}");
    let bootstrap = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":AppState::for_testing(store.clone()).await.bootstrap_token}))
        .send()
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), 200);
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn records_ip_first_byte_and_token_details_without_bodies_by_default() {
    let upstream = upstream().await;
    let (store, key, _directory) = seeded("details-default.db", upstream).await;
    let address = gateway(&store).await;
    let response = chat(address, &key, false, "hi").await;
    assert_eq!(response.status(), 200);
    let requests = store.requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let record = &requests[0];
    assert_eq!(record.status, "success");
    assert_eq!(record.client_ip.as_deref(), Some("127.0.0.1"));
    assert!(record.first_byte_ms.is_some_and(|ms| ms >= 0));
    assert!(
        record.latency_ms > 0 && record.latency_ms >= record.first_byte_ms.unwrap(),
        "latency {} first_byte {:?}",
        record.latency_ms,
        record.first_byte_ms
    );
    assert_eq!(record.cached_tokens, 80);
    assert_eq!(record.reasoning_tokens, 7);
    assert_eq!(record.input_tokens, 100);
    assert_eq!(record.output_tokens, 20);
    assert!(!record.has_body);
    assert!(store.request_bodies(&record.id).await.unwrap().is_none());

    let client = reqwest::Client::new();
    let cookie = admin_cookie(&client, address, &store).await;
    let bodies = client
        .get(format!(
            "http://{address}/api/v1/requests/{}/bodies",
            record.id
        ))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(bodies.status(), 404);
}

#[tokio::test]
async fn stores_json_bodies_when_enabled_and_stops_when_disabled() {
    let upstream = upstream().await;
    let (store, key, _directory) = seeded("details-bodies.db", upstream).await;
    store.set_setting("log_bodies", &json!(true)).await.unwrap();
    let address = gateway(&store).await;
    let response = chat(address, &key, false, "secret prompt").await;
    assert_eq!(response.status(), 200);
    let record = store.requests().await.unwrap().remove(0);
    assert!(record.has_body);
    let (request_body, response_body) = store.request_bodies(&record.id).await.unwrap().unwrap();
    assert!(request_body.contains("secret prompt"));
    let response_value: Value = serde_json::from_str(&response_body.unwrap()).unwrap();
    assert_eq!(
        response_value["choices"][0]["message"]["content"],
        "detailed reply"
    );

    let client = reqwest::Client::new();
    let cookie = admin_cookie(&client, address, &store).await;
    let bodies = client
        .get(format!(
            "http://{address}/api/v1/requests/{}/bodies",
            record.id
        ))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(bodies.status(), 200);
    let bodies: Value = bodies.json().await.unwrap();
    assert!(
        bodies["request_body"]
            .as_str()
            .unwrap()
            .contains("secret prompt")
    );
    assert!(
        bodies["response_body"]
            .as_str()
            .unwrap()
            .contains("detailed reply")
    );

    // Disabling via the management API stops new captures; history is kept.
    let update = client
        .post(format!("http://{address}/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&json!({"log_bodies": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(update.status(), 200);
    let response = chat(address, &key, false, "later prompt").await;
    assert_eq!(response.status(), 200);
    let requests = store.requests().await.unwrap();
    let latest = &requests[0];
    assert!(!latest.has_body);
    assert!(store.request_bodies(&record.id).await.unwrap().is_some());

    // Clearing history removes bodies through the cascade.
    store.clear_requests().await.unwrap();
    assert!(store.request_bodies(&record.id).await.unwrap().is_none());
}

#[tokio::test]
async fn streams_record_token_details_and_reconstructed_response_body() {
    let upstream = upstream().await;
    let (store, key, _directory) = seeded("details-stream.db", upstream).await;
    store.set_setting("log_bodies", &json!(true)).await.unwrap();
    let address = gateway(&store).await;
    let response = chat(address, &key, true, "streamed prompt").await;
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(text.contains("streamed reply"));
    let record = store.requests().await.unwrap().remove(0);
    assert_eq!(record.status, "success");
    assert_eq!(record.input_tokens, 100);
    assert_eq!(record.output_tokens, 20);
    assert_eq!(record.cached_tokens, 80);
    assert_eq!(record.reasoning_tokens, 7);
    assert_eq!(record.client_ip.as_deref(), Some("127.0.0.1"));
    assert!(record.first_byte_ms.is_some());
    assert!(record.latency_ms > 0);
    assert!(record.has_body);
    let (request_body, response_body) = store.request_bodies(&record.id).await.unwrap().unwrap();
    assert!(request_body.contains("streamed prompt"));
    let response_value: Value = serde_json::from_str(&response_body.unwrap()).unwrap();
    assert_eq!(
        response_value["choices"][0]["message"]["content"],
        "streamed reply"
    );
}
