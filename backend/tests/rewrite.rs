use axum::{Router, extract::State, response::IntoResponse, routing::any};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    rewrite::{HeaderRule, ParamDefaultRule, ParamReplaceRule, PromptReplaceRule, RewriteConfig},
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<Value>>>);

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

// Echo upstream: records every request's path, selected headers and parsed body
// so tests can assert exactly what the gateway sent after rewriting.
#[axum::debug_handler(state = Captured)]
async fn echo_handler(
    State(captured): State<Captured>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let path = request.uri().path().to_string();
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let x_trace = request
        .headers()
        .get("x-trace")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = axum::body::to_bytes(request.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let value: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };
    captured.0.lock().unwrap().push(json!({
        "path": path,
        "user_agent": user_agent,
        "x_trace": x_trace,
        "origin": origin,
        "authorization": authorization,
        "body": value,
    }));
    if path.ends_with("/models") {
        return axum::Json(json!({"data":[{"id":"alias-real"}]})).into_response();
    }
    if path.ends_with("/embeddings") {
        return axum::Json(json!({"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.1]}],"model":value.get("model").cloned().unwrap_or(json!("")),"usage":{"prompt_tokens":1,"total_tokens":1}})).into_response();
    }
    let model = value.get("model").and_then(Value::as_str).unwrap_or("");
    let system = value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages
                .iter()
                .find(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        })
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    axum::Json(json!({"id":"chat_1","model":model,"choices":[{"index":0,"message":{"role":"assistant","content":format!("model={model} system={system}")},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}})).into_response()
}

async fn echo_upstream() -> (SocketAddr, Captured) {
    let captured = Captured::default();
    let handler = any(echo_handler);
    let address = serve(Router::new().fallback(handler).with_state(captured.clone())).await;
    (address, captured)
}

async fn admin(address: SocketAddr, store: &Store) -> (reqwest::Client, String) {
    let client = reqwest::Client::new();
    let url = format!("http://{address}");
    let state = AppState::for_testing(store.clone()).await;
    client
        .post(format!("{url}/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":state.bootstrap_token}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(format!("{url}/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    (client, cookie)
}

async fn client_key(store: &Store, models: Vec<String>) -> String {
    store
        .create_client_key(UpsertClientKey {
            name: "rw".into(),
            models,
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap()
        .plaintext
}

#[tokio::test]
async fn inference_applies_headers_model_alias_and_system_prompt_rewrite() {
    let (upstream, captured) = echo_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("rw.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "rw".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["alias-model".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: RewriteConfig {
                    headers: vec![HeaderRule {
                        name: "X-Trace".into(),
                        value: "provider-trace".into(),
                    }],
                    param_replace: vec![ParamReplaceRule {
                        field: "model".into(),
                        pattern: "^alias-(.+)$".into(),
                        replacement: "real-$1".into(),
                    }],
                    prompt_replace: vec![PromptReplaceRule {
                        pattern: "SECRET".into(),
                        replacement: "PUBLIC".into(),
                    }],
                    param_defaults: vec![ParamDefaultRule {
                        field: "temperature".into(),
                        value: json!(0.3),
                    }],
                },
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "alias-model".into(),
            name: "alias-model".into(),
            description: String::new(),
            patterns: vec!["alias-model".into()],
            provider_id: provider.id,
            model: "alias-model".into(),
            is_default: false,
            list_members: false,
            candidates: vec![(provider.id, "alias-model".into())],
            rewrite: RewriteConfig {
                headers: vec![HeaderRule {
                    name: "x-trace".into(),
                    value: "route-trace".into(),
                }],
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let plaintext = client_key(&store, vec!["alias-model".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;

    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"alias-model","messages":[{"role":"system","content":"keep SECRET safe"},{"role":"user","content":"SECRET user text"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    // The outbound model is rewritten, the system prompt is rewritten, and the
    // user message is untouched. The reply echoes what the upstream received.
    let content = body["choices"][0]["message"]["content"].as_str().unwrap();
    assert!(
        content.contains("model=real-model"),
        "outbound model rewritten: {content}"
    );
    assert!(
        content.contains("system=keep PUBLIC safe"),
        "system rewritten: {content}"
    );

    let seen = captured.0.lock().unwrap();
    let chat = seen
        .iter()
        .find(|hit| hit["path"].as_str().unwrap().ends_with("/chat/completions"))
        .unwrap();
    assert_eq!(chat["body"]["model"], "real-model");
    assert_eq!(
        chat["body"]["temperature"], 0.3,
        "only-if-absent default applied"
    );
    assert_eq!(
        chat["x_trace"], "route-trace",
        "route header overrides provider"
    );
    assert_eq!(chat["authorization"], "Bearer good-key", "auth header wins");
    assert_eq!(
        chat["body"]["messages"][1]["content"], "SECRET user text",
        "user content untouched"
    );
}

#[tokio::test]
async fn route_test_probe_reports_effective_model_and_masked_headers() {
    let (upstream, _captured) = echo_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("probe.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "rw".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["alias-model".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: RewriteConfig {
                    headers: vec![HeaderRule {
                        name: "X-Trace".into(),
                        value: "super-secret-trace-value".into(),
                    }],
                    param_replace: vec![ParamReplaceRule {
                        field: "model".into(),
                        pattern: "^alias-(.+)$".into(),
                        replacement: "real-$1".into(),
                    }],
                    ..Default::default()
                },
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let route = store
        .create_route(NewRoute {
            slug: "alias-model".into(),
            name: "alias-model".into(),
            description: String::new(),
            patterns: vec!["alias-model".into()],
            provider_id: provider.id,
            model: "alias-model".into(),
            is_default: false,
            list_members: false,
            candidates: vec![(provider.id, "alias-model".into())],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;
    let response = client
        .post(format!("http://{address}/api/v1/routes/{}/test", route.id))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    let result = &body["candidates"][0]["result"];
    assert_eq!(result["ok"], true);
    assert_eq!(result["effective_model"], "real-model");
    let headers = result["headers"].as_array().unwrap();
    assert_eq!(headers[0]["name"], "X-Trace");
    assert_eq!(headers[0]["value"], "supe…", "header value masked");
}

#[tokio::test]
async fn sensitive_header_is_rejected_at_save() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("rej.db"))
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;
    let response = client
        .post(format!("http://{address}/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({"name":"bad","protocol":"openai","endpoint":"http://127.0.0.1:9/v1","models":["m"],"secret":"sk-x","headers":[{"name":"Authorization","value":"Bearer evil"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("managed by the gateway")
    );
}

#[tokio::test]
async fn auxiliary_forwarding_applies_headers_and_raw_param_rewrite() {
    let (upstream, captured) = echo_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("aux.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "rw".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["embed-alias".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: RewriteConfig {
                    headers: vec![HeaderRule {
                        name: "X-Trace".into(),
                        value: "aux-trace".into(),
                    }],
                    param_replace: vec![ParamReplaceRule {
                        field: "model".into(),
                        pattern: "^embed-(.+)$".into(),
                        replacement: "real-$1".into(),
                    }],
                    param_defaults: vec![ParamDefaultRule {
                        field: "encoding_format".into(),
                        value: json!("float"),
                    }],
                    ..Default::default()
                },
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "embed-alias".into(),
            name: "embed-alias".into(),
            description: String::new(),
            patterns: vec!["*".into()],
            provider_id: provider.id,
            model: "embed-alias".into(),
            is_default: true,
            list_members: false,
            candidates: vec![(provider.id, "embed-alias".into())],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/embeddings"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"embed-alias","input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let seen = captured.0.lock().unwrap();
    let hit = seen
        .iter()
        .find(|hit| hit["path"].as_str().unwrap().ends_with("/embeddings"))
        .unwrap();
    assert_eq!(hit["body"]["model"], "real-alias");
    assert_eq!(hit["body"]["encoding_format"], "float");
    assert_eq!(hit["x_trace"], "aux-trace");
}
