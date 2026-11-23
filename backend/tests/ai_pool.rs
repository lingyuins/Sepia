use axum::{Router, extract::State, response::IntoResponse, routing::any};
use sepia_gateway::{
    KeyStore, NewProvider, ProviderProtocol, Store,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
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

/// One chat call recorded by the mock AI service.
#[derive(Clone, Debug)]
struct ChatHit {
    #[allow(dead_code)] // recorded for failure diagnostics
    model: String,
    #[allow(dead_code)]
    content: String,
}

#[derive(Clone)]
struct MockAi {
    hits: Arc<Mutex<Vec<ChatHit>>>,
    /// Per-model delay before answering.
    delay_ms: u64,
    /// Models that always fail with HTTP 500.
    broken: Vec<String>,
    /// Echo the slug-shaped slug as every other route's name and omit the
    /// name on the rest, so tests can exercise readable-name derivation.
    spoof_names: bool,
}

async fn ai_upstream(delay_ms: u64, broken: Vec<String>) -> (SocketAddr, Arc<Mutex<Vec<ChatHit>>>) {
    ai_upstream_with(delay_ms, broken, false).await
}

async fn ai_upstream_with(
    delay_ms: u64,
    broken: Vec<String>,
    spoof_names: bool,
) -> (SocketAddr, Arc<Mutex<Vec<ChatHit>>>) {
    let mock = MockAi {
        hits: Arc::new(Mutex::new(Vec::new())),
        delay_ms,
        broken,
        spoof_names,
    };
    let handler = any(
        move |State(mock): State<MockAi>, request: axum::extract::Request| async move {
            let path = request.uri().path().to_string();
            let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            if path.ends_with("/models") {
                return axum::Json(json!({"data":[{"id":"planner-x"},{"id":"planner-y"}]}))
                    .into_response();
            }
            let model = value
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let content = value
                .pointer("/messages/1/content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            mock.hits.lock().unwrap().push(ChatHit {
                model: model.clone(),
                content: content.clone(),
            });
            if mock.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(mock.delay_ms)).await;
            }
            if mock.broken.contains(&model) {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"error":{"message":"planner offline"}})),
                )
                    .into_response();
            }
            let hallucinate = content.contains("m-hallucinated");
            // Propose one route per catalog line, echoing the pairs verbatim so
            // tests can assert which batch this service processed.
            let routes: Vec<Value> = content
                .lines()
                .filter(|line| line.starts_with("provider_id="))
                .enumerate()
                .map(|(index, line)| {
                    let mut parts = line.split_whitespace();
                    let provider_id: i64 = parts
                        .next()
                        .unwrap()
                        .trim_start_matches("provider_id=")
                        .parse()
                        .unwrap();
                    let model = parts
                        .nth(1)
                        .unwrap()
                        .trim_start_matches("model=")
                        .to_string();
                    let mut candidates = vec![json!({"provider_id": provider_id, "model": model})];
                    if hallucinate {
                        // Planner invents pairs alongside the real one.
                        candidates.push(json!({"provider_id":999,"model":"ghost-model"}));
                        candidates
                            .push(json!({"provider_id":provider_id,"model":"not-in-catalog"}));
                    }
                    let name = if mock.spoof_names && index % 2 == 1 {
                        // Suffer the missing-name fallback on every second route.
                        Value::Null
                    } else {
                        Value::String(model.clone())
                    };
                    json!({
                        "slug": model,
                        "name": name,
                        "description": "generated",
                        "patterns": [model],
                        "candidates": candidates
                    })
                })
                .collect();
            axum::Json(json!({"choices":[{"index":0,"message":{"role":"assistant","content":format!("Here you go:\n```json\n{}\n```", json!({"routes": routes}))}}]})).into_response()
        },
    );
    let address = serve(Router::new().fallback(handler).with_state(mock.clone())).await;
    (address, mock.hits)
}

// Keep the TempDir alive in the caller (Linux CI deletes SQLite files on drop).
async fn gateway(models: &[String]) -> (SocketAddr, AppState, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("ai.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://127.0.0.1:1".into(),
                models: models.to_vec(),
                plugin_id: None,
                mode: "single".into(),
            },
            "relay-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let _ = provider;
    let state = AppState::for_testing(store).await;
    let address = serve(app(state.clone())).await;
    (address, state, directory)
}

