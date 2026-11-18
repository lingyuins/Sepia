use sepia_gateway::{NewProvider, NewRoute, ProviderProtocol, Store, server::AppState};
use serde_json::{Value, json};

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
            },
            "beta-key-1",
            &[("beta-key-1".into(), vec!["gpt-x".into()])],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(beta.id, "beta-key-2", Some(&["other".into()]), &keys)
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
    let entries = catalog.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["model"], "embed-x");
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
