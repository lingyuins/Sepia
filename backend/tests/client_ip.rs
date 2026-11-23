//! Client address resolution behind a reverse proxy: with a trusted proxy the
//! forwarded headers drive both the recorded `client_ip` and the key whitelist;
//! without one the transport peer stands and forged headers are inert.
use axum::{Router, response::IntoResponse, routing::any};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    policy::ClientPolicy,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::net::SocketAddr;
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

/// OpenAI-compatible upstream answering every POST with a success payload.
async fn upstream() -> SocketAddr {
    let handler = || async move {
        axum::response::Json(json!({
            "id":"req_1","model":"gpt-test",
            "choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":5},
            "data":[{"embedding":[0.1],"index":0}]
        }))
        .into_response()
    };
    serve(Router::new().fallback(any(handler))).await
}

// Keep the TempDir alive in the caller: dropping it deletes the SQLite files.
async fn seeded(db: &str, address: SocketAddr) -> (Store, i64, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let keys = KeyStore::testing();
    store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{address}/v1"),
                models: vec!["gpt-test".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "upstream-secret",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "gpt-test".into(),
            name: "gpt-test".into(),
            description: String::new(),
            patterns: vec!["gpt-test".into()],
            provider_id: 1,
            model: "gpt-test".into(),
            is_default: false,
            list_members: true,
            candidates: vec![],
        })
        .await
        .unwrap();
    let created = store
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
        .unwrap();
    (store, created.id, created.plaintext, directory)
}

/// Only the forwarded address is whitelisted, so a request classified by the
/// transport peer is rejected while one classified by the headers succeeds.
async fn whitelist(store: &Store, key_id: i64, addresses: &[&str]) {
    store
        .set_client_policy(
            key_id,
            &ClientPolicy {
                ip_whitelist: addresses
                    .iter()
                    .map(|address| address.to_string())
                    .collect(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

async fn post(
    address: SocketAddr,
    path: &str,
    key: &str,
    header: Option<(&str, &str)>,
    body: Value,
) -> reqwest::Response {
    let mut request = reqwest::Client::new()
        .post(format!("http://{address}{path}"))
        .bearer_auth(key)
        .json(&body);
    if let Some((name, value)) = header {
        request = request.header(name, value);
    }
    request.send().await.unwrap()
}

fn chat() -> Value {
    json!({"model":"gpt-test","messages":[{"role":"user","content":"hi"}]})
}

#[tokio::test]
async fn trusted_proxy_honours_forwarded_address() {
    let upstream = upstream().await;
    let (store, key_id, key, _directory) = seeded("client-ip-trusted.db", upstream).await;
    whitelist(&store, key_id, &["203.0.113.7"]).await;
    let mut state = AppState::for_testing(store.clone()).await;
    state.trusted_proxy = true;
    let address = serve(app(state)).await;

    let forwarded = post(
        address,
        "/v1/chat/completions",
        &key,
        Some(("x-forwarded-for", "203.0.113.7, 10.0.0.1")),
        chat(),
    )
    .await;
    assert_eq!(forwarded.status(), 200);
    let real_ip = post(
        address,
        "/v1/chat/completions",
        &key,
        Some(("x-real-ip", "203.0.113.7")),
        chat(),
    )
    .await;
    assert_eq!(real_ip.status(), 200);
    let unknown = post(
        address,
        "/v1/chat/completions",
        &key,
        Some(("x-forwarded-for", "198.51.100.9")),
        chat(),
    )
    .await;
    assert_eq!(unknown.status(), 403);

    let requests = store.requests().await.unwrap();
    assert_eq!(requests.len(), 2, "{requests:?}");
    for record in requests {
        assert_eq!(record.client_ip.as_deref(), Some("203.0.113.7"));
    }
}

#[tokio::test]
async fn untrusted_proxy_ignores_forwarded_address() {
    let upstream = upstream().await;
    let (store, key_id, key, _directory) = seeded("client-ip-untrusted.db", upstream).await;
    whitelist(&store, key_id, &["203.0.113.7"]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;

    let forged = post(
        address,
        "/v1/chat/completions",
        &key,
        Some(("x-forwarded-for", "203.0.113.7")),
        chat(),
    )
    .await;
    assert_eq!(forged.status(), 403);

    whitelist(&store, key_id, &[]).await;
    let recorded = post(
        address,
        "/v1/chat/completions",
        &key,
        Some(("x-forwarded-for", "198.51.100.9")),
        chat(),
    )
    .await;
    assert_eq!(recorded.status(), 200);
    let requests = store.requests().await.unwrap();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].client_ip.as_deref(), Some("127.0.0.1"));
}

#[tokio::test]
async fn auxiliary_and_response_routes_use_the_forwarded_address() {
    let upstream = upstream().await;
    let (store, key_id, key, _directory) = seeded("client-ip-auxiliary.db", upstream).await;
    whitelist(&store, key_id, &["203.0.113.7"]).await;
    let mut state = AppState::for_testing(store.clone()).await;
    state.trusted_proxy = true;
    let address = serve(app(state)).await;

    let embeddings = post(
        address,
        "/v1/embeddings",
        &key,
        Some(("x-forwarded-for", "203.0.113.7")),
        json!({"model":"gpt-test","input":["hi"]}),
    )
    .await;
    assert_eq!(embeddings.status(), 200);
    let response = post(
        address,
        "/v1/responses",
        &key,
        Some(("x-forwarded-for", "203.0.113.7")),
        json!({"model":"gpt-test","input":"hi","store":false}),
    )
    .await;
    assert_eq!(response.status(), 200);
    let direct = post(
        address,
        "/v1/embeddings",
        &key,
        None,
        json!({"model":"gpt-test","input":["hi"]}),
    )
    .await;
    assert_eq!(direct.status(), 403);

    let requests = store.requests().await.unwrap();
    assert_eq!(requests.len(), 2, "{requests:?}");
    for record in requests {
        assert_eq!(record.client_ip.as_deref(), Some("203.0.113.7"));
    }
}
