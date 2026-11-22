use axum::{Router, extract::State, routing::post};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Hits(Arc<Mutex<Vec<(String, String)>>>);

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

async fn dual_upstream(working: &'static str) -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let handler = post(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let path = request.uri().path().to_string();
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            hits.0.lock().unwrap().push((path.clone(), secret));
            if path.ends_with(working) {
                axum::Json(if working == "/responses" {
                    json!({"id":"resp_1","object":"response","status":"completed","model":"upstream-model","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"recovered"}]}],"usage":{"input_tokens":2,"output_tokens":1}})
                } else {
                    json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"recovered"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})
                })
                .into_response()
            } else {
                axum::http::StatusCode::NOT_FOUND.into_response()
            }
        },
    );
    let address = serve(Router::new().fallback(handler).with_state(hits.clone())).await;
    (address, hits)
}

use axum::response::IntoResponse;

// Keep the TempDir alive in the caller: dropping it deletes the SQLite files
// while pooled connections still reference them (disk I/O errors on Linux).
async fn store_with_route(
    db: &str,
    protocol: ProviderProtocol,
    address: SocketAddr,
) -> (Store, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let provider = store
        .create_provider(
            NewProvider {
                name: "universal".into(),
                protocol,
                endpoint: format!("http://{address}"),
                models: vec!["upstream-model".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "first-secret",
            &[],
            &KeyStore::testing(),
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id: provider.id,
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
    (store, key, directory)
}

async fn gateway(store: &Store) -> SocketAddr {
    serve(app(AppState::for_testing(store.clone()).await)).await
}

#[tokio::test]
async fn chat_request_retries_to_responses_on_responses_only_upstream() {
    let (upstream, hits) = dual_upstream("/responses").await;
    let (store, key, _directory) =
        store_with_route("chat-to-responses.db", ProviderProtocol::OpenAI, upstream).await;
    let address = gateway(&store).await;
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
    let paths: Vec<String> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(paths, vec!["/chat/completions", "/responses"]);
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "success");
    assert_eq!(requests[0].route.as_deref(), Some("chat"));
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].status, "retry");
    assert!(
        attempts[0]
            .error
            .as_deref()
            .unwrap()
            .contains("chat/completions")
    );
    assert_eq!(attempts[1].status, "success");
}

#[tokio::test]
async fn responses_request_retries_to_chat_on_chat_only_upstream() {
    let (upstream, hits) = dual_upstream("/chat/completions").await;
    let (store, key, _directory) =
        store_with_route("responses-to-chat.db", ProviderProtocol::OpenAI, upstream).await;
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","input":"Hello","store":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let value = response.json::<Value>().await.unwrap();
    assert_eq!(value["status"], "completed");
    assert_eq!(value["output"][0]["content"][0]["text"], "recovered");
    let paths: Vec<String> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(paths, vec!["/responses", "/chat/completions"]);
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "success");
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].status, "retry");
    assert_eq!(attempts[1].status, "success");
}

#[tokio::test]
async fn responses_request_targets_responses_endpoint_first_when_available() {
    let (upstream, hits) = dual_upstream("/responses").await;
    let (store, key, _directory) =
        store_with_route("responses-direct.db", ProviderProtocol::OpenAI, upstream).await;
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","input":"Hello","store":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let paths: Vec<String> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(paths, vec!["/responses"]);
}

#[tokio::test]
async fn anthropic_request_converts_to_responses_when_chat_unavailable() {
    let (upstream, hits) = dual_upstream("/responses").await;
    let (store, key, _directory) = store_with_route(
        "anthropic-to-responses.db",
        ProviderProtocol::OpenAI,
        upstream,
    )
    .await;
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/messages"))
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .json(
            &json!({"model":"chat","messages":[{"role":"user","content":"Hello"}],"max_tokens":32}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let value = response.json::<Value>().await.unwrap();
    assert_eq!(value["content"][0]["text"], "recovered");
    assert_eq!(value["stop_reason"], "end_turn");
    let paths: Vec<String> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(paths, vec!["/chat/completions", "/responses"]);
}

#[tokio::test]
async fn gemini_request_converts_to_responses_when_chat_unavailable() {
    let (upstream, hits) = dual_upstream("/responses").await;
    let (store, key, _directory) =
        store_with_route("gemini-to-responses.db", ProviderProtocol::OpenAI, upstream).await;
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/models/chat:generateContent"))
        .header("x-goog-api-key", &key)
        .json(&json!({"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let value = response.json::<Value>().await.unwrap();
    assert_eq!(
        value["candidates"][0]["content"]["parts"][0]["text"],
        "recovered"
    );
    let paths: Vec<String> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(paths, vec!["/chat/completions", "/responses"]);
}

#[tokio::test]
async fn authentication_failures_rotate_keys_before_switching_endpoint() {
    let hits = Hits::default();
    let handler = post(
        |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let path = request.uri().path().to_string();
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            hits.0.lock().unwrap().push((path.clone(), secret.clone()));
            if path.ends_with("/responses") && secret == "Bearer second-secret" {
                axum::Json(json!({"id":"resp_1","object":"response","status":"completed","model":"upstream-model","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"recovered"}]}],"usage":{"input_tokens":2,"output_tokens":1}})).into_response()
            } else {
                axum::http::StatusCode::UNAUTHORIZED.into_response()
            }
        },
    );
    let upstream = serve(Router::new().fallback(handler).with_state(hits.clone())).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("auth-rotation.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "universal".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["upstream-model".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "first-secret",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "second-secret", None, None, &keys)
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id: provider.id,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
        })
        .await
        .unwrap();
    store.set_setting("max_attempts", &json!(4)).await.unwrap();
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
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let seen = hits.0.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            (
                "/chat/completions".to_string(),
                "Bearer first-secret".to_string()
            ),
            (
                "/chat/completions".to_string(),
                "Bearer second-secret".to_string()
            ),
            ("/responses".to_string(), "Bearer first-secret".to_string()),
            ("/responses".to_string(), "Bearer second-secret".to_string()),
        ]
    );
}

#[tokio::test]
async fn failing_both_endpoints_advances_to_next_candidate() {
    let (broken, _) = dual_upstream("/none").await;
    let success = serve(Router::new().fallback(post(|| async {
        axum::Json(json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"recovered"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}}))
    })))
    .await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("both-fail.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let first = store
        .create_provider(
            NewProvider {
                name: "broken".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{broken}"),
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
        .id;
    let second = store
        .create_provider(
            NewProvider {
                name: "healthy".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{success}"),
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
        .id;
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
    let address = gateway(&store).await;
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
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0].status, "retry");
    assert!(
        attempts[0]
            .error
            .as_deref()
            .unwrap()
            .contains("chat/completions")
    );
    assert_eq!(attempts[1].status, "retry");
    assert!(attempts[1].error.as_deref().unwrap().contains("responses"));
    assert_eq!(attempts[2].status, "success");
}
