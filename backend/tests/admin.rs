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
        http: reqwest::Client::new(),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.path().to_path_buf(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        secure_cookie: false,
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
async fn provider_creation_discovers_models_before_persisting() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("discovery-create.db"))
        .await
        .unwrap();
    store
        .bootstrap("admin", "correct horse", "bootstrap", "bootstrap")
        .await
        .unwrap();
    let session = store.login("admin", "correct horse").await.unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}/v1", upstream.local_addr().unwrap());
    tokio::spawn(
        axum::serve(
            upstream,
            axum::Router::new().route(
                "/v1/models",
                axum::routing::get(|| async { Json(json!({"data":[{"id":"discovered-model"}]})) }),
            ),
        )
        .into_future(),
    );
    let state = AppState {
        store: store.clone(),
        keys: KeyStore::testing(),
        http: reqwest::Client::new(),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.path().to_path_buf(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        secure_cookie: false,
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
    let created = client.post(format!("http://{address}/api/v1/providers"))
        .header("cookie", format!("sepia_session={session}"))
        .json(&json!({"name":"Discovered","protocol":"openai","endpoint":upstream_url,"models":[],"secret":"test-secret","group_id":"official","enabled":true}))
        .send().await.unwrap();
    assert_eq!(created.status(), 200, "{}", created.text().await.unwrap());
    let providers = store.providers().await.unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].models, vec!["discovered-model"]);
}
