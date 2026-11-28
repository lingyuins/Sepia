//! Observability durability: WAL journaling + attempts/requests indexes keep
//! console reads from starving inference writes, attempt/route write failures
//! never discard a successful upstream reply, and `body_retention_days`
//! actually prunes request history (attempts and bodies cascade).
use axum::{Router, routing::post};
use sepia_gateway::{
    KeyStore, NewProvider, NewRoute, ProviderProtocol, Store, UpsertClientKey,
    server::{AppState, app},
};
use serde_json::{Value, json};
use sqlx::{Row, sqlite::SqlitePool};
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

fn chat_reply() -> Value {
    json!({"id":"chat_1","model":"upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"recovered"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})
}

// `Store::pool` is crate-private, so tests open their own connection to the
// same WAL file (readers and writers coexist without blocking each other).
async fn external_pool(directory: &std::path::Path, db: &str) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite://{}", directory.join(db).display()))
        .await
        .unwrap()
}

// Keep the TempDir alive in the caller while pooled connections use the files.
async fn seeded(db: &str, upstream: SocketAddr) -> (Store, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["upstream-model".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "upstream-secret",
            &[],
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
            candidates: vec![(provider.id, "upstream-model".into())],
            rewrite: Default::default(),
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

async fn chat(address: SocketAddr, key: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(key)
        .json(&json!({"model":"chat","messages":[{"role":"user","content":"Hello"}]}))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn store_uses_wal_and_observability_indexes() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("wal.db"))
        .await
        .unwrap();
    let pool = external_pool(directory.path(), "wal.db").await;
    let journal: String = sqlx::query("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(journal, "wal");
    let indexes: Vec<String> = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN ('attempts_request', 'requests_created')",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.get(0))
    .collect();
    assert_eq!(indexes.len(), 2, "{indexes:?}");
    drop(store);
}

#[tokio::test]
async fn attempt_write_failure_never_discards_a_successful_reply() {
    let upstream = serve(Router::new().fallback(post(|| async { axum::Json(chat_reply()) }))).await;
    let (store, key, directory) = seeded("attempt-loss.db", upstream).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    // Break only the attempt table: the request row, route attribution and
    // quota settlement still work, so the successful reply must survive.
    let pool = external_pool(directory.path(), "attempt-loss.db").await;
    sqlx::query("DROP TABLE attempts")
        .execute(&pool)
        .await
        .unwrap();
    let response = chat(address, &key).await;
    assert_eq!(response.status(), 200);
    let value: Value = response.json().await.unwrap();
    assert_eq!(value["choices"][0]["message"]["content"], "recovered");
    // The console list itself joins the attempt table, so read the request
    // row directly: it must carry the real success outcome and tokens.
    let row = sqlx::query("SELECT status, input_tokens, output_tokens FROM requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "success");
    assert_eq!(row.get::<i64, _>("input_tokens"), 2);
    assert_eq!(row.get::<i64, _>("output_tokens"), 1);
}

async fn seed_retention_rows(store: &Store, pool: &SqlitePool, endpoint: &str) -> (String, String) {
    let keys = KeyStore::testing();
    store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: endpoint.into(),
                models: vec!["m".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "secret",
            &[],
            &keys,
        )
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
        .unwrap();
    let old = store
        .begin_request(key.id, ProviderProtocol::OpenAI, "m")
        .await
        .unwrap();
    let fresh = store
        .begin_request(key.id, ProviderProtocol::OpenAI, "m")
        .await
        .unwrap();
    sqlx::query("UPDATE requests SET created_at = ? WHERE id = ?")
        .bind((chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339())
        .bind(&old)
        .execute(pool)
        .await
        .unwrap();
    (old, fresh)
}

#[tokio::test]
async fn retention_validation_and_pruning_cascade() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("retention.db"))
        .await
        .unwrap();
    let pool = external_pool(directory.path(), "retention.db").await;
    for invalid in [json!(-1), json!(3651), json!("7"), json!(1.5)] {
        assert!(
            store
                .set_setting("body_retention_days", &invalid)
                .await
                .is_err(),
            "accepted {invalid}"
        );
    }
    store
        .set_setting("body_retention_days", &json!(0))
        .await
        .unwrap();
    let (old, fresh) = seed_retention_rows(&store, &pool, "http://127.0.0.1:9").await;
    for request_id in [&old, &fresh] {
        store
            .record_attempt(sepia_gateway::observability::NewAttempt {
                request_id,
                provider_id: Some(1),
                provider_key_id: None,
                model: "m",
                status: "success",
                error: None,
                input_tokens: 1,
                output_tokens: 1,
                latency_ms: 1,
            })
            .await
            .unwrap();
        store.save_request_body(request_id, "{}").await.unwrap();
    }
    // Unset/0 keeps history forever.
    let events = sepia_gateway::events::Events::new();
    assert_eq!(store.prune_requests(0).await.unwrap(), 0);
    assert_eq!(store.enforce_retention(&events).await.unwrap(), 0);
    assert_eq!(store.requests().await.unwrap().len(), 2);
    // A real window removes only the aged row, cascading attempts and bodies.
    store
        .set_setting("body_retention_days", &json!(30))
        .await
        .unwrap();
    assert_eq!(store.enforce_retention(&events).await.unwrap(), 1);
    let requests = store.requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id, fresh);
    assert!(store.attempts(&old).await.unwrap().is_empty());
    assert!(store.request_bodies(&old).await.unwrap().is_none());
    assert_eq!(store.attempts(&fresh).await.unwrap().len(), 1);
    assert!(store.request_bodies(&fresh).await.unwrap().is_some());
}

#[tokio::test]
async fn saving_retention_setting_prunes_immediately() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("retention-api.db"))
        .await
        .unwrap();
    let pool = external_pool(directory.path(), "retention-api.db").await;
    let state = AppState::for_testing(store.clone()).await;
    let address = serve(app(state)).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    let bootstrap = client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"test"}))
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
    let (old, _fresh) = seed_retention_rows(&store, &pool, "http://127.0.0.1:9").await;
    let saved = client
        .post(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&json!({"body_retention_days":30}))
        .send()
        .await
        .unwrap();
    assert_eq!(saved.status(), 200);
    // The sweep runs in a spawned task; poll briefly for its effect.
    let mut remaining = 2;
    for _ in 0..100 {
        remaining = store.requests().await.unwrap().len();
        if remaining == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(remaining, 1);
    assert!(store.attempts(&old).await.unwrap().is_empty());
    let rejected = client
        .post(url("/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&json!({"body_retention_days":-1}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 400);
}
