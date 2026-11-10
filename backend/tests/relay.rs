use axum::{Router, extract::State, response::Json, routing::post};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<(String, String, Value)>>>);
async fn upstream(
    State(captured): State<Captured>,
    request: axum::extract::Request,
) -> Json<Value> {
    let path = request.uri().path().to_string();
    let authorization = request
        .headers()
        .get("authorization")
        .or_else(|| request.headers().get("x-api-key"))
        .or_else(|| request.headers().get("x-goog-api-key"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    captured
        .0
        .lock()
        .unwrap()
        .push((path.clone(), authorization, value.clone()));
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("example")
        .to_string();
    Json(if path.contains("messages") {
        json!({"id":"msg_test","model":model,"role":"assistant","content":[{"type":"text","text":"Hello from Sepia"}],"stop_reason":"end_turn","usage":{"input_tokens":4,"output_tokens":3}})
    } else if path.contains("generateContent") {
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":"Hello from Sepia"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":3}})
    } else {
        json!({"id":"chat_test","model":model,"choices":[{"index":0,"message":{"role":"assistant","content":"Hello from Sepia"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":3}})
    })
}
async fn spawn_upstream() -> (SocketAddr, Captured) {
    let captured = Captured::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .fallback(post(upstream))
        .with_state(captured.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (address, captured)
}
fn body(protocol: &str, model: &str, stream: bool) -> Value {
    match protocol {
        "anthropic" => {
            json!({"model":model,"messages":[{"role":"user","content":"Hello"}],"max_tokens":32,"stream":stream})
        }
        "gemini" => {
            json!({"contents":[{"role":"user","parts":[{"text":"Hello"}]}],"generationConfig":{"maxOutputTokens":32}})
        }
        _ => {
            json!({"model":model,"messages":[{"role":"user","content":"Hello"}],"max_tokens":32,"stream":stream})
        }
    }
}
async fn request(address: SocketAddr, protocol: &str, model: &str, key: &str) -> Value {
    let (path, auth) = match protocol {
        "anthropic" => ("/v1/messages".into(), ("x-api-key", key.to_string())),
        "gemini" => (
            format!("/v1beta/models/{model}:generateContent"),
            ("x-goog-api-key", key.to_string()),
        ),
        _ => (
            "/v1/chat/completions".into(),
            ("authorization", format!("Bearer {key}")),
        ),
    };
    let response = reqwest::Client::new()
        .post(format!("http://{address}{path}"))
        .header("anthropic-version", "2023-06-01")
        .header(auth.0, auth.1)
        .json(&body(protocol, model, false))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        200,
        "{protocol} → {model}: {}",
        response.text().await.unwrap_or_default()
    );
    response.json().await.unwrap()
}
#[tokio::test]
async fn all_native_protocols_relay_to_all_provider_protocols() {
    let (upstream, captured) = spawn_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("test.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    for protocol in ["openai", "anthropic", "gemini"] {
        let protocol = match protocol {
            "anthropic" => sepia_gateway::ProviderProtocol::Anthropic,
            "gemini" => sepia_gateway::ProviderProtocol::Gemini,
            _ => sepia_gateway::ProviderProtocol::OpenAI,
        };
        let base = if protocol == sepia_gateway::ProviderProtocol::Gemini {
            format!("http://{upstream}/v1beta")
        } else {
            format!("http://{upstream}")
        };
        store
            .create_provider(
                NewProvider {
                    name: format!("{protocol:?}"),
                    protocol,
                    endpoint: base,
                    models: vec!["example".into()],
                },
                "upstream-secret",
                &keys,
            )
            .await
            .unwrap();
    }
    let providers = store.providers().await.unwrap();
    for (index, provider) in providers.iter().enumerate() {
        store
            .create_route(NewRoute {
                slug: format!("example-{index}"),
                patterns: vec![format!("example-{index}")],
                provider_id: provider.id,
                model: "example".into(),
                is_default: false,
                list_members: true,
                candidates: vec![],
            })
            .await
            .unwrap();
    }
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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            app(AppState::for_testing(store.clone()).await)
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );
    for inbound in ["openai", "anthropic", "gemini"] {
        for outbound in 0..3 {
            let response = request(address, inbound, &format!("example-{outbound}"), &key).await;
            match inbound {
                "anthropic" => assert_eq!(response["content"][0]["text"], "Hello from Sepia"),
                "gemini" => assert_eq!(
                    response["candidates"][0]["content"]["parts"][0]["text"],
                    "Hello from Sepia"
                ),
                _ => assert_eq!(
                    response["choices"][0]["message"]["content"],
                    "Hello from Sepia"
                ),
            }
        }
    }
    let calls = captured.0.lock().unwrap();
    assert_eq!(calls.len(), 9);
    assert!(
        calls
            .iter()
            .all(|(_, secret, _)| secret.contains("upstream-secret"))
    );
    assert!(
        calls
            .iter()
            .any(|(path, _, _)| path.ends_with("/chat/completions"))
    );
    assert!(
        calls
            .iter()
            .any(|(path, _, _)| path.ends_with("/v1/messages"))
    );
    assert!(
        calls
            .iter()
            .any(|(path, _, _)| path.ends_with(":generateContent"))
    );
}
