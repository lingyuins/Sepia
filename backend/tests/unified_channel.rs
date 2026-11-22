use axum::{
    Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Hits(Arc<Mutex<Vec<(String, String)>>>);

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

fn chat_reply() -> Value {
    json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"recovered"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})
}

// An upstream that fails every inference leg with 500 but can serve a model
// catalog, recording the bearer secret of every hit.
async fn failing_upstream() -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let handler = get(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            hits.0
                .lock()
                .unwrap()
                .push((request.uri().path().to_string(), secret));
            axum::Json(json!({"data":[{"id":"upstream-model"}]})).into_response()
        },
    );
    let inference = post(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            hits.0
                .lock()
                .unwrap()
                .push((request.uri().path().to_string(), secret));
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        },
    );
    let address = serve(
        Router::new()
            .fallback(inference)
            .route("/models", handler)
            .with_state(hits.clone()),
    )
    .await;
    (address, hits)
}

async fn healthy_upstream() -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let handler = get(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            hits.0
                .lock()
                .unwrap()
                .push((request.uri().path().to_string(), secret));
            axum::Json(json!({"data":[{"id":"upstream-model"}]})).into_response()
        },
    );
    let inference = post(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            hits.0
                .lock()
                .unwrap()
                .push((request.uri().path().to_string(), secret));
            axum::Json(chat_reply()).into_response()
        },
    );
    let address = serve(
        Router::new()
            .fallback(inference)
            .route("/models", handler)
            .with_state(hits.clone()),
    )
    .await;
    (address, hits)
}

// Keep the TempDir alive in the caller while pooled connections use the files.
async fn unified_store(
    db: &str,
    first: SocketAddr,
    second: SocketAddr,
) -> (Store, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "unified".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{first}"),
                models: vec!["upstream-model".into()],
                plugin_id: None,
                mode: "unified".into(),
            },
            "first-secret",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let second_endpoint = store
        .append_provider_endpoint(provider.id, &format!("http://{second}"))
        .await
        .unwrap();
    store
        .add_provider_key(
            provider.id,
            "second-secret",
            None,
            Some(second_endpoint),
            &keys,
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "chat".into(),
            name: "chat".into(),
            description: String::new(),
            patterns: vec!["chat".into()],
            provider_id: provider.id,
            model: "upstream-model".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
        })
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
    (store, key, directory)
}

async fn gateway(store: &Store) -> SocketAddr {
    serve(app(AppState::for_testing(store.clone()).await)).await
}

#[tokio::test]
async fn unified_channel_fails_over_to_the_second_address() {
    let (first, first_hits) = failing_upstream().await;
    let (second, second_hits) = healthy_upstream().await;
    let (store, key, _directory) = unified_store("failover.db", first, second).await;
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
        "recovered"
    );
    // The first address's key fails both legs; the second address's own key
    // (not the first address's) serves the recovered reply.
    {
        let first = first_hits.0.lock().unwrap();
        assert!(!first.is_empty());
        assert!(
            first
                .iter()
                .all(|(_, secret)| secret == "Bearer first-secret")
        );
    }
    {
        let second = second_hits.0.lock().unwrap();
        assert!(
            second
                .iter()
                .any(|(_, secret)| secret == "Bearer second-secret")
        );
    }
    let requests = store.requests().await.unwrap();
    let attempts = store.attempts(&requests[0].id).await.unwrap();
    assert_eq!(attempts.last().unwrap().status, "success");
    assert!(attempts.iter().any(|attempt| attempt.status == "retry"));
}

#[tokio::test]
async fn unified_channel_rotates_keys_within_an_address() {
    // The first address rejects every key with 401; its two keys rotate within
    // the leg before the request fails over to the healthy second address.
    let unauthorized = {
        let hits = Hits::default();
        let handler = post(
            move |State(hits): State<Hits>, request: axum::extract::Request| async move {
                let secret = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                hits.0
                    .lock()
                    .unwrap()
                    .push((request.uri().path().to_string(), secret));
                axum::http::StatusCode::UNAUTHORIZED.into_response()
            },
        );
        let address = serve(Router::new().fallback(handler).with_state(hits.clone())).await;
        (address, hits)
    };
    let (second, _) = healthy_upstream().await;
    let (store, key, _directory) = unified_store("rotate.db", unauthorized.0, second).await;
    let provider = store.providers().await.unwrap()[0].id;
    let first_endpoint = store.provider_endpoints(provider).await.unwrap()[0].id;
    store
        .add_provider_key(
            provider,
            "first-extra",
            None,
            Some(first_endpoint),
            &KeyStore::testing(),
        )
        .await
        .unwrap();
    // Both legs of the first address rotate through two keys (4 attempts);
    // raise the budget so the second address still gets a turn.
    store.set_setting("max_attempts", &json!(8)).await.unwrap();
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let hits = unauthorized.1.0.lock().unwrap();
    let secrets: Vec<&str> = hits.iter().map(|(_, secret)| secret.as_str()).collect();
    assert!(secrets.contains(&"Bearer first-secret"));
    assert!(secrets.contains(&"Bearer first-extra"));
}

