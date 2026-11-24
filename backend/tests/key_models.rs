use axum::{Router, extract::State, routing::any};
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

// Relay-style upstream: each key sees a different model catalog and only
// accepts inference for models in that catalog.
async fn relay_upstream() -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let handler = any(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let path = request.uri().path().to_string();
            let secret = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let value: Value = if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&body).unwrap()
            };
            hits.0.lock().unwrap().push((path.clone(), secret.clone()));
            let catalog: &[&str] = match secret.as_str() {
                "Bearer claude-key" => &["claude-a", "claude-b"],
                "Bearer gpt-key" => &["gpt-a"],
                _ => &[],
            };
            if path.ends_with("/models") {
                if catalog.is_empty() {
                    return axum::http::StatusCode::UNAUTHORIZED.into_response();
                }
                return axum::Json(
                    json!({"data": catalog.iter().map(|id| json!({"id": id})).collect::<Vec<_>>()}),
                )
                .into_response();
            }
            let model = value.get("model").and_then(Value::as_str).unwrap_or("");
            if !catalog.contains(&model) {
                return axum::http::StatusCode::UNAUTHORIZED.into_response();
            }
            axum::Json(json!({"id":"chat_1","model":model,"choices":[{"index":0,"message":{"role":"assistant","content":"scoped reply"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})).into_response()
        },
    );
    let address = serve(Router::new().fallback(handler).with_state(hits.clone())).await;
    (address, hits)
}

use axum::response::IntoResponse;

// Keep the TempDir alive in the caller: dropping it deletes the SQLite files
// while pooled connections still reference them (disk I/O errors on Linux).
async fn store_with_scoped_provider(
    db: &str,
    address: SocketAddr,
) -> (Store, i64, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{address}"),
                models: vec!["claude-a".into(), "claude-b".into(), "gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "claude-key",
            &[(
                "claude-key".into(),
                vec!["claude-a".into(), "claude-b".into()],
            )],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "gpt-key", Some(&["gpt-a".into()]), None, &keys)
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "claude-a".into(),
            name: "claude-a".into(),
            description: String::new(),
            patterns: vec!["claude-a".into()],
            provider_id: provider.id,
            model: "claude-a".into(),
            is_default: false,
            list_members: true,
            candidates: vec![],
            rewrite: Default::default(),
        })
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "gpt-a".into(),
            name: "gpt-a".into(),
            description: String::new(),
            patterns: vec!["gpt-a".into()],
            provider_id: provider.id,
            model: "gpt-a".into(),
            is_default: false,
            list_members: true,
            candidates: vec![],
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
    (store, provider.id, key, directory)
}

async fn gateway(store: &Store) -> SocketAddr {
    serve(app(AppState::for_testing(store.clone()).await)).await
}

