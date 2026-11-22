use axum::{Json, Router, response::IntoResponse, routing::post};
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
                plugin_id: None,
                mode: "single".into(),
            },
            "upstream-secret",
            &[],
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

#[tokio::test]
async fn response_string_input_becomes_an_upstream_user_message() {
    let upstream = Router::new().route(
        "/chat/completions",
        post(|Json(body): Json<Value>| async move {
            assert_eq!(body["messages"], json!([{"role":"user","content":"Hello Responses"}]));
            assert_eq!(body["max_tokens"], 64);
            assert_eq!(body["temperature"], 0.2);
            Json(json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"Response text"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}))
        }),
    );
    let (address, _, key, _directory) = gateway(upstream).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(key)
        .json(&json!({"model":"chat","input":"Hello Responses","max_output_tokens":64,"temperature":0.2}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["object"], "response");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["model"], "upstream-model");
    assert_eq!(body["output"][0]["type"], "message");
    assert_eq!(body["output"][0]["role"], "assistant");
    assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
    assert_eq!(body["output"][0]["content"][0]["text"], "Response text");
    assert_eq!(body["usage"]["input_tokens"], 4);
    assert_eq!(body["usage"]["output_tokens"], 2);
    assert_eq!(body["usage"]["total_tokens"], 6);
}

#[tokio::test]
async fn response_structured_input_preserves_roles_instructions_and_text_parts() {
    let upstream = Router::new().route(
        "/chat/completions",
        post(|Json(body): Json<Value>| async move {
            assert_eq!(body["messages"], json!([
                {"role":"system","content":"Follow the instructions\nBe concise"},
                {"role":"user","content":"Hello there"},
                {"role":"assistant","content":"Previous reply"}
            ]));
            Json(json!({"id":"chat_2","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"Next reply"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3}}))
        }),
    );
    let (address, _, key, _directory) = gateway(upstream).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(key)
        .json(&json!({
            "model":"chat",
            "instructions":"Follow the instructions",
            "input":[
                {"type":"message","role":"developer","content":"Be concise"},
                {"role":"user","content":[{"type":"input_text","text":"Hello "},{"type":"input_text","text":"there"}]},
                {"role":"assistant","content":[{"type":"output_text","text":"Previous reply"}]}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["output"][0]["content"][0]["text"], "Next reply");
}

#[tokio::test]
async fn response_input_stream_emits_response_events_with_usage() {
    let upstream = Router::new().route(
        "/chat/completions",
        post(|| async {
            let stream = concat!(
                "data: {\"id\":\"chat_stream\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}],\"usage\":null}\n\n",
                "data: {\"id\":\"chat_stream\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Streamed\"},\"finish_reason\":null}],\"usage\":null}\n\n",
                "data: {\"id\":\"chat_stream\",\"model\":\"upstream-model\",\"choices\":[],\"usage\":{\"prompt_tokens\":6,\"completion_tokens\":2}}\n\n",
                "data: {\"id\":\"chat_stream\",\"model\":\"upstream-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n",
                "data: [DONE]\n\n"
            );
            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from(stream))
                .unwrap()
                .into_response()
        }),
    );
    let (address, store, key, _directory) = gateway(upstream).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(key)
        .json(&json!({"model":"chat","input":"Stream please","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    for event in [
        "response.created",
        "response.output_item.added",
        "response.content_part.added",
        "response.output_text.delta",
        "response.output_text.done",
        "response.content_part.done",
        "response.output_item.done",
        "response.completed",
    ] {
        assert!(
            body.contains(&format!("event: {event}\n")),
            "{event} missing: {body}"
        );
    }
    assert!(body.contains("\"delta\":\"Streamed\""), "{body}");
    let completed = body
        .split("\n\n")
        .find_map(|frame| {
            let data = frame.strip_prefix("event: response.completed\ndata: ")?;
            serde_json::from_str::<Value>(data).ok()
        })
        .expect("response.completed must contain JSON");
    assert_eq!(completed["sequence_number"], 8);
    assert_eq!(completed["response"]["status"], "completed");
    assert_eq!(
        completed["response"]["output"][0]["content"][0]["text"],
        "Streamed"
    );
    assert_eq!(completed["response"]["usage"]["input_tokens"], 6);
    assert_eq!(completed["response"]["usage"]["output_tokens"], 2);
    let requests = store.requests().await.unwrap();
    assert_eq!(requests[0].status, "success");
    assert_eq!(requests[0].model, "upstream-model");
}

#[tokio::test]
async fn response_input_image_is_forwarded_to_the_upstream() {
    let upstream = Router::new().route(
        "/chat/completions",
        post(|Json(body): Json<Value>| async move {
            assert!(body["messages"][0]["content"].as_array().unwrap().iter().any(|part| part["type"] == "image_url" && part["image_url"]["url"] == "https://example.test/cat.png"));
            Json(json!({"id":"chat_image","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"A cat"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}))
        }),
    );
    let (address, _, key, _directory) = gateway(upstream).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/responses"))
        .bearer_auth(key)
        .json(&json!({"model":"chat","input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.test/cat.png"}]}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["output"][0]["content"][0]["text"], "A cat");
}