#[tokio::test]
async fn sync_provider_endpoints_preserves_keys_and_cascades_removed_addresses() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("sync.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "unified".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: "http://a.test/v1".into(),
                models: vec!["m".into()],
                plugin_id: None,
                mode: "unified".into(),
            },
            "a-secret",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let b = store
        .append_provider_endpoint(provider.id, "http://b.test/v1")
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "b-secret", None, Some(b), &keys)
        .await
        .unwrap();
    let before = store.provider_endpoints(provider.id).await.unwrap();
    assert_eq!(before.len(), 2);
    // Reorder/replace addresses by position: the first row keeps its id (and
    // its key), the second row's address changes but its key survives, and a
    // removed third address would cascade-delete its keys.
    let synced = store
        .sync_provider_endpoints(
            provider.id,
            &["http://a.test/v1".into(), "http://c.test/v1".into()],
        )
        .await
        .unwrap();
    assert_eq!(synced.len(), 2);
    assert_eq!(synced[0].id, before[0].id);
    assert_eq!(synced[1].id, before[1].id);
    assert_eq!(synced[1].endpoint, "http://c.test/v1");
    let keys_after = store.provider_keys(provider.id).await.unwrap();
    assert_eq!(keys_after.len(), 2);
    assert!(
        keys_after
            .iter()
            .any(|key| key.endpoint_id == Some(before[1].id))
    );
    // providers.endpoint mirrors the first address.
    let mirrored = store.providers().await.unwrap()[0].endpoint.clone();
    assert_eq!(mirrored, "http://a.test/v1");
    // Shrinking to one address removes the second row and its key.
    let shrunk = store
        .sync_provider_endpoints(provider.id, &["http://a.test/v1".into()])
        .await
        .unwrap();
    assert_eq!(shrunk.len(), 1);
    let keys_shrunk = store.provider_keys(provider.id).await.unwrap();
    assert_eq!(keys_shrunk.len(), 1);
    assert_eq!(keys_shrunk[0].endpoint_id, Some(before[0].id));
}

#[tokio::test]
async fn unified_channel_management_creates_addresses_and_detects_only_the_first() {
    let (first, first_hits) = failing_upstream().await;
    let (second, second_hits) = healthy_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("admin.db"))
        .await
        .unwrap();
    let mut state = AppState::for_testing(store.clone()).await;
    state.bootstrap_token = "bootstrap".into();
    let address = serve(app(state)).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"bootstrap"}))
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
    let created = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"Unified admin",
            "protocol":"openai",
            "endpoint":format!("http://{first}"),
            "models":["upstream-model"],
            "secret":"first-secret",
            "mode":"unified",
            "endpoints":[
                {"endpoint":format!("http://{first}"),"secrets":["first-secret"]},
                {"endpoint":format!("http://{second}"),"secrets":["second-secret"]}
            ],
            "group_id":"official",
            "enabled":true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200);
    let provider_id = created.json::<Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let providers: Value = client
        .get(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(providers[0]["mode"], "unified");
    assert_eq!(providers[0]["endpoints"].as_array().unwrap().len(), 2);
    assert_eq!(providers[0]["keys"].as_array().unwrap().len(), 2);
    // Detection targets the first address only: key_index 0 resolves to the
    // first address's key and never touches the second address.
    let detected = client
        .post(url(&format!("/api/v1/providers/{provider_id}/test")))
        .header("cookie", &cookie)
        .json(&json!({"key_index":0}))
        .send()
        .await
        .unwrap();
    assert_eq!(detected.status(), 200);
    assert_eq!(
        detected.json::<Value>().await.unwrap()["models"][0],
        "upstream-model"
    );
    let first = first_hits.0.lock().unwrap();
    assert!(
        first
            .iter()
            .all(|(_, secret)| secret == "Bearer first-secret")
    );
    assert!(!first.is_empty());
    assert!(second_hits.0.lock().unwrap().is_empty());
}
