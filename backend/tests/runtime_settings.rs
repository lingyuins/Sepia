use axum::{Router, routing::post};
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

fn chat_reply() -> Value {
    json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"recovered"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})
}

#[tokio::test]
async fn numeric_settings_are_validated_and_reach_the_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("settings.db"))
        .await
        .unwrap();
    let snapshots = sepia_gateway::snapshot::Snapshots::load(&store)
        .await
        .unwrap();
    let defaults = snapshots.get();
    assert_eq!(defaults.max_attempts, 3);
    assert_eq!(defaults.connect_timeout_seconds, 10);
    assert_eq!(defaults.first_byte_timeout_seconds, 60);
    assert_eq!(defaults.stream_idle_timeout_seconds, 60);
    assert_eq!(defaults.retry_after_cap_seconds, 5);
    drop(defaults);
    for (key, max) in [
        ("connect_timeout_seconds", 60u64),
        ("first_byte_timeout_seconds", 600),
        ("stream_idle_timeout_seconds", 600),
        ("max_attempts", 16),
        ("retry_after_cap_seconds", 60),
    ] {
        for invalid in [json!(0), json!(max + 1), json!("4"), json!(1.5)] {
            assert!(
                store.set_setting(key, &invalid).await.is_err(),
                "{key} accepted {invalid}"
            );
        }
        store.set_setting(key, &json!(max)).await.unwrap();
    }
    snapshots.refresh(&store).await.unwrap();
    let configured = snapshots.get();
    assert_eq!(configured.max_attempts, 16);
    assert_eq!(configured.connect_timeout_seconds, 60);
    assert_eq!(configured.first_byte_timeout_seconds, 600);
    assert_eq!(configured.stream_idle_timeout_seconds, 600);
    assert_eq!(configured.retry_after_cap_seconds, 60);
}

// Keep the TempDir alive in the caller while pooled connections use the files.
async fn store_with_two_candidates(
    db: &str,
    slow: SocketAddr,
    healthy: SocketAddr,
) -> (Store, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let keys = KeyStore::testing();
    let mut ids = Vec::new();
    for (name, address) in [("slow", slow), ("healthy", healthy)] {
        ids.push(
            store
                .create_provider(
                    NewProvider {
                        name: name.into(),
                        protocol: ProviderProtocol::OpenAI,
                        endpoint: format!("http://{address}"),
                        models: vec!["upstream-model".into()],
                        plugin_id: None,
                        mode: "single".into(),
                    },
                    "upstream-secret",
                    &[],
                    &keys,
                )
                .await
                .unwrap()
                .id,
        );
    }
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id: ids[0],
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![
                (ids[0], "upstream-model".into()),
                (ids[1], "upstream-model".into()),
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
            secret: None,
        })
        .await
        .unwrap()
        .plaintext;
    (store, key, directory)
}

#[tokio::test]
async fn configured_first_byte_timeout_fails_over_to_the_next_candidate() {
    // The slow upstream never returns headers within the configured 1s budget.
    let slow = serve(Router::new().fallback(post(|| async {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        axum::Json(chat_reply())
    })))
    .await;
    let healthy = serve(Router::new().fallback(post(|| async { axum::Json(chat_reply()) }))).await;
    let (store, key, _directory) = store_with_two_candidates("first-byte.db", slow, healthy).await;
    store
        .set_setting("first_byte_timeout_seconds", &json!(1))
        .await
        .unwrap();
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
        response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
        "recovered"
    );
    let requests = store.requests().await.unwrap();
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    // The timed-out key enters the 60s cooldown, so the slow provider's second
    // leg has no eligible key and is skipped: timeout retry, then healthy success.
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].status, "retry");
    assert!(
        attempts[0]
            .error
            .as_deref()
            .unwrap()
            .contains("upstream timed out"),
        "{:?}",
        attempts[0].error
    );
    assert!(attempts[0].latency_ms < 5_000);
    assert_eq!(attempts[1].status, "success");
}

#[tokio::test]
async fn configured_stream_idle_timeout_fails_a_stalled_sse_stream() {
    // One delta, then silence: the 1s idle budget must terminate the stream
    // instead of the former fixed 60s wait, and the request is recorded failed.
    let stalled = serve(Router::new().fallback(post(|| async {
        let stream = async_stream::stream! {
            yield Ok::<_, std::io::Error>("data: {\"id\":\"chat_1\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n");
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        };
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from_stream(stream))
            .unwrap()
    })))
    .await;
    let healthy = serve(Router::new().fallback(post(|| async { axum::Json(chat_reply()) }))).await;
    let (store, key, _directory) =
        store_with_two_candidates("stream-idle.db", stalled, healthy).await;
    store
        .set_setting("stream_idle_timeout_seconds", &json!(1))
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let started = std::time::Instant::now();
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}],"stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(text.contains("partial"), "{text}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "idle timeout did not terminate the stalled stream: {:?}",
        started.elapsed()
    );
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "failed");
}

#[tokio::test]
async fn long_streams_survive_beyond_the_first_byte_budget() {
    // Regression: a total reqwest timeout used to bound the whole SSE body,
    // cutting streams longer than the header budget. Gaps stay under the idle
    // budget, so a stream spanning several seconds must complete intact.
    let slow_stream = serve(Router::new().fallback(post(|| async {
        let stream = async_stream::stream! {
            yield Ok::<_, std::io::Error>("data: {\"id\":\"chat_1\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"one\"}}]}\n\n");
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            yield Ok::<_, std::io::Error>("data: {\"id\":\"chat_1\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"two\"}}]}\n\n");
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            yield Ok::<_, std::io::Error>("data: {\"id\":\"chat_1\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1}}\n\n");
            yield Ok::<_, std::io::Error>("data: [DONE]\n\n");
        };
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from_stream(stream))
            .unwrap()
    })))
    .await;
    let healthy = serve(Router::new().fallback(post(|| async { axum::Json(chat_reply()) }))).await;
    let (store, key, _directory) =
        store_with_two_candidates("long-stream.db", slow_stream, healthy).await;
    store
        .set_setting("first_byte_timeout_seconds", &json!(1))
        .await
        .unwrap();
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
    assert!(text.contains("one") && text.contains("two"), "{text}");
    assert!(text.contains("[DONE]"), "{text}");
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "success");
}
