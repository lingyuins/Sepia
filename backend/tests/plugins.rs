//! End-to-end plugin runtime tests: package install, hook rewriting, route
//! filtering, provider plugins, host capabilities, RPC and lifecycle. The wasm
//! fixture is built from backend/plugin-examples (wasm32-unknown-unknown).
use axum::{Router, extract::State, response::IntoResponse, routing::any};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{
    io::Write,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

const WASM: &[u8] = include_bytes!("fixtures/echo.wasm");

fn manifest_json() -> Value {
    json!({
        "id": "echo",
        "name": "Echo",
        "version": "1.0.0",
        "description": "reference plugin",
        "events": [
            "relay.request.before_route", "relay.route.filter", "relay.strategy",
            "relay.request.before_upstream", "relay.response.before_send",
            "relay.completed", "provider.fetch", "provider.models",
            "http.rpc", "plugin.migrate"
        ],
        "permissions": ["kv", "secret", "http", "log", "relay:read", "relay:write", "provider", "rpc:read", "rpc:write"],
        "http_origins": [],
        "priority": 0,
        "timeout_ms": 2000,
        "memory_mib": 16,
        "config_schema": {"type": "object"},
        "ui": {"entry": "index.html", "title": "Echo UI"}
    })
}

fn package_bytes(manifest: &Value, ui: Option<&str>) -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(manifest.to_string().as_bytes()).unwrap();
    zip.start_file("plugin.wasm", options).unwrap();
    zip.write_all(WASM).unwrap();
    if let Some(ui) = ui {
        zip.start_file("ui/index.html", options).unwrap();
        zip.write_all(ui.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

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

#[derive(Clone, Default)]
struct Hits(Arc<Mutex<Vec<(String, Value)>>>);

async fn chat_upstream() -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let handler = any(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let header = request
                .headers()
                .get("x-sepia-plugin")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            hits.0.lock().unwrap().push((header, value.clone()));
            let model = value.get("model").and_then(Value::as_str).unwrap_or("");
            axum::Json(json!({"id":"chat_1","model":model,"choices":[{"index":0,"message":{"role":"assistant","content":"pong"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})).into_response()
        },
    );
    let address = serve(Router::new().fallback(handler).with_state(hits.clone())).await;
    (address, hits)
}

async fn admin(address: SocketAddr, state: &AppState) -> (reqwest::Client, String) {
    let client = reqwest::Client::new();
    let url = format!("http://{address}");
    let state = state.clone();
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

async fn install(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    bytes: Vec<u8>,
) -> reqwest::Response {
    let part = reqwest::multipart::Part::bytes(bytes).file_name("plugin.zip");
    let form = reqwest::multipart::Form::new().part("package", part);
    client
        .post(format!("{base}/api/v1/plugins/install"))
        .header("cookie", cookie)
        .multipart(form)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn plugin_install_enable_hooks_and_lifecycle() {
    let (upstream, hits) = chat_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("plugins.db"))
        .await
        .unwrap();
    let provider = store
        .create_provider(
            NewProvider {
                name: "up".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "gpt-key",
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
            patterns: vec!["gpt-a".into()],
            provider_id: provider.id,
            model: "gpt-a".into(),
            is_default: true,
            list_members: true,
            candidates: vec![(provider.id, "gpt-a".into())],
        })
        .await
        .unwrap();
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
            name: "k".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap();

    let state = AppState::for_testing(store.clone()).await;
    let address = serve(app(state.clone())).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(address, &state).await;

    // Install (disabled by default) then enable.
    let response = install(
        &client,
        &base,
        &cookie,
        package_bytes(
            &manifest_json(),
            Some("<html><head></head><body>echo</body></html>"),
        ),
    )
    .await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let listed: Value = client
        .get(format!("{base}/api/v1/plugins"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed[0]["id"], "echo");
    assert_eq!(listed[0]["enabled"], false);
    assert_eq!(listed[0]["has_ui"], true);
    assert!(listed[0].get("wasm").is_none());

    // Configure model rewrite + enable.
    client
        .put(format!("{base}/api/v1/plugins/echo/config"))
        .header("cookie", &cookie)
        .json(&json!({"model": "gpt-a"}))
        .send()
        .await
        .unwrap();
    let enable = client
        .post(format!("{base}/api/v1/plugins/echo/enabled"))
        .header("cookie", &cookie)
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(enable.status(), 200);

    // Inference: request rewrite + upstream header + response rewrite.
    let chat = client
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&key.plaintext)
        .json(&json!({"model":"gpt-a","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(chat.status(), 200, "{}", chat.text().await.unwrap());
    let body: Value = chat.json().await.unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "pong [plugin]");
    let recorded = hits.0.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].0, "echo",
        "upstream rewrite header must reach upstream"
    );

    // relay.completed incremented the kv counter (fire-and-forget).
    let mut count = "0".to_string();
    for _ in 0..50 {
        let rpc: Value = client
            .post(format!("{base}/api/v1/plugins/echo/rpc/count"))
            .header("cookie", &cookie)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        count = rpc["completions"]
            .as_str()
            .unwrap_or("0")
            .trim_matches('"')
            .to_string();
        if count == "1" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(count, "1", "relay.completed must increment kv counter");

    // RPC echo + secret capability.
    let echo: Value = client
        .post(format!("{base}/api/v1/plugins/echo/rpc/anything"))
        .header("cookie", &cookie)
        .json(&json!({"hello": "world"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(echo["echo"]["hello"], "world");
    let stored = client
        .post(format!("{base}/api/v1/plugins/echo/rpc/store-secret"))
        .header("cookie", &cookie)
        .json(&json!({"value": "s3cret"}))
        .send()
        .await
        .unwrap();
    assert_eq!(stored.status(), 200);

    // UI bundle is sandboxed and carries a session.
    let ui: Value = client
        .get(format!("{base}/api/v1/plugins/echo/ui-bundle"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(ui["session"].as_str().unwrap().starts_with("sepia_plugin_"));
    assert!(
        ui["document"]
            .as_str()
            .unwrap()
            .contains("Content-Security-Policy")
    );
    assert!(
        ui["document"]
            .as_str()
            .unwrap()
            .contains("__SEPIA_SESSION__")
    );
    assert!(
        ui["document"]
            .as_str()
            .unwrap()
            .contains("<body>echo</body>")
    );

    // Downgrade rejected.
    let mut old = manifest_json();
    old["version"] = json!("0.9.0");
    let downgrade = install(&client, &base, &cookie, package_bytes(&old, None)).await;
    assert_eq!(downgrade.status(), 400);

    // Disable then uninstall.
    client
        .post(format!("{base}/api/v1/plugins/echo/enabled"))
        .header("cookie", &cookie)
        .json(&json!({"enabled": false}))
        .send()
        .await
        .unwrap();
    let deleted = client
        .delete(format!("{base}/api/v1/plugins/echo"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);
    let listed: Value = client
        .get(format!("{base}/api/v1/plugins"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn route_filter_excludes_candidates() {
    let (upstream, hits) = chat_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("filter.db"))
        .await
        .unwrap();
    let excluded = store
        .create_provider(
            NewProvider {
                name: "bad".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "gpt-key",
            &[],
            &KeyStore::testing(),
        )
        .await
        .unwrap();
    let good = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "gpt-key",
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
            patterns: vec!["gpt-a".into()],
            provider_id: good.id,
            model: "gpt-a".into(),
            is_default: true,
            list_members: true,
            candidates: vec![(excluded.id, "gpt-a".into()), (good.id, "gpt-a".into())],
        })
        .await
        .unwrap();
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
            name: "k".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
    let address = serve(app(state.clone())).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(address, &state).await;
    install(
        &client,
        &base,
        &cookie,
        package_bytes(&manifest_json(), Some("<html>echo</html>")),
    )
    .await;
    // Exclude the first provider id via config.
    client
        .put(format!("{base}/api/v1/plugins/echo/config"))
        .header("cookie", &cookie)
        .json(&json!({"exclude": [excluded.id]}))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{base}/api/v1/plugins/echo/enabled"))
        .header("cookie", &cookie)
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    let chat = client
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&key.plaintext)
        .json(&json!({"model":"gpt-a","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(chat.status(), 200);
    // Only the good provider was contacted (one upstream call).
    assert_eq!(hits.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn plugin_backed_provider_discovers_and_serves() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("provider.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
    let address = serve(app(state.clone())).await;
    let base = format!("http://{address}");
    let (client, cookie) = admin(address, &state).await;
    let installed = install(
        &client,
        &base,
        &cookie,
        package_bytes(&manifest_json(), Some("<html>echo</html>")),
    )
    .await;
    assert_eq!(
        installed.status(),
        200,
        "{}",
        installed.text().await.unwrap()
    );
    client
        .post(format!("{base}/api/v1/plugins/echo/enabled"))
        .header("cookie", &cookie)
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    // Create a plugin-backed provider; discovery delegates to provider.models.
    let created = client
        .post(format!("{base}/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({"name":"plugin-prov","protocol":"openai","endpoint":"plugin://echo","secret":"x","models":[],"plugin_id":"echo"}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200, "{}", created.text().await.unwrap());
    let provider: Value = created.json().await.unwrap();
    let provider_id = provider["id"].as_i64().unwrap();
    let listed: Value = client
        .get(format!("{base}/api/v1/providers"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let found = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"].as_i64() == Some(provider_id))
        .unwrap();
    assert_eq!(found["models"], json!(["plugin-chat"]));
    let route = store
        .create_route(NewRoute {
            slug: "p".into(),
            name: "p".into(),
            description: String::new(),
            patterns: vec!["plugin-chat".into()],
            provider_id,
            model: "plugin-chat".into(),
            is_default: true,
            list_members: true,
            candidates: vec![(provider_id, "plugin-chat".into())],
        })
        .await
        .unwrap();
    store.set_route_enabled(route.id, true).await.unwrap();
    state.snapshots.refresh(&store).await.unwrap();
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
            name: "k".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap();
    // Inference goes through provider.fetch (synthetic response), then the
    // response-rewrite hook appends the plugin marker.
    let chat = client
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&key.plaintext)
        .json(&json!({"model":"plugin-chat","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(chat.status(), 200, "{}", chat.text().await.unwrap());
    let body: Value = chat.json().await.unwrap();
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "plugin pong [plugin]"
    );
}

#[tokio::test]
async fn anonymous_plugin_routes_are_protected() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("auth.db"))
        .await
        .unwrap();
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{address}/api/v1/plugins"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}
