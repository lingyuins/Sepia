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
    let provider = store
        .create_provider(
            sepia_gateway::NewProvider {
                name: "Before".into(),
                protocol: sepia_gateway::ProviderProtocol::OpenAI,
                endpoint: "http://localhost/v1".into(),
                models: vec!["one".into()],
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
        .finish_request_with_cost(&request, "success", 10, 5, 42)
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