#[tokio::test]
async fn requests_only_use_keys_whose_scope_contains_the_model() {
    let (upstream, hits) = relay_upstream().await;
    let (store, _, key, _directory) = store_with_scoped_provider("scoped.db", upstream).await;
    let address = gateway(&store).await;
    for model in ["claude-a", "gpt-a"] {
        let response = reqwest::Client::new()
            .post(format!("http://{address}/v1/chat/completions"))
            .bearer_auth(&key)
            .json(&json!({"model":model,"messages":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{model}");
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["choices"][0]["message"]["content"], "scoped reply");
    }
    let inference: Vec<_> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.ends_with("/chat/completions"))
        .cloned()
        .collect();
    assert_eq!(
        inference
            .iter()
            .map(|(_, secret)| secret.as_str())
            .collect::<Vec<_>>(),
        vec!["Bearer claude-key", "Bearer gpt-key"]
    );
    let requests = store.requests().await.unwrap();
    assert!(requests.iter().all(|request| request.status == "success"));
}

#[tokio::test]
async fn auth_rotation_stays_within_eligible_scoped_keys() {
    // Two keys scoped to the same model, one unscoped key that must never be
    // used for it; the first scoped key is rejected upstream.
    let hits = Hits::default();
    let handler = any({
        let hits = hits.clone();
        move |request: axum::extract::Request| {
            let hits = hits.clone();
            async move {
                let secret = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                hits.0
                    .lock()
                    .unwrap()
                    .push((request.uri().path().to_string(), secret.clone()));
                if secret == "Bearer good-claude" {
                    axum::Json(json!({"id":"chat_1","model":"claude-a","choices":[{"index":0,"message":{"role":"assistant","content":"rotated"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})).into_response()
                } else {
                    axum::http::StatusCode::UNAUTHORIZED.into_response()
                }
            }
        }
    });
    let upstream = serve(Router::new().fallback(handler)).await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("rotation.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["claude-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "bad-claude",
            &[("bad-claude".into(), vec!["claude-a".into()])],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(
            provider.id,
            "good-claude",
            Some(&["claude-a".into()]),
            None,
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "unscoped-key", None, None, &keys)
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "claude-a".into(),
            name: "claude-a".into(),
            description: String::new(),
            patterns: vec!["claude-a".into()],
            provider_id: provider.id,
            model: "claude-a".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
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
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"claude-a","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let secrets: Vec<_> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(_, secret)| secret.clone())
        .collect();
    assert_eq!(secrets, vec!["Bearer bad-claude", "Bearer good-claude"]);
}

#[tokio::test]
async fn unscoped_keys_still_serve_every_model() {
    let (upstream, hits) = relay_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("legacy.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}"),
                models: vec!["gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "gpt-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .create_route(NewRoute {
            slug: "gpt-a".into(),
            name: "gpt-a".into(),
            description: String::new(),
            patterns: vec!["gpt-a".into()],
            provider_id: provider.id,
            model: "gpt-a".into(),
            is_default: true,
            list_members: true,
            candidates: vec![],
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
    let address = gateway(&store).await;
    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"gpt-a","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(hits.0.lock().unwrap().last().unwrap().1, "Bearer gpt-key");
}

#[tokio::test]
async fn management_discovers_models_per_key_and_persists_scopes() {
    let (upstream, hits) = relay_upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("admin.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
    let address = serve(app(state)).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":AppState::for_testing(store.clone()).await.bootstrap_token}))
        .send()
        .await
        .unwrap();
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

    // Create with empty models: per-key discovery must union both catalogs.
    let created = client
        .post(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"relay",
            "protocol":"openai",
            "endpoint":format!("http://{upstream}"),
            "models":[],
            "secret":"claude-key",
            "secrets":["claude-key","gpt-key"],
            "key_models":[
                {"secret":"claude-key","models":["claude-a","claude-b"]},
                {"secret":"gpt-key","models":["gpt-a"]}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        created.status(),
        200,
        "{}",
        created.text().await.unwrap_or_default()
    );
    let providers: Value = client
        .get(url("/api/v1/providers"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let provider = &providers[0];
    let mut models: Vec<&str> = provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model.as_str().unwrap())
        .collect();
    models.sort_unstable();
    assert_eq!(models, vec!["claude-a", "claude-b", "gpt-a"]);
    assert_eq!(
        provider["keys"][0]["models"],
        json!(["claude-a", "claude-b"])
    );
    assert_eq!(provider["keys"][1]["models"], json!(["gpt-a"]));
    let provider_id = provider["id"].as_i64().unwrap();
    let key_id = provider["keys"][0]["id"].as_i64().unwrap();

    // Per-key detection with a saved key (no plaintext) via key_index.
    let detected: Value = client
        .post(url(&format!("/api/v1/providers/{provider_id}/test")))
        .header("cookie", &cookie)
        .json(&json!({"key_index":1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detected["models"], json!(["gpt-a"]));

    // Scope editing persists through the dedicated endpoint.
    let updated = client
        .put(url(&format!(
            "/api/v1/providers/{provider_id}/keys/{key_id}/models"
        )))
        .header("cookie", &cookie)
        .json(&json!({"models":["claude-a"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    let keys: Value = client
        .get(url(&format!("/api/v1/providers/{provider_id}/keys")))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(keys[0]["models"], json!(["claude-a"]));

    // Provider update can also re-scope keys by secret.
    let updated = client
        .put(url(&format!("/api/v1/providers/{provider_id}")))
        .header("cookie", &cookie)
        .json(&json!({
            "name":"relay",
            "protocol":"openai",
            "endpoint":format!("http://{upstream}"),
            "models":["claude-a","claude-b","gpt-a"],
            "secret":"",
            "key_models":[{"secret":"claude-key","models":["claude-a","claude-b"]}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    let keys: Value = client
        .get(url(&format!("/api/v1/providers/{provider_id}/keys")))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(keys[0]["models"], json!(["claude-a", "claude-b"]));

    let discoveries: Vec<_> = hits
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.ends_with("/models"))
        .map(|(_, secret)| secret.clone())
        .collect();
    assert_eq!(
        discoveries,
        vec!["Bearer claude-key", "Bearer gpt-key", "Bearer gpt-key"]
    );
}

// Model groups widen a key's scope by union; they never rewrite routing.
async fn store_with_group_route(
    db: &str,
    address: SocketAddr,
) -> (Store, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join(db)).await.unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "relay".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{address}"),
                models: vec!["claude-a".into(), "claude-b".into(), "gpt-a".into()],
                plugin_id: None,
                mode: "single".into(),
                rewrite: Default::default(),
            },
            "claude-key",
            &[(
                "claude-key".into(),
                vec!["claude-a".into(), "claude-b".into()],
            )],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "gpt-key", Some(&["gpt-a".into()]), None, &keys)
        .await
        .unwrap();
    for model in ["claude-a", "claude-b", "gpt-a"] {
        store
            .create_route(NewRoute {
                slug: model.into(),
                name: model.into(),
                description: String::new(),
                patterns: vec![model.into()],
                provider_id: provider.id,
                model: model.into(),
                is_default: false,
                list_members: true,
                candidates: vec![],
                rewrite: Default::default(),
            })
            .await
            .unwrap();
    }
    let key = store
        .create_client_key(UpsertClientKey {
            name: "grouped".into(),
            models: vec!["gpt-a".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    (store, key.plaintext, directory)
}

#[tokio::test]
async fn model_group_members_union_into_key_scope() {
    let (upstream, _hits) = relay_upstream().await;
    let (store, key, _directory) = store_with_group_route("group_union.db", upstream).await;
    let client = reqwest::Client::new();
    let key_id = store.client_keys().await.unwrap()[0].id;
    let address = gateway(&store).await;
    // Before any group reference, only the key's own model is allowed.
    let before = client
        .post(format!("http://{address}/v1/chat/completions"))
        .bearer_auth(&key)
        .json(&json!({"model":"claude-a","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(before.status(), 403);
    let group = store
        .create_model_group("Claude", "", &["claude-a".into(), "claude-b".into()])
        .await
        .unwrap();
    let mut policy = store.client_policy(key_id).await.unwrap();
    policy.model_groups = vec![group.id];
    store.set_client_policy(key_id, &policy).await.unwrap();
    // The group's members now authorize claude-a alongside the key's gpt-a.
    for model in ["claude-a", "gpt-a"] {
        let response = client
            .post(format!("http://{address}/v1/chat/completions"))
            .bearer_auth(&key)
            .json(&json!({"model":model,"messages":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{model}");
    }
    // The model list reflects the union of key models and group members.
    let models: Value = client
        .get(format!("http://{address}/v1/models"))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"claude-a"), "{ids:?}");
    assert!(ids.contains(&"claude-b"), "{ids:?}");
    assert!(ids.contains(&"gpt-a"), "{ids:?}");
}

#[tokio::test]
async fn deleting_a_referenced_model_group_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("group_delete.db"))
        .await
        .unwrap();
    let state = AppState::for_testing(store.clone()).await;
    let address = serve(app(state)).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{address}{path}");
    client
        .post(url("/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":AppState::for_testing(store.clone()).await.bootstrap_token}))
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
    let group: Value = client
        .post(url("/api/v1/model-groups"))
        .header("cookie", &cookie)
        .json(&json!({"name":"Claude","members":["claude-a"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let group_id = group["id"].as_i64().unwrap();
    let key = store
        .create_client_key(UpsertClientKey {
            name: "ref".into(),
            models: vec![],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: None,
        })
        .await
        .unwrap();
    let mut policy = store.client_policy(key.id).await.unwrap();
    policy.model_groups = vec![group_id];
    store.set_client_policy(key.id, &policy).await.unwrap();
    let rejected = client
        .delete(url(&format!("/api/v1/model-groups/{group_id}")))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 409);
    // Detaching the reference allows deletion.
    policy.model_groups = vec![];
    store.set_client_policy(key.id, &policy).await.unwrap();
    let deleted = client
        .delete(url(&format!("/api/v1/model-groups/{group_id}")))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);
    let listed: Value = client
        .get(url("/api/v1/model-groups"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed["groups"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn create_client_key_honors_an_imported_secret() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("imported_secret.db"))
        .await
        .unwrap();
    let imported = store
        .create_client_key(UpsertClientKey {
            name: "imported".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: Some("sk-octopus-imported".into()),
        })
        .await
        .unwrap();
    assert_eq!(imported.plaintext, "sk-octopus-imported");
    let authenticated = store
        .authenticate_client("sk-octopus-imported")
        .await
        .unwrap();
    assert!(authenticated.is_some());
    // A blank or absent secret still generates a fresh sk-sepia- token.
    let generated = store
        .create_client_key(UpsertClientKey {
            name: "generated".into(),
            models: vec!["*".into()],
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
            secret: Some("   ".into()),
        })
        .await
        .unwrap();
    assert!(generated.plaintext.starts_with("sk-sepia-"));
}
