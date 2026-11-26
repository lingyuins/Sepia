use sepia_gateway::{
    KeyStore, Store,
    server::{AppState, app},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpListener;

async fn start_gateway() -> (String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("events.db"))
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
        litellm_source: String::new(),
        openrouter_source: String::new(),
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
    (format!("http://{address}"), directory)
}

async fn login(client: &reqwest::Client, base: &str) -> String {
    let bootstrap = client
        .post(format!("{base}/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), 200);
    let login = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({"username":"admin","password":"correct horse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

/// Reads whole SSE frames until one carries a `data:` line, then parses it.
/// Keep-alive comment frames are skipped, so this stays deterministic without
/// sleeping.
async fn next_event(response: &mut reqwest::Response) -> Value {
    let mut buffer = String::new();
    loop {
        if let Some(end) = buffer.find("\n\n") {
            let frame: String = buffer.drain(..end + 2).collect();
            let payload = frame
                .lines()
                .find_map(|line| line.strip_prefix("data:").map(str::trim_start));
            if let Some(payload) = payload {
                return serde_json::from_str(payload).unwrap();
            }
        }
        let chunk = response.chunk().await.unwrap().unwrap();
        buffer.push_str(std::str::from_utf8(&chunk).unwrap());
    }
}

#[tokio::test]
async fn events_require_session_and_report_management_mutations() {
    let (base, _directory) = start_gateway().await;
    let client = reqwest::Client::new();
    let anonymous = client
        .get(format!("{base}/api/v1/events"))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 401);

    let cookie = login(&client, &base).await;
    // The handler subscribes before the response headers reach the client, so
    // a mutation sent after this resolves cannot outrun the subscription.
    let response = client
        .get(format!("{base}/api/v1/events"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/event-stream"
    );
    let mut stream = response;

    let created = client
        .post(format!("{base}/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"Live events",
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
    assert_eq!(created.status(), 200);
    let event = tokio::time::timeout(Duration::from_secs(5), next_event(&mut stream))
        .await
        .expect("provider creation event");
    assert_eq!(event["domains"], json!(["providers", "routes"]));

    // Reads never publish: a plain listing must not wake the stream.
    let listed = client
        .get(format!("{base}/api/v1/providers"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(listed.status(), 200);
    assert!(
        tokio::time::timeout(Duration::from_millis(300), next_event(&mut stream))
            .await
            .is_err(),
        "a GET must not publish a workspace event"
    );

    // Clearing request history invalidates the request list and analytics.
    let cleared = client
        .delete(format!("{base}/api/v1/requests"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), 200);
    let event = tokio::time::timeout(Duration::from_secs(5), next_event(&mut stream))
        .await
        .expect("request clear event");
    assert_eq!(event["domains"], json!(["requests", "analytics"]));
}
