use axum::{Router, routing::post};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
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

/// Upstream that answers with its own tag and records every hit.
async fn tagged_upstream(tag: &'static str, hits: Arc<Mutex<Vec<&'static str>>>) -> SocketAddr {
    serve(Router::new().fallback(post(move || {
        let hits = hits.clone();
        async move {
            hits.lock().unwrap().push(tag);
            axum::Json(json!({
                "id": "chatcmpl-1",
                "object": "chat.completion",
                "model": "upstream-model",
                "choices": [{"index":0,"message":{"role":"assistant","content":tag},"finish_reason":"stop"}],
                "usage": {"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}
            }))
        }
    })))
    .await
}

async fn provider(store: &Store, name: &str, address: SocketAddr) -> i64 {
    store
        .create_provider(
            NewProvider {
                name: name.into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{address}"),
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
        .id
}

async fn chat(address: SocketAddr, key: &str) -> String {
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn round_robin_cycles_candidates_in_order() {
    let hits = Arc::new(Mutex::new(Vec::new()));
    let first = tagged_upstream("first", hits.clone()).await;
    let second = tagged_upstream("second", hits.clone()).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("rr.db"))
        .await
        .unwrap();
    let a = provider(&store, "alpha", first).await;
    let b = provider(&store, "beta", second).await;
    let route = store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id: a,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![(a, "upstream-model".into()), (b, "upstream-model".into())],
        })
        .await
        .unwrap();
    store
        .set_route_strategy(route.id, "round-robin", &[])
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
            secret: None,
        })
        .await
        .unwrap()
        .plaintext;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    for expected in ["first", "second", "first", "second"] {
        assert_eq!(chat(address, &key).await, expected);
    }
    assert_eq!(
        *hits.lock().unwrap(),
        ["first", "second", "first", "second"]
    );
}

#[tokio::test]
async fn cost_optimized_prefers_cheapest_priced_candidate() {
    let hits = Arc::new(Mutex::new(Vec::new()));
    let expensive = tagged_upstream("expensive", hits.clone()).await;
    let cheap = tagged_upstream("cheap", hits.clone()).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("cost.db"))
        .await
        .unwrap();
    let a = provider(&store, "expensive", expensive).await;
    let b = provider(&store, "cheap", cheap).await;
    store
        .set_model_price(a, "upstream-model", 30_000_000, 60_000_000)
        .await
        .unwrap();
    store
        .set_model_price(b, "upstream-model", 200_000, 600_000)
        .await
        .unwrap();
    let route = store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id: a,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![(a, "upstream-model".into()), (b, "upstream-model".into())],
        })
        .await
        .unwrap();
    store
        .set_route_strategy(route.id, "cost-optimized", &[])
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
            secret: None,
        })
        .await
        .unwrap()
        .plaintext;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    assert_eq!(chat(address, &key).await, "cheap");
    assert_eq!(*hits.lock().unwrap(), ["cheap"]);
}

#[tokio::test]
async fn priority_keeps_saved_candidate_order() {
    let hits = Arc::new(Mutex::new(Vec::new()));
    let first = tagged_upstream("first", hits.clone()).await;
    let second = tagged_upstream("second", hits.clone()).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("priority.db"))
        .await
        .unwrap();
    let a = provider(&store, "alpha", first).await;
    let b = provider(&store, "beta", second).await;
    let route = store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id: a,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![(a, "upstream-model".into()), (b, "upstream-model".into())],
        })
        .await
        .unwrap();
    // Even with lopsided weights, priority ignores them.
    store
        .set_route_strategy(route.id, "priority", &[1, 100])
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
            secret: None,
        })
        .await
        .unwrap()
        .plaintext;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    for _ in 0..3 {
        assert_eq!(chat(address, &key).await, "first");
    }
}

#[tokio::test]
async fn strategy_and_weights_round_trip_through_management_api() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("api.db"))
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
        metadata_source: sepia_gateway::metadata::DEFAULT_SOURCE.into(),
        plugins: sepia_gateway::plugin::shared(
            sepia_gateway::plugin::PluginRuntime::new().unwrap(),
        ),
    };
    let address = serve(app(state)).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    let bootstrap = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), 200);
    let login = client
        .post(url("/api/v1/auth/login"))
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
    let upstream = tagged_upstream("one", Arc::new(Mutex::new(Vec::new()))).await;
    let provider: Value = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({"name":"alpha","protocol":"openai","endpoint":format!("http://{upstream}"),"models":["m-one","m-two"],"secret":"s","secrets":["s"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let provider_id = provider["id"].as_i64().unwrap();
    let candidates = json!([[provider_id, "m-one"], [provider_id, "m-two"]]);

    // Unknown strategy is rejected.
    let invalid = client
        .post(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .json(&json!({"slug":"bad","name":"bad","description":"","patterns":["bad"],"candidates":candidates,"strategy":"nope"}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 400);
    // Weight count must match candidates.
    let mismatched = client
        .post(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .json(&json!({"slug":"bad","name":"bad","description":"","patterns":["bad"],"candidates":candidates,"strategy":"weighted","weights":[1]}))
        .send()
        .await
        .unwrap();
    assert_eq!(mismatched.status(), 400);

    let created = client
        .post(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .json(&json!({"slug":"chat","name":"chat","description":"","patterns":["chat"],"candidates":candidates,"strategy":"weighted","weights":[7,2]}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200);
    let routes: Value = client
        .get(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let route = &routes.as_array().unwrap()[0];
    assert_eq!(route["strategy"], "weighted");
    assert_eq!(route["weights"], json!([7, 2]));
    let route_id = route["id"].as_i64().unwrap();

    // Catalog channels expose the owning route's strategy.
    let catalog: Value = client
        .get(url("/api/v1/models/catalog"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let model = catalog["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["model"] == "m-one")
        .unwrap();
    assert_eq!(model["channels"][0]["routes"][0]["strategy"], "weighted");

    // Update persists a different strategy and clamps weights.
    let updated = client
        .put(url(&format!("/api/v1/routes/{route_id}")))
        .header("cookie", &cookie)
        .json(&json!({"slug":"chat","name":"chat","description":"","patterns":["chat"],"candidates":candidates,"enabled":true,"strategy":"least-used","weights":[0,500]}))
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
    let route = &routes.as_array().unwrap()[0];
    assert_eq!(route["strategy"], "least-used");
    assert_eq!(route["weights"], json!([1, 100]));

    // A partial update without strategy/weights keeps the stored values;
    // reordered candidates keep their weights by identity, new ones get 1.
    let partial = client
        .put(url(&format!("/api/v1/routes/{route_id}")))
        .header("cookie", &cookie)
        .json(&json!({"slug":"chat","name":"chat renamed","description":"","patterns":["chat"],"candidates":json!([[provider_id,"m-two"],[provider_id,"m-one"]]),"enabled":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(partial.status(), 200);
    let routes: Value = client
        .get(url("/api/v1/routes"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let route = &routes.as_array().unwrap()[0];
    assert_eq!(route["strategy"], "least-used");
    assert_eq!(route["weights"], json!([100, 1]));
}
