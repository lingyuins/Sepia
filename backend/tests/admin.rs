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
        events: Default::default(),
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
        events: Default::default(),
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
        events: Default::default(),
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
        events: Default::default(),
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

#[tokio::test]
async fn delete_all_routes_is_admin_only_and_clears_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("bulk-routes.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
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
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"test"}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
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
    let provider: Value = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({"name":"Bulk","protocol":"openai","endpoint":"http://127.0.0.1:9/v1","models":["m"],"secret":"upstream-secret","group_id":"official","enabled":true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let provider_id = provider["id"].as_i64().unwrap();
    for slug in ["bulk-a", "bulk-b"] {
        let created = client
            .post(url("/api/v1/routes"))
            .header("cookie", &cookie)
            .json(&json!({"slug":slug,"name":slug,"description":"","patterns":[slug],"candidates":[[provider_id,"m"]]}))
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 200);
    }
    client
        .post(url("/api/v1/users"))
        .header("cookie", &cookie)
        .json(&json!({"username":"editor","password":"password123","role":"editor"}))
        .send()
        .await
        .unwrap();
    let editor_login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"editor","password":"password123"}))
        .send()
        .await
        .unwrap();
    let editor_cookie = editor_login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let denied = client
        .delete(url("/api/v1/routes"))
        .header("cookie", &editor_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    let deleted = client
        .delete(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);
    let body: Value = deleted.json().await.unwrap();
    assert_eq!(body["deleted"], 2);
    let routes: Value = client
        .get(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(routes.as_array().unwrap().len(), 0);
    assert!(store.routes().await.unwrap().is_empty());
}

#[tokio::test]
async fn provider_model_removal_prunes_route_candidates_and_empty_routes() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("prune-routes.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
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
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"test"}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
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
    let provider = |name: &str, models: Value| json!({"name":name,"protocol":"openai","endpoint":"http://127.0.0.1:9/v1","models":models,"secret":"upstream-secret","group_id":"official","enabled":true});
    let first: Value = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&provider("Prune A", json!(["m-a", "m-b"])))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&provider("Prune B", json!(["m-a"])))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let first_id = first["id"].as_i64().unwrap();
    let second_id = second["id"].as_i64().unwrap();
    for (slug, candidates) in [
        ("mixed", json!([[first_id, "m-b"], [second_id, "m-a"]])),
        ("doomed", json!([[first_id, "m-b"]])),
        ("untouched-empty", json!([[second_id, "m-a"]])),
    ] {
        let created = client
            .post(url("/api/v1/routes"))
            .header("cookie", &cookie)
            .json(&json!({"slug":slug,"name":slug,"description":"","patterns":[slug],"candidates":candidates}))
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 200);
    }
    // A pre-existing empty route (update_route accepts an empty candidate
    // list) must survive an unrelated provider's model-list save.
    let routes: Value = client
        .get(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let empty_id = routes
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["slug"] == "untouched-empty")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let emptied = client
        .put(url(&format!("/api/v1/routes/{empty_id}")))
        .header("cookie", &cookie)
        .json(&json!({"slug":"untouched-empty","name":"untouched-empty","description":"","patterns":["untouched-empty"],"candidates":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(emptied.status(), 200);
    let updated = client
        .put(url(&format!("/api/v1/providers/{first_id}")))
        .header("cookie", &cookie)
        .json(&provider("Prune A", json!(["m-a"])))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    let routes: Value = client
        .get(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let routes = routes.as_array().unwrap();
    assert_eq!(
        routes.len(),
        2,
        "route with only removed models must be deleted; the unrelated empty route must survive"
    );
    let mixed = routes
        .iter()
        .find(|route| route["slug"] == "mixed")
        .unwrap();
    assert_eq!(
        mixed["candidates"],
        json!([[second_id, "m-a"]]),
        "only the pruned provider's candidate is removed"
    );
    let empty = routes
        .iter()
        .find(|route| route["slug"] == "untouched-empty")
        .unwrap();
    assert_eq!(empty["candidates"], json!([]));
}

#[tokio::test]
async fn client_key_update_edits_metadata_and_preserves_secret() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("key-edit.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
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
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"test"}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
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
    let created: Value = client
        .post(url("/api/v1/keys"))
        .header("cookie", &cookie)
        .json(&json!({"name":"before","models":["m"],"token_limit":0,"max_amount_micros":0,"rpm":10,"tpm":0}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_i64().unwrap();
    let plaintext = created["plaintext"].as_str().unwrap().to_string();
    let policy = client
        .post(url(&format!("/api/v1/keys/{id}/policy")))
        .header("cookie", &cookie)
        .json(&json!({"ip_whitelist":["10.0.0.1"],"model_quotas":{"m":{"rpm":2,"tpm":50}},"allowed_groups":["official"],"excluded_providers":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(policy.status(), 200);
    client
        .post(url("/api/v1/users"))
        .header("cookie", &cookie)
        .json(&json!({"username":"editor","password":"password123","role":"editor"}))
        .send()
        .await
        .unwrap();
    let editor_login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"editor","password":"password123"}))
        .send()
        .await
        .unwrap();
    let editor_cookie = editor_login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let denied = client
        .put(url(&format!("/api/v1/keys/{id}")))
        .header("cookie", &editor_cookie)
        .json(&json!({"name":"hijacked","models":["*"],"token_limit":0,"max_amount_micros":0,"rpm":0,"tpm":0}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    let expired = client
        .put(url(&format!("/api/v1/keys/{id}")))
        .header("cookie", &cookie)
        .json(&json!({"name":"after","models":["m"],"token_limit":0,"max_amount_micros":0,"rpm":0,"tpm":0,"expires_at":"2020-01-01T00:00"}))
        .send()
        .await
        .unwrap();
    assert_eq!(expired.status(), 400);
    let updated = client
        .put(url(&format!("/api/v1/keys/{id}")))
        .header("cookie", &cookie)
        .json(&json!({"name":"after","models":["m","extra"],"token_limit":5000,"max_amount_micros":1500000,"rpm":60,"tpm":1000,"secret":"{{SECRET_363i2053}}"}))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    let keys: Value = client
        .get(url("/api/v1/keys"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let record = keys
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["id"] == id)
        .unwrap();
    assert_eq!(record["name"], "after");
    assert_eq!(record["models"], json!(["m", "extra"]));
    assert_eq!(record["token_limit"], 5000);
    assert_eq!(record["max_amount_micros"], 1500000);
    assert_eq!(record["rpm"], 60);
    assert_eq!(record["tpm"], 1000);
    assert_eq!(record["ip_whitelist"], json!(["10.0.0.1"]));
    assert_eq!(record["model_quotas"], json!({"m":{"rpm":2,"tpm":50}}));
    assert_eq!(record["allowed_groups"], json!(["official"]));
    assert_eq!(record["excluded_providers"], json!([]));
    assert_eq!(record["model_groups"], json!([]));
    assert!(record.get("plaintext").is_none());
    assert!(record.get("token_hash").is_none());
    let authenticated = store.authenticate_client(&plaintext).await.unwrap();
    assert_eq!(
        authenticated.unwrap().id,
        id,
        "original plaintext stays valid"
    );
    // Round-trip: setting a future expiry then editing again with the same
    // echoed value succeeds (a stored expiry is not re-validated).
    let expiring = client
        .put(url(&format!("/api/v1/keys/{id}")))
        .header("cookie", &cookie)
        .json(&json!({"name":"after","models":["m","extra"],"token_limit":5000,"max_amount_micros":1500000,"rpm":60,"tpm":1000,"expires_at":"2099-01-01T00:00"}))
        .send()
        .await
        .unwrap();
    assert_eq!(expiring.status(), 200);
    let keys: Value = client
        .get(url("/api/v1/keys"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let stored_expiry = keys
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["id"] == id)
        .unwrap()["expires_at"]
        .as_str()
        .unwrap()
        .to_string();
    let echoed = client
        .put(url(&format!("/api/v1/keys/{id}")))
        .header("cookie", &cookie)
        .json(&json!({"name":"renamed-again","models":["m","extra"],"token_limit":5000,"max_amount_micros":1500000,"rpm":60,"tpm":1000,"expires_at":stored_expiry}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        echoed.status(),
        200,
        "unchanged expiry round-trips through edit"
    );
    let missing = client
        .put(url("/api/v1/keys/999999"))
        .header("cookie", &cookie)
        .json(&json!({"name":"ghost","models":[],"token_limit":0,"max_amount_micros":0,"rpm":0,"tpm":0}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 400);
}

#[tokio::test]
async fn new_route_auto_grants_keys_that_selected_all_routes() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("auto-grant.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
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
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"test"}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(url("/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
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
    let provider: Value = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({"name":"Grant","protocol":"openai","endpoint":"http://127.0.0.1:9/v1","models":["m"],"secret":"upstream-secret","group_id":"official","enabled":true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let provider_id = provider["id"].as_i64().unwrap();
    let create_route = |slug: &'static str, enabled: bool| {
        let client = client.clone();
        let cookie = cookie.clone();
        async move {
            let response = client
                .post(url("/api/v1/routes"))
                .header("cookie", &cookie)
                .json(&json!({"slug":slug,"name":slug,"description":"","patterns":[slug],"candidates":[[provider_id,"m"]],"enabled":enabled}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
        }
    };
    let create_key = |name: &'static str, models: Value| {
        let client = client.clone();
        let cookie = cookie.clone();
        async move {
            let created: Value = client
                .post(url("/api/v1/keys"))
                .header("cookie", &cookie)
                .json(&json!({"name":name,"models":models,"token_limit":0,"max_amount_micros":0,"rpm":0,"tpm":0}))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            created["id"].as_i64().unwrap()
        }
    };
    // First route: no key may be extended (empty selection is ambiguous).
    let early = create_key("early", json!([])).await;
    create_route("alpha", true).await;
    let keys_after_first = store.client_keys().await.unwrap();
    assert!(
        keys_after_first
            .iter()
            .find(|key| key.id == early)
            .unwrap()
            .models
            .is_empty()
    );
    create_route("beta", true).await;
    create_route("disabled-route", false).await;
    let all = create_key("all", json!(["alpha", "beta"])).await;
    let partial = create_key("partial", json!(["alpha"])).await;
    let wildcard = create_key("wildcard", json!(["*"])).await;
    let grouped = create_key("grouped", json!([])).await;
    let group: Value = client
        .post(url("/api/v1/model-groups"))
        .header("cookie", &cookie)
        .json(&json!({"name":"Group","members":["alpha","beta"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let group_id = group["id"].as_i64().unwrap();
    let group_policy = client
        .post(url(&format!("/api/v1/keys/{grouped}/policy")))
        .header("cookie", &cookie)
        .json(&json!({"ip_whitelist":[],"model_quotas":{},"allowed_groups":[],"excluded_providers":[],"model_groups":[group_id]}))
        .send()
        .await
        .unwrap();
    assert_eq!(group_policy.status(), 200);
    let disabled_key = create_key("disabled-key", json!(["alpha", "beta"])).await;
    let disabled = client
        .post(url(&format!("/api/v1/keys/{disabled_key}/enabled")))
        .header("cookie", &cookie)
        .json(&json!({"enabled":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(disabled.status(), 200);
    create_route("gamma", true).await;
    let keys = store.client_keys().await.unwrap();
    let models_of = |id: i64| keys.iter().find(|key| key.id == id).unwrap().models.clone();
    assert_eq!(models_of(all), vec!["alpha", "beta", "gamma"]);
    assert_eq!(models_of(partial), vec!["alpha"]);
    assert_eq!(models_of(wildcard), vec!["*"]);
    assert!(
        models_of(grouped).is_empty(),
        "model-group keys are never extended"
    );
    assert!(models_of(early).is_empty());
    // The disabled key keeps its stored scope: auto-grant only touches enabled keys.
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "sqlite://{}",
            directory.path().join("auto-grant.db").display()
        ))
        .await
        .unwrap();
    let disabled_models: String = sqlx::query_scalar("SELECT models FROM client_keys WHERE id = ?")
        .bind(disabled_key)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        disabled_models,
        serde_json::to_string(&json!(["alpha", "beta"])).unwrap()
    );
    // A second new route only extends keys that now cover every enabled route.
    create_route("delta", true).await;
    let keys = store.client_keys().await.unwrap();
    assert_eq!(
        keys.iter().find(|key| key.id == all).unwrap().models,
        vec!["alpha", "beta", "gamma", "delta"]
    );
    assert_eq!(
        keys.iter().find(|key| key.id == partial).unwrap().models,
        vec!["alpha"]
    );
}
