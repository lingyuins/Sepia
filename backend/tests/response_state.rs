use axum::{Json, Router, routing::post};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
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
async fn gateway(upstream: Router) -> (SocketAddr, Store, String, tempfile::TempDir) {
    let upstream = serve(upstream).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("responses.db"))
        .await
        .unwrap();
    let provider = store
        .create_provider(
            NewProvider {
                name: "Upstream".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["upstream-model".into()],
            },
            "upstream-secret",
            &KeyStore::testing(),
        )
        .await
        .unwrap()
        .id;
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "Chat".into(),
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
            name: "responses".into(),
            models: vec!["chat".into()],
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
    (address, store, key, directory)
}
fn reply(text: &str) -> Json<Value> {
    Json(
        json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}),
    )
}

#[tokio::test]
async fn previous_response_replays_ordered_tool_history() {
    let upstream = Router::new().route("/chat/completions", post(|Json(body): Json<Value>| async move {
        if body["messages"].as_array().unwrap().len() == 1 {
            Json(json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_weather","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}))
        } else {
            assert!(body["messages"].as_array().unwrap().iter().any(|message| message["tool_calls"][0]["id"] == "call_weather"));
            assert!(body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "tool" && message["tool_call_id"] == "call_weather" && message["content"] == "sunny"));
            reply("It is sunny")
        }
    }));
    let (address, store, key, _directory) = gateway(upstream).await;
    let client = reqwest::Client::new();
    let first = client
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","input":"Weather?"}))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 200);
    let first: Value = first.json().await.unwrap();
    assert_eq!(first["output"][0]["type"], "function_call");
    assert_eq!(first["output"][0]["call_id"], "call_weather");
    let stored = store
        .response(
            1,
            first["id"].as_str().unwrap().strip_prefix("resp_").unwrap(),
        )
        .await
        .unwrap();
    assert!(
        stored.is_some(),
        "response was not persisted: {first}; stored={stored:?}"
    );
    let second = client.post(format!("http://{address}/v1/responses")).bearer_auth(&key).json(&json!({"model":"chat","previous_response_id":first["id"],"input":[{"type":"function_call_output","call_id":"call_weather","output":"sunny"}]})).send().await.unwrap();
    assert_eq!(second.status(), 200, "{}", second.text().await.unwrap());
    let second: Value = second.json().await.unwrap();
    assert_eq!(second["output"][0]["content"][0]["text"], "It is sunny");
}

#[tokio::test]
async fn background_response_is_polled_and_cancelled() {
    let upstream = Router::new().route(
        "/chat/completions",
        post(|| async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            reply("Background done")
        }),
    );
    let (address, _, key, _directory) = gateway(upstream).await;
    let client = reqwest::Client::new();
    let created = client
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","input":"Work","background":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200);
    let created: Value = created.json().await.unwrap();
    assert!(matches!(
        created["status"].as_str(),
        Some("queued" | "in_progress")
    ));
    let id = created["id"].as_str().unwrap();
    let mut completed = None;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        let current: Value = client
            .get(format!("http://{address}/v1/responses/{id}"))
            .bearer_auth(&key)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["status"] == "completed" {
            completed = Some(current);
            break;
        }
    }
    assert_eq!(
        completed.unwrap()["output"][0]["content"][0]["text"],
        "Background done"
    );
    let cancelled = client
        .post(format!("http://{address}/v1/responses/{id}/cancel"))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), 200);
    assert_eq!(
        cancelled.json::<Value>().await.unwrap()["status"],
        "cancelled"
    );
}

#[tokio::test]
async fn running_background_response_can_be_cancelled() {
    let upstream = Router::new().route(
        "/chat/completions",
        post(|| async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            reply("too late")
        }),
    );
    let (address, store, key, _directory) = gateway(upstream).await;
    let client = reqwest::Client::new();
    let created: Value = client
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","input":"Work","background":true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();
    let internal_id = id.strip_prefix("resp_").unwrap();
    for _ in 0..50 {
        let current = store.response(1, internal_id).await.unwrap().unwrap();
        if current.status == "in_progress" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        store
            .response(1, internal_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "in_progress"
    );
    let cancelled = client
        .post(format!("http://{address}/v1/responses/{id}/cancel"))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), 200);
    assert_eq!(
        cancelled.json::<Value>().await.unwrap()["status"],
        "cancelled"
    );
    tokio::time::sleep(Duration::from_millis(550)).await;
    let current: Value = client
        .get(format!("http://{address}/v1/responses/{id}"))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current["status"], "cancelled");
    assert_ne!(current["output"][0]["content"][0]["text"], "too late");
}

#[tokio::test]
async fn background_upstream_failure_reaches_failed_state() {
    let upstream = Router::new().route(
        "/chat/completions",
        post(|| async { axum::http::StatusCode::BAD_GATEWAY }),
    );
    let (address, _, key, _directory) = gateway(upstream).await;
    let client = reqwest::Client::new();
    let created: Value = client
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","input":"Work","background":true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        let current: Value = client
            .get(format!("http://{address}/v1/responses/{id}"))
            .bearer_auth(&key)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["status"] == "failed" {
            return;
        }
    }
    panic!("background response did not reach failed state");
}

#[tokio::test]
async fn unstored_response_cannot_be_retrieved() {
    let (address, _, key, _directory) =
        gateway(Router::new().route("/chat/completions", post(|| async { reply("private") })))
            .await;
    let client = reqwest::Client::new();
    let created: Value = client
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","input":"Private","store":false}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["store"], false);
    assert_eq!(created["output"][0]["content"][0]["text"], "private");
    let response = client
        .get(format!(
            "http://{address}/v1/responses/{}",
            created["id"].as_str().unwrap()
        ))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn conversation_items_are_scoped_and_paginated() {
    let (address, store, key, _directory) =
        gateway(Router::new().route("/chat/completions", post(|| async { reply("ok") }))).await;
    let client = reqwest::Client::new();
    let created = client.post(format!("http://{address}/v1/conversations")).bearer_auth(&key).json(&json!({"metadata":{"topic":"test"},"items":[{"type":"message","role":"user","content":[{"type":"input_text","text":"Hello"}]}]})).send().await.unwrap();
    assert_eq!(created.status(), 200, "{}", created.text().await.unwrap());
    let conversation: Value = created.json().await.unwrap();
    let id = conversation["id"].as_str().unwrap();
    let listed = client
        .get(format!(
            "http://{address}/v1/conversations/{id}/items?limit=1&order=asc"
        ))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap();
    assert_eq!(listed.status(), 200);
    let items: Value = listed.json().await.unwrap();
    assert_eq!(items["data"].as_array().unwrap().len(), 1);
    let other = store
        .create_client_key(UpsertClientKey {
            name: "other".into(),
            models: vec!["chat".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap()
        .plaintext;
    let denied = client
        .get(format!("http://{address}/v1/conversations/{id}"))
        .bearer_auth(other)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 404);
}
