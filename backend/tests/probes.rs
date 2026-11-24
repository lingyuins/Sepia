use axum::{Router, extract::State, response::IntoResponse, routing::any};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Hits(Arc<Mutex<Vec<String>>>);

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

// Chat upstream: gpt-key serves gpt-a; every other key is rejected. The
// /responses endpoint does not exist, exercising the dual-endpoint fallback.
async fn chat_upstream() -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let handler = any(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let path = request.uri().path().to_string();
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let value: Value = if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&body).unwrap()
            };
            hits.0.lock().unwrap().push(path.clone());
            if path.ends_with("/responses") {
                return axum::http::StatusCode::NOT_FOUND.into_response();
            }
            if secret != "Bearer gpt-key" {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    axum::Json(json!({"error":{"message":"bad key"}})),
                )
                    .into_response();
            }
            if path.ends_with("/models") {
                return axum::Json(json!({"data":[{"id":"gpt-a"}]})).into_response();
            }
            let model = value.get("model").and_then(Value::as_str).unwrap_or("");
            axum::Json(json!({"id":"chat_1","model":model,"choices":[{"index":0,"message":{"role":"assistant","content":"probe pong"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})).into_response()
        },
    );
    let address = serve(Router::new().fallback(handler).with_state(hits.clone())).await;
    (address, hits)
}

