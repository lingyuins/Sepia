use sepia_gateway::Store;

#[tokio::test]
async fn provider_and_route_updates_round_trip_and_analytics_are_real() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("integration.db"))
        .await
        .unwrap();
    let empty = store.analytics_for("30 days").await.unwrap();
    assert_eq!(empty["requests"], 0);
    assert_eq!(empty["latency_ms"], 0.0);
    assert_eq!(empty["by_route"], serde_json::json!([]));
    let provider = store
        .create_provider(
            sepia_gateway::NewProvider {
                name: "Before".into(),
                protocol: sepia_gateway::ProviderProtocol::OpenAI,
                endpoint: "http://localhost/v1".into(),
                models: vec!["one".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "secret",
            &[],
            &sepia_gateway::KeyStore::testing(),
        )
        .await
        .unwrap();
    store
        .update_provider(sepia_gateway::updates::ProviderUpdate {
            id: provider.id,
            name: "After",
            protocol: sepia_gateway::ProviderProtocol::Anthropic,
            endpoint: "http://localhost/v2",
            models: &["two".into()],
            group: "team",
            enabled: true,
            plugin_id: None,
            mode: "single",
            rewrite: &Default::default(),
        })
        .await
        .unwrap();
    let providers = store.providers().await.unwrap();
    assert_eq!(providers[0].name, "After");
    assert_eq!(
        providers[0].protocol,
        sepia_gateway::ProviderProtocol::Anthropic
    );
    assert_eq!(providers[0].models, vec!["two"]);
    assert!(
        store
            .channel_groups()
            .await
            .unwrap()
            .iter()
            .any(|group| group.id == "official")
    );
    let route = store
        .create_route(sepia_gateway::NewRoute {
            slug: "chat".into(),
            name: "Chat route".into(),
            description: "Real route description".into(),
            patterns: vec!["chat".into()],
            provider_id: provider.id,
            model: "two".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    store
        .update_route(
            route.id,
            sepia_gateway::updates::RouteUpdate {
                slug: "renamed",
                name: "Renamed route",
                description: "Updated route description",
                patterns: &["new-*".into()],
                is_default: false,
                list_members: true,
                enabled: true,
                candidates: &[(provider.id, "two".into())],
                strategy: Some("priority"),
                weights: Some(&[]),
                rewrite: &Default::default(),
            },
        )
        .await
        .unwrap();
    let routes = store.routes().await.unwrap();
    assert_eq!(routes[0].slug, "renamed");
    assert_eq!(routes[0].name, "Renamed route");
    assert_eq!(routes[0].description, "Updated route description");
    assert_eq!(routes[0].patterns, vec!["new-*"]);
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
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
        .unwrap();
    let request = store
        .begin_request(key.id, sepia_gateway::ProviderProtocol::OpenAI, "chat")
        .await
        .unwrap();
    store
        .record_attempt(sepia_gateway::observability::NewAttempt {
            request_id: &request,
            provider_id: Some(provider.id),
            provider_key_id: Some(1),
            model: "two",
            status: "success",
            error: None,
            input_tokens: 10,
            output_tokens: 5,
            latency_ms: 25,
        })
        .await
        .unwrap();
    store
        .finish_request_with_cost(&request, "success", 10, 5, 42, 25)
        .await
        .unwrap();
    store
        .set_group("official", "Official", "Built-in providers")
        .await
        .unwrap();
    let groups = store.channel_groups().await.unwrap();
    assert!(groups.iter().any(|group| group.id == "official"));
    let analytics = store.analytics_for("24 hours").await.unwrap();
    assert_eq!(analytics["requests"], 1);
    assert_eq!(analytics["successful"], 1);
    assert_eq!(analytics["by_provider"][0]["requests"], 1);
    store
        .set_setting("max_attempts", &serde_json::json!(1))
        .await
        .unwrap();
    let snapshots = sepia_gateway::snapshot::Snapshots::load(&store)
        .await
        .unwrap();
    assert_eq!(snapshots.get().max_attempts, 1);
}

async fn seed_recorded_request(
    store: &Store,
    key_id: i64,
    provider_id: i64,
    model: &str,
) -> String {
    let request = store
        .begin_request(key_id, sepia_gateway::ProviderProtocol::OpenAI, model)
        .await
        .unwrap();
    store
        .record_attempt(sepia_gateway::observability::NewAttempt {
            request_id: &request,
            provider_id: Some(provider_id),
            provider_key_id: Some(1),
            model,
            status: "success",
            error: None,
            input_tokens: 10,
            output_tokens: 5,
            latency_ms: 25,
        })
        .await
        .unwrap();
    store
        .finish_request_with_cost(&request, "success", 10, 5, 42, 25)
        .await
        .unwrap();
    request
}

async fn backdate_request(store_path: &std::path::Path, request_id: &str, age: &str) {
    // The Store pool is crate-private; a second connection to the same database file
    // rewrites only the timestamps of a seeded record.
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite://{}", store_path.display()))
        .await
        .unwrap();
    for statement in [
        "UPDATE requests SET created_at = datetime('now', ?) WHERE id = ?",
        "UPDATE attempts SET created_at = datetime('now', ?) WHERE request_id = ?",
    ] {
        sqlx::query(statement)
            .bind(age)
            .bind(request_id)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
}

#[tokio::test]
async fn analytics_periods_cover_ninety_days_and_all_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("analytics-periods.db");
    let store = Store::connect(&path).await.unwrap();
    let provider = store
        .create_provider(
            sepia_gateway::NewProvider {
                name: "Periods".into(),
                protocol: sepia_gateway::ProviderProtocol::OpenAI,
                endpoint: "http://localhost/v1".into(),
                models: vec!["recent".into(), "middle".into(), "legacy".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "secret",
            &[],
            &sepia_gateway::KeyStore::testing(),
        )
        .await
        .unwrap();
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
            name: "periods".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    seed_recorded_request(&store, key.id, provider.id, "recent").await;
    let middle = seed_recorded_request(&store, key.id, provider.id, "middle").await;
    let legacy = seed_recorded_request(&store, key.id, provider.id, "legacy").await;
    backdate_request(&path, &middle, "-60 days").await;
    backdate_request(&path, &legacy, "-120 days").await;

    let day_buckets = |analytics: &serde_json::Value| {
        analytics["series"]
            .as_array()
            .unwrap()
            .iter()
            .all(|point| point["time"].as_str().unwrap().ends_with("T00:00:00Z"))
    };

    // Unchanged narrow periods still exclude the older seeded records.
    assert_eq!(
        store.analytics_for("24 hours").await.unwrap()["requests"],
        1
    );
    assert_eq!(store.analytics_for("7 days").await.unwrap()["requests"], 1);
    assert_eq!(store.analytics_for("30 days").await.unwrap()["requests"], 1);

    let ninety = store.analytics_for("90 days").await.unwrap();
    assert_eq!(ninety["requests"], 2);
    assert_eq!(ninety["input_tokens"], 20);
    assert!(day_buckets(&ninety));
    assert_eq!(ninety["by_provider"][0]["requests"], 2);
    let models = ninety["by_model"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert!(models.iter().any(|row| row["model"] == "recent"));
    assert!(models.iter().any(|row| row["model"] == "middle"));
    assert!(!models.iter().any(|row| row["model"] == "legacy"));
    assert_eq!(
        ninety["series"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| point["requests"].as_i64().unwrap())
            .sum::<i64>(),
        2
    );
    assert_eq!(
        ninety["series"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| point["tokens"].as_i64().unwrap())
            .sum::<i64>(),
        30
    );

    let all = store.analytics_for("all").await.unwrap();
    assert_eq!(all["requests"], 3);
    assert_eq!(all["input_tokens"], 30);
    assert_eq!(all["output_tokens"], 15);
    assert_eq!(all["successful"], 3);
    assert!(day_buckets(&all));
    assert_eq!(all["by_provider"][0]["requests"], 3);
    let models = all["by_model"].as_array().unwrap();
    assert_eq!(models.len(), 3);
    assert!(models.iter().any(|row| row["model"] == "legacy"));
    assert!(models.iter().any(|row| row["model"] == "middle"));
    assert!(models.iter().any(|row| row["model"] == "recent"));
    assert_eq!(
        all["series"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| point["requests"].as_i64().unwrap())
            .sum::<i64>(),
        3
    );

    assert_eq!(
        store.analytics_for("2 days").await.unwrap_err(),
        "unsupported analytics period"
    );
}

async fn seed_routed_request(store: &Store, key_id: i64, model: &str, route: &str) -> String {
    let request = store
        .begin_request(key_id, sepia_gateway::ProviderProtocol::OpenAI, model)
        .await
        .unwrap();
    store.set_request_route(&request, route).await.unwrap();
    store
        .finish_request_with_cost(&request, "success", 10, 5, 42, 25)
        .await
        .unwrap();
    request
}

#[tokio::test]
async fn analytics_by_route_aggregates_routes_outside_the_newest_request_page() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("analytics-routes.db");
    let store = Store::connect(&path).await.unwrap();
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
            name: "routes".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    // The request list the console shows only returns the newest 200 records, so a
    // route whose traffic is older than that page would lose its counters entirely;
    // the server-side aggregate must keep reporting it.
    for _ in 0..3 {
        let request = seed_routed_request(&store, key.id, "legacy-model", "legacy-route").await;
        backdate_request(&path, &request, "-1 days").await;
    }
    for _ in 0..205 {
        seed_routed_request(&store, key.id, "busy-model", "busy-route").await;
    }
    // Direct requests never carry a route and must not become a `""` row.
    let direct = store
        .begin_request(key.id, sepia_gateway::ProviderProtocol::OpenAI, "direct")
        .await
        .unwrap();
    store
        .finish_request_with_cost(&direct, "success", 1, 1, 0, 10)
        .await
        .unwrap();

    let analytics = store.analytics_for("30 days").await.unwrap();
    assert_eq!(analytics["requests"], 209);
    let routes = analytics["by_route"].as_array().unwrap();
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0]["route"], "busy-route");
    assert_eq!(routes[0]["requests"], 205);
    assert_eq!(routes[0]["successful"], 205);
    assert_eq!(routes[0]["tokens"], 3075);
    assert_eq!(routes[0]["latency_ms"], 25.0);
    let legacy = routes
        .iter()
        .find(|row| row["route"] == "legacy-route")
        .unwrap();
    assert_eq!(legacy["requests"], 3);
    assert_eq!(legacy["successful"], 3);
    assert_eq!(legacy["tokens"], 45);
    assert_eq!(legacy["latency_ms"], 25.0);
    let routed: i64 = routes
        .iter()
        .map(|row| row["requests"].as_i64().unwrap())
        .sum();
    assert_eq!(routed, 208);

    // The narrow window still excludes the backdated route while keeping the rest.
    let day = store.analytics_for("24 hours").await.unwrap();
    assert_eq!(day["by_route"].as_array().unwrap().len(), 1);
    assert_eq!(day["by_route"][0]["route"], "busy-route");
}

#[tokio::test]
async fn analytics_by_model_resolves_empty_stored_models_to_the_successful_attempt() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("analytics-models.db"))
        .await
        .unwrap();
    let provider = store
        .create_provider(
            sepia_gateway::NewProvider {
                name: "Models".into(),
                protocol: sepia_gateway::ProviderProtocol::OpenAI,
                endpoint: "http://localhost/v1".into(),
                models: vec!["glm-5.3".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "secret",
            &[],
            &sepia_gateway::KeyStore::testing(),
        )
        .await
        .unwrap();
    let key = store
        .create_client_key(sepia_gateway::UpsertClientKey {
            name: "models".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    // Non-Gemini inbound requests store an empty `requests.model`; the chart must
    // still group them under the real upstream model instead of an unnamed bar.
    // Mix in a direct request with a stored model so the GROUP BY must evaluate
    // the resolution expression per row, not just for the first row of a group.
    seed_recorded_request(&store, key.id, provider.id, "glm-5.3").await;
    seed_recorded_request(&store, key.id, provider.id, "glm-5.3").await;
    seed_recorded_request(&store, key.id, provider.id, "other-model").await;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "sqlite://{}",
            directory.path().join("analytics-models.db").display()
        ))
        .await
        .unwrap();
    sqlx::query("UPDATE requests SET model = '' WHERE model = 'glm-5.3'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let analytics = store.analytics_for("all").await.unwrap();
    let models = analytics["by_model"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["model"], "glm-5.3");
    assert_eq!(models[0]["requests"], 2);
    assert_eq!(models[1]["model"], "other-model");
    assert_eq!(models[1]["requests"], 1);
}