async fn login(client: &reqwest::Client, base: &str, username: &str, password: &str) -> String {
    let response = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({"username":username,"password":password}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response
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

async fn admin(base: &str, state: &AppState) -> (reqwest::Client, String) {
    let client = reqwest::Client::new();
    client
        .post(format!("{base}/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":state.bootstrap_token}))
        .send()
        .await
        .unwrap();
    let cookie = login(&client, base, "admin", "correct horse").await;
    (client, cookie)
}

fn service_body(endpoint: &str, model: &str, concurrency: i64) -> Value {
    json!({
        "name": format!("planner-{model}"),
        "endpoint": endpoint,
        "secret": "pool-key",
        "model": model,
        "models": ["planner-x", "planner-y"],
        "timeout_seconds": 10,
        "concurrency": concurrency,
        "enabled": true
    })
}

async fn enable_pool(client: &reqwest::Client, base: &str, cookie: &str) {
    let response = client
        .post(format!("{base}/api/v1/settings"))
        .header("cookie", cookie)
        .json(&json!({"ai_pool_enabled": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn pool_crud_never_exposes_secrets_and_keeps_key_on_blank_update() {
    let (address, state, _directory) = gateway(&["m-a".into()]).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(&base, &state).await;
    let (upstream, _hits) = ai_upstream(0, vec![]).await;

    let created = client
        .post(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &cookie)
        .json(&service_body(&format!("http://{upstream}"), "planner-x", 2))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200);
    let created: Value = created.json().await.unwrap();
    let id = created["id"].as_i64().unwrap();
    assert!(created.get("secret").is_none());
    assert!(created.get("nonce").is_none());
    assert!(created.get("ciphertext").is_none());

    // Blank secret on update keeps the stored key: saved-key detection works.
    let updated = client
        .put(format!("{base}/api/v1/ai/pool/{id}"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"renamed","endpoint":format!("http://{upstream}"),"secret":"",
            "model":"planner-y","models":["planner-x","planner-y"],
            "timeout_seconds":30,"concurrency":4,"enabled":true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    let updated: Value = updated.json().await.unwrap();
    assert_eq!(updated["name"], "renamed");
    assert_eq!(updated["timeout_seconds"], 30);
    assert_eq!(updated["concurrency"], 4);

    let detect = client
        .post(format!("{base}/api/v1/ai/pool/{id}/test"))
        .header("cookie", &cookie)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(detect.status(), 200);
    let detect: Value = detect.json().await.unwrap();
    assert_eq!(detect["models"], json!(["planner-x", "planner-y"]));

    // Draft detection with a fresh key hits the same catalog endpoint.
    let draft = client
        .post(format!("{base}/api/v1/ai/pool/test"))
        .header("cookie", &cookie)
        .json(&json!({"endpoint":format!("http://{upstream}"),"secret":"other-key"}))
        .send()
        .await
        .unwrap();
    assert_eq!(draft.status(), 200);

    // Validation bounds.
    let invalid = client
        .post(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"bad","endpoint":format!("http://{upstream}"),"secret":"k",
            "model":"planner-x","timeout_seconds":0,"concurrency":99
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 400);

    let deleted = client
        .delete(format!("{base}/api/v1/ai/pool/{id}"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);
    let listed: Value = client
        .get(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed["services"], json!([]));
}

#[tokio::test]
async fn generation_is_off_by_default_and_requires_a_service() {
    let (address, state, _directory) = gateway(&["m-a".into()]).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(&base, &state).await;

    // Default: pool disabled → 409, never calls any AI service.
    let response = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"group models"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);

    enable_pool(&client, &base, &cookie).await;
    // Enabled but empty pool → 409.
    let response = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"group models"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
}

#[tokio::test]
async fn pool_mutations_are_admin_only_but_generation_allows_editors() {
    let (address, state, _directory) = gateway(&["m-a".into()]).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(&base, &state).await;
    let (upstream, _hits) = ai_upstream(0, vec![]).await;
    client
        .post(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &cookie)
        .json(&service_body(&format!("http://{upstream}"), "planner-x", 1))
        .send()
        .await
        .unwrap();
    enable_pool(&client, &base, &cookie).await;

    for (username, role) in [("edit", "editor"), ("view", "viewer")] {
        let created = client
            .post(format!("{base}/api/v1/users"))
            .header("cookie", &cookie)
            .json(&json!({"username":username,"password":"{{SECRET_9f2c11aa}}","role":role}))
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 200);
    }
    let editor = login(&client, &base, "edit", "{{SECRET_9f2c11aa}}").await;
    let viewer = login(&client, &base, "view", "{{SECRET_9f2c11aa}}").await;

    // Pool management: admin only (valid bodies, so 403 is the role check).
    let response = client
        .post(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &editor)
        .json(&service_body(&format!("http://{upstream}"), "planner-y", 1))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    let response = client
        .post(format!("{base}/api/v1/ai/pool/test"))
        .header("cookie", &viewer)
        .json(&json!({"endpoint":format!("http://{upstream}"),"secret":"k"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);

    // Generation: editor allowed, viewer read-only.
    let response = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &editor)
        .json(&json!({"instruction":"group models"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &viewer)
        .json(&json!({"instruction":"group models"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn generation_dispatches_batches_dynamically_and_retries_failures() {
    let models: Vec<String> = (0..6).map(|index| format!("m-{index}")).collect();
    let (address, state, _directory) = gateway(&models).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(&base, &state).await;
    enable_pool(&client, &base, &cookie).await;

    // Fast healthy service + slow broken service. Broken batches must be
    // retried on the healthy one instead of failing the whole generation.
    let (fast, fast_hits) = ai_upstream(0, vec![]).await;
    let (slow, slow_hits) = ai_upstream(400, vec!["planner-slow".into()]).await;
    for (upstream, model) in [(fast, "planner-fast"), (slow, "planner-slow")] {
        let response = client
            .post(format!("{base}/api/v1/ai/pool"))
            .header("cookie", &cookie)
            .json(&service_body(&format!("http://{upstream}"), model, 2))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    let response = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"one route per model","batch_size":1}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["batches"], 6);
    assert_eq!(result["services"], 2);
    assert_eq!(result["errors"], json!([]));
    let routes = result["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 6);
    for route in routes {
        assert_eq!(route["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(route["candidates"][0]["provider"], "relay");
    }

    // Dynamic allocation: every batch must reach the service it was assigned
    // to (hits record the service's own model), the fast service handled
    // strictly more batches, and every model was generated exactly once. The
    // slow service's two failures were retried on the fast one, so it saw at
    // most its initial two batches.
    let fast_hits = fast_hits.lock().unwrap().clone();
    let slow_hits = slow_hits.lock().unwrap().clone();
    assert!(
        fast_hits.iter().all(|hit| hit.model == "planner-fast"),
        "fast service received another service's batches: {fast_hits:?}"
    );
    assert!(
        slow_hits.iter().all(|hit| hit.model == "planner-slow"),
        "slow service received another service's batches: {slow_hits:?}"
    );
    let fast_count = fast_hits.len();
    let slow_count = slow_hits.len();
    assert!(fast_count >= 6, "fast={fast_count}");
    assert!(slow_count <= 2, "slow={slow_count}");
    let mut generated: Vec<String> = routes
        .iter()
        .map(|route| route["slug"].as_str().unwrap().to_string())
        .collect();
    generated.sort();
    assert_eq!(generated, models);

    // Proposals can now be persisted through the normal route API.
    let first = &routes[0];
    let candidates: Vec<(i64, String)> = first["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| {
            (
                candidate["provider_id"].as_i64().unwrap(),
                candidate["model"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let created = client
        .post(format!("{base}/api/v1/routes"))
        .header("cookie", &cookie)
        .json(&json!({
            "slug": first["slug"], "name": first["name"], "description": first["description"],
            "patterns": first["patterns"], "candidates": candidates, "enabled": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200);

    // Regeneration skips slugs that already exist.
    let again: Value = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"one route per model","batch_size":1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(again["routes"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn generated_routes_replace_slug_shaped_and_missing_names() {
    // The planner echoes the slug as the name on every other route and omits
    // the name on the rest; sanitization must return readable display names
    // while the client-facing slug stays lowercase kebab-case.
    let models: Vec<String> = ["qwen4-max", "gpt-4o", "claude-sonnet-4", "llama-3"]
        .map(str::to_string)
        .to_vec();
    let (address, state, _directory) = gateway(&models).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(&base, &state).await;
    enable_pool(&client, &base, &cookie).await;
    let (upstream, _hits) = ai_upstream_with(0, vec![], true).await;
    let response = client
        .post(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &cookie)
        .json(&service_body(
            &format!("http://{upstream}"),
            "planner-names",
            1,
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let result: Value = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"name the routes","batch_size":10}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["errors"], json!([]));
    let mut pairs: Vec<(&str, &str)> = result["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|route| {
            (
                route["slug"].as_str().unwrap(),
                route["name"].as_str().unwrap(),
            )
        })
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("claude-sonnet-4", "Claude Sonnet 4"),
            ("gpt-4o", "Gpt 4o"),
            ("llama-3", "Llama 3"),
            ("qwen4-max", "Qwen4 Max"),
        ]
    );
}

#[tokio::test]
async fn generation_drops_invented_candidates_and_reports_dead_services() {
    let (address, state, _directory) = gateway(&["m-a".into(), "m-hallucinated".into()]).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(&base, &state).await;
    enable_pool(&client, &base, &cookie).await;
    let (upstream, _hits) = ai_upstream(0, vec!["planner-x".into()]).await;
    client
        .post(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &cookie)
        .json(&service_body(&format!("http://{upstream}"), "planner-y", 1))
        .send()
        .await
        .unwrap();

    // The planner adds invented provider/model pairs next to real ones; only
    // catalog-backed candidates survive sanitization.
    let result: Value = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"group","batch_size":1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let routes = result["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 2);
    for route in routes {
        let candidates = route["candidates"].as_array().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0]["provider_id"], 1);
        assert_eq!(candidates[0]["model"], route["slug"]);
    }

    // Now break the only service: each batch fails twice, errors are reported.
    let broken = client
        .put(format!("{base}/api/v1/ai/pool/1"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"broken","endpoint":format!("http://{upstream}"),"secret":"",
            "model":"planner-x","models":[],"timeout_seconds":10,"concurrency":1,"enabled":true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(broken.status(), 200);
    let result: Value = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"group","batch_size":1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["routes"], json!([]));
    let errors = result["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    assert!(errors[0].as_str().unwrap().contains("planner offline"));
}

#[tokio::test]
async fn provider_backed_pool_service_borrows_provider_credentials() {
    let (upstream, hits) = ai_upstream(0, vec![]).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("ai_provider.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["m-a".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "provider-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let state = AppState::for_testing(store).await;
    let address = serve(app(state.clone())).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(&base, &state).await;
    enable_pool(&client, &base, &cookie).await;
    // Provider-backed service: no endpoint or secret in the body.
    let created = client
        .post(format!("{base}/api/v1/ai/pool"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"from-provider","provider_id":provider.id,"model":"planner-x",
            "models":["planner-x"],"timeout_seconds":10,"concurrency":1,"enabled":true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        created.status(),
        200,
        "{}",
        created.text().await.unwrap_or_default()
    );
    let created: Value = created.json().await.unwrap();
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["provider_id"], json!(provider.id));
    assert_eq!(created["endpoint"], format!("http://{upstream}"));
    assert!(created.get("secret").is_none());
    // Saved-service detection borrows the provider's first key.
    let detect = client
        .post(format!("{base}/api/v1/ai/pool/{id}/test"))
        .header("cookie", &cookie)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(detect.status(), 200);
    let detect: Value = detect.json().await.unwrap();
    assert_eq!(detect["models"], json!(["planner-x", "planner-y"]));
    // Draft detection by provider id also borrows the key.
    let draft = client
        .post(format!("{base}/api/v1/ai/pool/test"))
        .header("cookie", &cookie)
        .json(&json!({"provider_id":provider.id}))
        .send()
        .await
        .unwrap();
    assert_eq!(draft.status(), 200);
    // Generation dispatches through the provider's endpoint and key.
    let result: Value = client
        .post(format!("{base}/api/v1/ai/routes/generate"))
        .header("cookie", &cookie)
        .json(&json!({"instruction":"group","batch_size":1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["routes"].as_array().unwrap().len(), 1);
    assert!(!hits.lock().unwrap().is_empty());
}
