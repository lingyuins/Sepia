use sepia_gateway::{NewProvider, NewRoute, ProviderProtocol, Store, server::AppState};
use serde_json::{Value, json};

#[tokio::test]
async fn large_catalog_is_compressed_without_changing_records() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("large-catalog.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
    let models: Vec<String> = (0..300).map(|index| format!("model-{index:03}")).collect();
    let provider = store
        .create_provider(
            NewProvider {
                name: "large-provider".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://provider.example/v1".into(),
                models,
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "provider-key",
            &[],
            &state.keys,
        )
        .await
        .unwrap();
    for model in &provider.models {
        store
            .set_model_price(provider.id, model, 1_000_000, 2_000_000)
            .await
            .unwrap();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            sepia_gateway::server::app(state.clone())
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let client = reqwest::Client::new();
    let base = format!("http://{address}");
    client
        .post(format!("{base}/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":state.bootstrap_token}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(format!("{base}/api/v1/auth/login"))
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
        .unwrap();
    let response = client
        .get(format!("{base}/api/v1/models/catalog"))
        .header("cookie", cookie)
        .header("accept-encoding", "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-encoding")
            .and_then(|value| value.to_str().ok()),
        Some("gzip"),
    );
    let encoded = response.bytes().await.unwrap();
    assert!(
        encoded.len() < 50_000,
        "compressed catalog was {} bytes",
        encoded.len()
    );
    let mut decoder = flate2::read::GzDecoder::new(encoded.as_ref());
    let catalog: Value = serde_json::from_reader(&mut decoder).unwrap();
    let entries = catalog["models"].as_array().unwrap();
    assert_eq!(entries.len(), 300);
    assert_eq!(entries[0]["model"], "model-000");
    assert_eq!(entries[299]["model"], "model-299");
    assert_eq!(entries[0]["channels"][0]["key_count"], 1);
    assert_eq!(entries[0]["channels"][0]["input_price_micros"], 1_000_000);
    assert_eq!(
        entries[299]["channels"][0]["output_price_micros"],
        2_000_000
    );
}

#[tokio::test]
async fn catalog_aggregates_channels_prices_scopes_and_routes() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("catalog.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
    let keys = state.keys.clone();
    let alpha = store
        .create_provider(
            NewProvider {
                name: "alpha".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://alpha.example/v1".into(),
                models: vec!["gpt-x".into(), "embed-x".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "alpha-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let beta = store
        .create_provider(
            NewProvider {
                name: "beta".into(),
                protocol: ProviderProtocol::Anthropic,
                endpoint: "http://beta.example".into(),
                models: vec!["gpt-x".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "beta-key-1",
            &[("beta-key-1".into(), vec!["gpt-x".into()])],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(beta.id, "beta-key-2", Some(&["other".into()]), None, &keys)
        .await
        .unwrap();
    store
        .set_model_price(alpha.id, "gpt-x", 1_000_000, 2_000_000)
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "smart-x".into(),
            name: "Smart X".into(),
            description: String::new(),
            patterns: vec!["smart-x".into()],
            provider_id: alpha.id,
            model: "gpt-x".into(),
            is_default: false,
            list_members: true,
            candidates: vec![(alpha.id, "gpt-x".into()), (beta.id, "gpt-x".into())],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    store.set_provider_enabled(beta.id, false).await.unwrap();

    let address = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(
            axum::serve(
                listener,
                sepia_gateway::server::app(state.clone())
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .into_future(),
        );
        address
    };
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":state.bootstrap_token}))
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

    let catalog: Value = client
        .get(url("/api/v1/models/catalog"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let entries = catalog["models"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(catalog["source"].is_null());
    assert!(catalog["fetched_at"].is_null());
    assert!(entries[0]["metadata"].is_null());
    assert_eq!(entries[0]["model"], "embed-x");
    assert_eq!(entries[0]["base_model"], "embed-x");
    let gpt = &entries[1];
    assert_eq!(gpt["model"], "gpt-x");
    let channels = gpt["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0]["provider"], "alpha");
    assert_eq!(channels[0]["enabled"], true);
    assert_eq!(channels[0]["key_count"], 1);
    assert_eq!(channels[0]["input_price_micros"], 1_000_000);
    assert_eq!(channels[0]["output_price_micros"], 2_000_000);
    assert_eq!(channels[0]["routes"][0]["slug"], "smart-x");
    assert_eq!(channels[1]["provider"], "beta");
    assert_eq!(channels[1]["enabled"], false);
    // beta-key-1 serves gpt-x, beta-key-2 is scoped to another model.
    assert_eq!(channels[1]["key_count"], 1);
    assert!(channels[1]["input_price_micros"].is_null());
    assert!(channels[1]["output_price_micros"].is_null());
    let embed = &entries[0]["channels"][0];
    assert!(embed["routes"].as_array().unwrap().is_empty());
    assert!(embed["input_price_micros"].is_null());

    // Unauthenticated callers cannot read the catalog.
    let anonymous = client
        .get(url("/api/v1/models/catalog"))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 401);
}

#[tokio::test]
async fn metadata_sync_fetches_source_and_matches_variants() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("metadata.db"))
        .await
        .unwrap();
    let mut state = AppState::for_testing(store.clone()).await;
    let mock_body = json!({"data": [
        {"id": "openai/gpt-x-2024-08-06", "name": "OpenAI: GPT-X (dated)", "description": "dated snapshot", "context_length": 128000,
         "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]},
         "pricing": {"prompt": "0.000001", "completion": "0.000002"},
         "supported_parameters": ["tools", "reasoning"]},
        {"id": "openai/gpt-x", "name": "OpenAI: GPT-X", "context_length": 128000,
         "pricing": {"prompt": "0.0000015", "completion": "0.000003"}},
        {"id": "vendor/kimi-k3:free", "name": "Vendor: Kimi K3 (free)", "context_length": 262144,
         "pricing": {"prompt": "0", "completion": "0"}}
    ]});
    let mock = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = mock_body.clone();
        tokio::spawn(
            axum::serve(
                listener,
                axum::Router::new().route(
                    "/models",
                    axum::routing::get(move || {
                        let body = body.clone();
                        async move { axum::Json(body) }
                    }),
                ),
            )
            .into_future(),
        );
        address
    };
    state.metadata_source = format!("http://{mock}/models");
    let keys = state.keys.clone();
    let alpha = store
        .create_provider(
            NewProvider {
                name: "alpha".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://alpha.example/v1".into(),
                models: vec!["gpt-x-2024-08-06".into(), "gpt-x".into(), "kimi-k3".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "alpha-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .set_model_price(alpha.id, "gpt-x", 5_000_000, 6_000_000)
        .await
        .unwrap();
    let address = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(
            axum::serve(
                listener,
                sepia_gateway::server::app(state.clone())
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .into_future(),
        );
        address
    };
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":state.bootstrap_token}))
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

    // Before any sync the catalog has no metadata.
    let before: Value = client
        .get(url("/api/v1/models/catalog"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(before["source"].is_null());
    assert!(before["models"][0]["metadata"].is_null());

    let sync = client
        .post(url("/api/v1/models/metadata/sync"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(sync.status(), 200);
    let sync_body: Value = sync.json().await.unwrap();
    assert_eq!(sync_body["count"], 3);
    assert_eq!(sync_body["source"], format!("http://{mock}/models"));
    assert!(sync_body["fetched_at"].as_str().is_some());
    // Sync fills only unset channel prices; configured prices stay untouched.
    assert_eq!(sync_body["prices_filled"], 1);

    let catalog: Value = client
        .get(url("/api/v1/models/catalog"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(catalog["source"], format!("http://{mock}/models"));
    let models = catalog["models"].as_array().unwrap();
    assert_eq!(models.len(), 3);
    let plain = &models[0];
    assert_eq!(plain["model"], "gpt-x");
    assert_eq!(plain["metadata"]["source_id"], "openai/gpt-x");
    let dated = &models[1];
    assert_eq!(dated["model"], "gpt-x-2024-08-06");
    assert_eq!(dated["base_model"], "gpt-x");
    assert_eq!(dated["metadata"]["source_id"], "openai/gpt-x-2024-08-06");
    assert_eq!(dated["metadata"]["display_name"], "GPT-X (dated)");
    assert_eq!(dated["metadata"]["context_length"], 128000);
    assert_eq!(dated["metadata"]["input_price_nanos"], 1_000);
    assert_eq!(dated["metadata"]["output_price_nanos"], 2_000);
    assert_eq!(
        dated["metadata"]["capabilities"],
        json!(["tools", "reasoning", "vision"])
    );
    let kimi = &models[2];
    assert_eq!(kimi["model"], "kimi-k3");
    assert_eq!(kimi["metadata"]["source_id"], "vendor/kimi-k3:free");
    assert_eq!(kimi["metadata"]["context_length"], 262144);
    assert!(kimi["metadata"]["input_price_nanos"].is_null());
    assert_eq!(plain["channels"][0]["input_price_micros"], 5_000_000);
    assert_eq!(plain["channels"][0]["output_price_micros"], 6_000_000);
    assert_eq!(dated["channels"][0]["input_price_micros"], 1_000_000);
    assert_eq!(dated["channels"][0]["output_price_micros"], 2_000_000);
    assert!(kimi["channels"][0]["input_price_micros"].is_null());

    // Sync replaces the previous snapshot instead of accumulating rows.
    let again = client
        .post(url("/api/v1/models/metadata/sync"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 200);
    let again_body: Value = again.json().await.unwrap();
    assert_eq!(again_body["prices_filled"], 0);
    let stored = store.model_metadata().await.unwrap();
    assert_eq!(stored.len(), 3);

    let anonymous = client
        .post(url("/api/v1/models/metadata/sync"))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 401);
}

#[tokio::test]
async fn metadata_sync_reports_unreachable_source() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("metadata-fail.db"))
        .await
        .unwrap();
    let mut state = AppState::for_testing(store.clone()).await;
    let dead = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        address
    };
    state.metadata_source = format!("http://{dead}/models");
    let address = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(
            axum::serve(
                listener,
                sepia_gateway::server::app(state.clone())
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .into_future(),
        );
        address
    };
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":state.bootstrap_token}))
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
    let sync = client
        .post(url("/api/v1/models/metadata/sync"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(sync.status(), 502);
    let (source, fetched_at) = store.catalog_meta().await.unwrap();
    assert!(source.is_none());
    assert!(fetched_at.is_none());
}
