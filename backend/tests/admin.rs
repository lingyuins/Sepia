use axum::Json;
use sepia_gateway::{
    KeyStore, Store,
    server::{AppState, app},
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

#[tokio::test]
async fn management_requires_session_and_client_secret_is_returned_once() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("admin.db"))
        .await
        .unwrap();
    let state = AppState {
        store: store.clone(),
        keys: KeyStore::testing(),
        http: std::sync::Arc::new(std::sync::Mutex::new((10, reqwest::Client::new()))),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.path().to_path_buf(),
        frontend_directory: "frontend/dist".into(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        routing_stats: sepia_gateway::strategy::RoutingStats::shared(),
        secure_cookie: false,
        trusted_proxy: false,
        metadata_source: sepia_gateway::metadata::DEFAULT_SOURCE.into(),
        plugins: sepia_gateway::plugin::shared(
            sepia_gateway::plugin::PluginRuntime::new().unwrap(),
        ),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    let denied = client
        .post(url("/api/v1/providers"))
        .json(&json!({"name":"private"}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    let status = client
        .get(url("/api/v1/auth/status"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(status["initialized"], false);
    let invalid_token = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"invalid"}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_token.status(), 400);
    let invalid_body: Value = invalid_token.json().await.unwrap();
    assert_eq!(invalid_body["error"]["message"], "invalid bootstrap token");
    let initialized = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(initialized.status(), 200);
    let repeated = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"replacement","password":"replacement password","token":"invalid after initialization"}))
        .send()
        .await
        .unwrap();
    assert_eq!(repeated.status(), 200);
    let repeated_body: Value = repeated.json().await.unwrap();
    assert_eq!(repeated_body["initialized"], true);
    let replaced_login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"replacement","password":"replacement password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(replaced_login.status(), 401);
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let login_cookie = login.headers().get("set-cookie").unwrap().to_str().unwrap();
    assert!(!login_cookie.split(';').any(|part| part.trim() == "Secure"));
    let cookie = login_cookie.split(';').next().unwrap().to_string();
    let created_provider = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"Protocol update",
            "protocol":"openai",
            "endpoint":"http://127.0.0.1:9000/v1",
            "models":["model"],
            "secret":"upstream-secret",
            "group_id":"official",
            "enabled":true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created_provider.status(), 200);
    let created_provider: Value = created_provider.json().await.unwrap();
    let provider_id = created_provider["id"].as_i64().unwrap();
    let updated_provider = client
        .put(url(&format!("/api/v1/providers/{provider_id}")))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"Protocol update",
            "protocol":"anthropic",
            "endpoint":"http://127.0.0.1:9000/v1",
            "models":["model"],
            "secret":"upstream-secret",
            "group_id":"official",
            "enabled":true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(updated_provider.status(), 200);
    let providers: Value = client
        .get(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(providers[0]["protocol"], "anthropic");
    let created = client.post(url("/api/v1/keys")).header("cookie", &cookie).json(&json!({"name":"client","models":["*"],"token_limit":0,"max_amount_micros":0,"rpm":0,"tpm":0})).send().await.unwrap();
    assert_eq!(created.status(), 200);
    let body: Value = created.json().await.unwrap();
    assert!(body["plaintext"].as_str().unwrap().starts_with("sk-sepia-"));
    let list = client
        .get(url("/api/v1/keys"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert!(list[0].get("plaintext").is_none());
    assert!(list[0].get("token_hash").is_none());
    let revoked = client
        .post(url(&format!("/api/v1/keys/{}/revoke", body["id"])))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(revoked.status(), 200);
    assert!(
        store
            .authenticate_client(body["plaintext"].as_str().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    let requests = client
        .get(url("/api/v1/requests"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(requests.status(), 200);
}

#[tokio::test]
async fn secure_cookie_mode_marks_session_cookie_secure() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("secure-cookie.db"))
        .await
        .unwrap();
    let mut state = AppState::for_testing(store).await;
    state.bootstrap_token = "bootstrap".into();
    state.secure_cookie = true;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    let initialized = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(initialized.status(), 200);
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let cookie = login.headers().get("set-cookie").unwrap().to_str().unwrap();
    assert!(cookie.split(';').any(|part| part.trim() == "Secure"));
}

#[tokio::test]
async fn provider_creation_discovers_models_before_persisting() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("discovery-create.db"))
        .await
        .unwrap();
    store
        .bootstrap("admin", "correct horse", "bootstrap", "bootstrap")
        .await
        .unwrap();
    let (session, _) = store.login("admin", "correct horse").await.unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}/v1", upstream.local_addr().unwrap());
    let request_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let upstream_count = request_count.clone();
    tokio::spawn(
        axum::serve(
            upstream,
            axum::Router::new().route(
                "/v1/models",
                axum::routing::get(move |headers: axum::http::HeaderMap| {
                    let count = upstream_count.clone();
                    async move {
                        let stage = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let expected = if stage == 2 {
                            "Bearer replacement-secret"
                        } else {
                            "Bearer initial-secret"
                        };
                        if headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            != Some(expected)
                        {
                            return (
                                axum::http::StatusCode::UNAUTHORIZED,
                                Json(json!({"error":"wrong credential"})),
                            );
                        }
                        (
                            axum::http::StatusCode::OK,
                            Json(json!({"data":[{"id":"discovered-model"}]})),
                        )
                    }
                }),
            ),
        )
        .into_future(),
    );
    let state = AppState {
        store: store.clone(),
        keys: KeyStore::testing(),
        http: std::sync::Arc::new(std::sync::Mutex::new((10, reqwest::Client::new()))),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.path().to_path_buf(),
        frontend_directory: "frontend/dist".into(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        routing_stats: sepia_gateway::strategy::RoutingStats::shared(),
        secure_cookie: false,
        trusted_proxy: false,
        metadata_source: sepia_gateway::metadata::DEFAULT_SOURCE.into(),
        plugins: sepia_gateway::plugin::shared(
            sepia_gateway::plugin::PluginRuntime::new().unwrap(),
        ),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let client = reqwest::Client::new();
    let preview = client
        .post(format!("http://{address}/api/v1/providers/test"))
        .header("cookie", format!("sepia_session={session}"))
        .json(&json!({"protocol":"openai","endpoint":upstream_url,"secret":"initial-secret"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        preview.status(),
        200,
        "preview: {}",
        preview.text().await.unwrap()
    );
    assert!(store.providers().await.unwrap().is_empty());
    let created = client.post(format!("http://{address}/api/v1/providers"))
        .header("cookie", format!("sepia_session={session}"))
        .json(&json!({"name":"Discovered","protocol":"openai","endpoint":upstream_url,"models":[],"secret":"initial-secret","secrets":["initial-secret"],"group_id":"official","enabled":true}))
        .send().await.unwrap();
    assert_eq!(
        created.status(),
        200,
        "create: {}",
        created.text().await.unwrap()
    );
    let providers = store.providers().await.unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].models, vec!["discovered-model"]);
    let retested = client
        .post(format!(
            "http://{address}/api/v1/providers/{}/test",
            providers[0].id
        ))
        .header("cookie", format!("sepia_session={session}"))
        .json(&json!({"endpoint":upstream_url,"secret":"replacement-secret"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        retested.status(),
        200,
        "retest: {}",
        retested.text().await.unwrap()
    );
    let models: Value = retested.json().await.unwrap();
    assert_eq!(models["models"][0], "discovered-model");
    let fallback = client
        .post(format!(
            "http://{address}/api/v1/providers/{}/test",
            providers[0].id
        ))
        .header("cookie", format!("sepia_session={session}"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        fallback.status(),
        200,
        "fallback: {}",
        fallback.text().await.unwrap()
    );
}

#[tokio::test]
async fn model_normalize_settings_round_trip_and_unknown_key_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("normalize.db"))
        .await
        .unwrap();
    let state = AppState {
        store: store.clone(),
        keys: KeyStore::testing(),
        http: std::sync::Arc::new(std::sync::Mutex::new((10, reqwest::Client::new()))),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.path().to_path_buf(),
        frontend_directory: "frontend/dist".into(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        routing_stats: sepia_gateway::strategy::RoutingStats::shared(),
        secure_cookie: false,
        trusted_proxy: false,
        metadata_source: sepia_gateway::metadata::DEFAULT_SOURCE.into(),
        plugins: sepia_gateway::plugin::shared(
            sepia_gateway::plugin::PluginRuntime::new().unwrap(),
        ),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    let password = "normalize test password";
    let bootstrap = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":password,"token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), 200);
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":password}))
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
    let rules = json!({
        "model_normalize_router_prefixes": ["dmxapi-", "[官B]-"],
        "model_normalize_functional_suffixes": ["-cc", "-\\d{8}"],
        "model_normalize_explicit_mappings": [{"variant":"claude-opus-4-6","canonical":"claude-opus-4.6"}],
    });
    let saved = client
        .post(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&rules)
        .send()
        .await
        .unwrap();
    assert_eq!(saved.status(), 200, "save: {}", saved.text().await.unwrap());
    let loaded: Value = client
        .get(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for (key, value) in rules.as_object().unwrap() {
        assert_eq!(&loaded[key], value, "setting {key} did not round-trip");
    }
    // Clearing a list persists an empty array (frontend falls back to builtins).
    let cleared = client
        .post(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&json!({"model_normalize_router_prefixes": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), 200);
    let loaded: Value = client
        .get(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(loaded["model_normalize_router_prefixes"], json!([]));
    let rejected = client
        .post(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&json!({"model_normalize_unknown": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 400);
}

#[tokio::test]
async fn workspace_and_gateway_address_settings_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("gateway-settings.db"))
        .await
        .unwrap();
    let state = AppState {
        store: store.clone(),
        keys: KeyStore::testing(),
        http: std::sync::Arc::new(std::sync::Mutex::new((10, reqwest::Client::new()))),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.path().to_path_buf(),
        frontend_directory: "frontend/dist".into(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        routing_stats: sepia_gateway::strategy::RoutingStats::shared(),
        secure_cookie: false,
        trusted_proxy: false,
        metadata_source: sepia_gateway::metadata::DEFAULT_SOURCE.into(),
        plugins: sepia_gateway::plugin::shared(
            sepia_gateway::plugin::PluginRuntime::new().unwrap(),
        ),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    let password = "gateway settings password";
    let bootstrap = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"root","password":password,"token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), 200);
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"root","password":password}))
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
    let settings = json!({
        "workspace_name": "Team console",
        "gateway_deployment": "cloud",
        "gateway_protocol": "https",
        "gateway_host": "ai.example.com",
        "gateway_port": "8443",
    });
    let saved = client
        .post(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert_eq!(saved.status(), 200, "save: {}", saved.text().await.unwrap());
    let loaded: Value = client
        .get(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for (key, value) in settings.as_object().unwrap() {
        assert_eq!(&loaded[key], value, "setting {key} did not round-trip");
    }
}