// Responses-only upstream: chat/completions is missing, /responses answers.
async fn responses_only_upstream() -> SocketAddr {
    let handler = any(move |request: axum::extract::Request| async move {
        let path = request.uri().path().to_string();
        if path.ends_with("/chat/completions") {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
        axum::Json(json!({"id":"resp_1","model":"gpt-a","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"responses pong"}]}],"usage":{"input_tokens":2,"output_tokens":1}})).into_response()
    });
    serve(Router::new().fallback(handler)).await
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

#[tokio::test]
async fn route_test_probes_every_candidate_with_real_upstream_calls() {
    let (upstream, hits) = chat_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("probe.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let good = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "gpt-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let bad = store
        .create_provider(
            NewProvider {
                name: "bad".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "revoked-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let route = store
        .create_route(NewRoute {
            slug: "probe-route".into(),
            name: "probe-route".into(),
            description: String::new(),
            patterns: vec!["gpt-a".into()],
            provider_id: good.id,
            model: "gpt-a".into(),
            is_default: false,
            list_members: true,
            candidates: vec![(good.id, "gpt-a".into()), (bad.id, "gpt-a".into())],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;
    let url = |path: &str| format!("http://{address}{path}");

    let response = client
        .post(url(&format!("/api/v1/routes/{}/test", route.id)))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["route"], "probe-route");
    let candidates = body["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0]["provider"], "good");
    assert_eq!(candidates[0]["result"]["ok"], true);
    assert_eq!(candidates[0]["result"]["reply"], "probe pong");
    assert_eq!(candidates[0]["result"]["endpoint"], "chat/completions");
    assert!(candidates[0]["result"]["latency_ms"].as_i64().unwrap() >= 0);
    assert_eq!(candidates[1]["provider"], "bad");
    assert_eq!(candidates[1]["result"]["ok"], false);
    let error = candidates[1]["result"]["error"].as_str().unwrap();
    assert!(error.contains("401"), "{error}");
    assert!(error.contains("bad key"), "{error}");
    // Both candidates were really probed on the chat leg. The revoked key
    // fails with a real 401 answer; it does not burn the responses leg.
    let paths = hits.0.lock().unwrap().clone();
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.ends_with("/chat/completions"))
            .count(),
        2
    );
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.ends_with("/responses"))
            .count(),
        0
    );

    // Probes never appear in the request log.
    let requests: Value = client
        .get(url("/api/v1/requests"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(requests.as_array().unwrap().len(), 0);

    // A disabled provider is reported without touching the upstream.
    store.set_provider_enabled(bad.id, false).await.unwrap();
    let before = hits.0.lock().unwrap().len();
    let body: Value = client
        .post(url(&format!("/api/v1/routes/{}/test", route.id)))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let disabled = &body["candidates"][1];
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["result"]["ok"], false);
    assert_eq!(disabled["result"]["error"], "provider disabled");
    assert_eq!(hits.0.lock().unwrap().len(), before + 1); // only the good provider probed again
}

#[tokio::test]
async fn route_test_reports_missing_key_scope_and_universal_fallback() {
    let (upstream, _) = chat_upstream().await;
    let responses = responses_only_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("scope.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let scoped = store
        .create_provider(
            NewProvider {
                name: "scoped".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["gpt-a".into(), "other".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "gpt-key",
            &[("gpt-key".into(), vec!["gpt-a".into()])],
            &keys,
        )
        .await
        .unwrap();
    let universal = store
        .create_provider(
            NewProvider {
                name: "universal".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{responses}"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "gpt-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let route = store
        .create_route(NewRoute {
            slug: "mixed".into(),
            name: "mixed".into(),
            description: String::new(),
            patterns: vec!["*".into()],
            provider_id: scoped.id,
            model: "other".into(),
            is_default: false,
            list_members: true,
            candidates: vec![(scoped.id, "other".into()), (universal.id, "gpt-a".into())],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;
    let body: Value = client
        .post(format!("http://{address}/api/v1/routes/{}/test", route.id))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let candidates = body["candidates"].as_array().unwrap();
    assert_eq!(candidates[0]["result"]["ok"], false);
    assert_eq!(candidates[0]["result"]["error"], "no key serves this model");
    // The universal provider's chat leg 404s; the responses leg succeeds.
    assert_eq!(candidates[1]["result"]["ok"], true);
    assert_eq!(candidates[1]["result"]["endpoint"], "responses");
    assert_eq!(candidates[1]["result"]["reply"], "responses pong");
}

#[tokio::test]
async fn provider_test_detects_catalog_with_saved_key_index_or_entered_secret() {
    let (upstream, hits) = chat_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("provider.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "revoked-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "gpt-key", None, None, &keys)
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;
    let url = |path: &str| format!("http://{address}{path}");

    // Default first saved key is revoked: catalog detection fails with 401.
    let response = client
        .post(url(&format!("/api/v1/providers/{}/test", provider.id)))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);

    // key_index selects the working saved key without any plaintext.
    let body: Value = client
        .post(url(&format!("/api/v1/providers/{}/test", provider.id)))
        .header("cookie", &cookie)
        .json(&json!({"key_index":1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["models"], json!(["gpt-a"]));

    // An entered secret overrides the saved key.
    let body: Value = client
        .post(url(&format!("/api/v1/providers/{}/test", provider.id)))
        .header("cookie", &cookie)
        .json(&json!({"secret":"gpt-key"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["models"], json!(["gpt-a"]));

    // Unsaved provider preview detects models without persisting anything.
    let body: Value = client
        .post(url("/api/v1/providers/test"))
        .header("cookie", &cookie)
        .json(&json!({"protocol":"openai","endpoint":format!("http://{upstream}"),"secret":"gpt-key"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["models"], json!(["gpt-a"]));
    let providers: Value = client
        .get(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(providers.as_array().unwrap().len(), 1);
    // Catalog detection only lists models; it never sends an inference ping.
    let paths = hits.0.lock().unwrap().clone();
    assert!(paths.iter().all(|path| path.ends_with("/models")));
}

// Reasoning upstream: spends its whole output budget on hidden reasoning when
// the budget is tiny, answering with empty visible content. A probe must give
// the model room to finish thinking instead of reporting an empty reply.
#[tokio::test]
async fn probe_budget_lets_reasoning_models_emit_visible_content() {
    let handler = any(move |request: axum::extract::Request| async move {
        let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        let budget = value
            .get("max_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(4096);
        let message = if budget < 64 {
            json!({"role":"assistant","content":"","reasoning_content":"thinking hard about ping"})
        } else {
            json!({"role":"assistant","content":"probe pong","reasoning_content":"thought about ping"})
        };
        let finish = if budget < 64 { "length" } else { "stop" };
        axum::Json(json!({"id":"chat_r","model":value.get("model").and_then(Value::as_str).unwrap_or(""),"choices":[{"index":0,"message":message,"finish_reason":finish}],"usage":{"prompt_tokens":2,"completion_tokens":budget.min(16),"completion_tokens_details":{"reasoning_tokens":budget.min(16)}}}))
            .into_response()
    });
    let upstream = serve(Router::new().fallback(handler)).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("reasoning.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "thinker".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["qwen-max".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "gpt-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let route = store
        .create_route(NewRoute {
            slug: "think-route".into(),
            name: "think-route".into(),
            description: String::new(),
            patterns: vec!["qwen-max".into()],
            provider_id: provider.id,
            model: "qwen-max".into(),
            is_default: false,
            list_members: true,
            candidates: vec![(provider.id, "qwen-max".into())],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;
    let body: Value = client
        .post(format!("http://{address}/api/v1/routes/{}/test", route.id))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["candidates"][0]["result"]["ok"], true);
    assert_eq!(body["candidates"][0]["result"]["reply"], "probe pong");
}
