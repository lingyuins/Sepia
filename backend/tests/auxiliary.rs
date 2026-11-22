use axum::{Router, extract::State, response::IntoResponse, routing::any};
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

// OpenAI-style upstream: only `Bearer good-key` is accepted. Every auxiliary
// path answers with a protocol-shaped payload; `strict-model` requests fail
// with a terminal 400 so passthrough of upstream errors can be asserted.
async fn upstream() -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let handler = any(
        move |State(hits): State<Hits>, request: axum::extract::Request| async move {
            let path = request.uri().path().to_string();
            let auth = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let content_type = request
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = axum::body::to_bytes(request.into_body(), 4 * 1024 * 1024)
                .await
                .unwrap();
            hits.0.lock().unwrap().push((path.clone(), auth.clone()));
            if auth != "Bearer good-key" {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    axum::Json(json!({"error":{"message":"bad key"}})),
                )
                    .into_response();
            }
            if path.ends_with("/embeddings") {
                return axum::Json(json!({"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.1,0.2]}],"model":"text-embedding-3-small","usage":{"prompt_tokens":3,"total_tokens":3}})).into_response();
            }
            if path.ends_with("/rerank") {
                return axum::Json(json!({"results":[{"index":0,"relevance_score":0.9}]}))
                    .into_response();
            }
            if path.ends_with("/moderations") {
                let value: Value = serde_json::from_slice(&body).unwrap();
                if value.get("model").and_then(Value::as_str) == Some("strict-model") {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(json!({"error":{"message":"bad input"}})),
                    )
                        .into_response();
                }
                return axum::Json(json!({"id":"modr_1","results":[{"flagged":false}]}))
                    .into_response();
            }
            if path.ends_with("/images/generations") {
                return axum::Json(
                    json!({"created":1,"data":[{"url":"http://image.local/1.png"}]}),
                )
                .into_response();
            }
            if path.ends_with("/audio/speech") {
                return (
                    [(axum::http::header::CONTENT_TYPE, "audio/mpeg")],
                    vec![0x49u8, 0x44, 0x33, 0x04],
                )
                    .into_response();
            }
            if path.ends_with("/audio/transcriptions") {
                assert!(
                    content_type.starts_with("multipart/form-data"),
                    "multipart content type must be forwarded: {content_type}"
                );
                let text = String::from_utf8_lossy(&body);
                assert!(text.contains("whisper-1"), "model field forwarded");
                assert!(
                    body.windows(9).any(|w| w == b"FAKEAUDIO"),
                    "file bytes forwarded"
                );
                return axum::Json(json!({"text":"hello"})).into_response();
            }
            if path.ends_with("/videos") {
                return axum::Json(json!({"id":"vid-1","status":"queued","model":"sora-2"}))
                    .into_response();
            }
            if path.ends_with("/videos/vid-1") {
                return axum::Json(json!({"id":"vid-1","status":"completed","model":"sora-2"}))
                    .into_response();
            }
            if path.ends_with("/video/generations") {
                return axum::Json(json!({"task_id":"gen-1","status":"queued"})).into_response();
            }
            if path.ends_with("/video/generations/gen-1") {
                return axum::Json(json!({"task_id":"gen-1","status":"completed"})).into_response();
            }
            if path.ends_with("/tools/fetch") {
                return axum::Json(json!({"content":"fetched page"})).into_response();
            }
            if path.ends_with("/suno/submit/music") {
                return axum::Json(json!({"code":"success","message":"","data":"song-1"}))
                    .into_response();
            }
            if path.ends_with("/suno/fetch") {
                return axum::Json(json!({"code":"success","data":[{"task_id":"song-1","status":"SUCCESS","data":[{"id":"clip-1","audio_url":"http://audio.local/1.mp3"}]}]}))
                    .into_response();
            }
            if path.ends_with("/suno/fetch/song-1") {
                return axum::Json(
                    json!({"code":"success","data":{"task_id":"song-1","status":"SUCCESS"}}),
                )
                .into_response();
            }
            if path.ends_with("/alpha/search") {
                return axum::Json(json!({"results":[{"title":"alpha"}]})).into_response();
            }
            if path.ends_with("/search") {
                return axum::Json(json!({"results":[{"title":"result"}]})).into_response();
            }
            axum::http::StatusCode::NOT_FOUND.into_response()
        },
    );
    let address = serve(Router::new().fallback(handler).with_state(hits.clone())).await;
    (address, hits)
}

async fn admin(address: SocketAddr, store: &Store) -> (reqwest::Client, String) {
    let client = reqwest::Client::new();
    let url = format!("http://{address}");
    let state = AppState::for_testing(store.clone()).await;
    client
        .post(format!("{url}/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"correct horse","token":state.bootstrap_token}))
        .send()
        .await
        .unwrap();
    let login = client
        .post(format!("{url}/api/v1/auth/login"))
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
    (client, cookie)
}

async fn client_key(store: &Store, models: Vec<String>) -> String {
    store
        .create_client_key(UpsertClientKey {
            name: "aux".into(),
            models,
            token_limit: 0,
            max_amount_micros: 0,
            rpm: 0,
            tpm: 0,
            expires_at: None,
        })
        .await
        .unwrap()
        .plaintext
}

async fn default_route(store: &Store, candidates: Vec<(i64, String)>) {
    store
        .create_route(NewRoute {
            slug: "aux-route".into(),
            name: "aux-route".into(),
            description: String::new(),
            patterns: vec!["*".into()],
            provider_id: candidates[0].0,
            model: candidates[0].1.clone(),
            is_default: true,
            list_members: false,
            candidates,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn embeddings_passthrough_records_usage_and_bodies() {
    let (upstream, hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("aux.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["text-embedding-3-small".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "text-embedding-3-small".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;
    client
        .post(format!("http://{address}/api/v1/settings"))
        .header("cookie", &cookie)
        .json(&json!({"log_bodies":true}))
        .send()
        .await
        .unwrap();

    let response = client
        .post(format!("http://{address}/v1/embeddings"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"text-embedding-3-small","input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"][0]["embedding"], json!([0.1, 0.2]));
    assert_eq!(body["usage"]["prompt_tokens"], 3);
    // The client body reaches the upstream verbatim.
    let paths = hits.0.lock().unwrap().clone();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].0, "/v1/embeddings");
    assert_eq!(paths[0].1, "Bearer good-key");

    let requests: Value = client
        .get(format!("http://{address}/api/v1/requests"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let record = &requests.as_array().unwrap()[0];
    assert_eq!(record["status"], "success");
    assert_eq!(record["model"], "text-embedding-3-small");
    assert_eq!(record["route"], "aux-route");
    assert_eq!(record["input_tokens"], 3);
    assert!(record["latency_ms"].as_i64().unwrap() > 0);
    let bodies: Value = client
        .get(format!(
            "http://{address}/api/v1/requests/{}/bodies",
            record["id"].as_str().unwrap()
        ))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        bodies["request_body"]
            .as_str()
            .unwrap()
            .contains("text-embedding-3-small")
    );
    assert!(
        bodies["response_body"]
            .as_str()
            .unwrap()
            .contains("embedding")
    );
}

#[tokio::test]
async fn auxiliary_failover_rotates_keys_and_skips_non_openai() {
    let (upstream, hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("failover.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let anthropic = store
        .create_provider(
            NewProvider {
                name: "anthropic".into(),
                protocol: ProviderProtocol::Anthropic,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["claude".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let bad = store
        .create_provider(
            NewProvider {
                name: "bad".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["rerank-model".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "bad-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    let good = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["rerank-model".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(
        &store,
        vec![
            (anthropic.id, "rerank-model".into()),
            (bad.id, "rerank-model".into()),
            (good.id, "rerank-model".into()),
        ],
    )
    .await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{address}/v1/rerank"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"rerank-model","query":"q","documents":["a"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["results"][0]["relevance_score"], 0.9);
    let paths = hits.0.lock().unwrap().clone();
    // Anthropic candidate is skipped without a call; the bad key gets one
    // real 401 before failover reaches the good provider.
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], ("/v1/rerank".into(), "Bearer bad-key".into()));
    assert_eq!(paths[1], ("/v1/rerank".into(), "Bearer good-key".into()));
}

#[tokio::test]
async fn terminal_upstream_error_passes_through_verbatim() {
    let (upstream, _hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("terminal.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["strict-model".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "strict-model".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;

    let response = client
        .post(format!("http://{address}/v1/moderations"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"strict-model","input":"x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body, json!({"error":{"message":"bad input"}}));
    let requests: Value = client
        .get(format!("http://{address}/api/v1/requests"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(requests[0]["status"], "failed");
}

#[tokio::test]
async fn multipart_transcription_forwards_model_and_file() {
    let (upstream, _hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("multipart.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["whisper-1".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "whisper-1".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"FAKEAUDIO".to_vec())
                .file_name("clip.wav")
                .mime_str("audio/wav")
                .unwrap(),
        );
    let response = client
        .post(format!("http://{address}/v1/audio/transcriptions"))
        .bearer_auth(&plaintext)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["text"], "hello");
}

#[tokio::test]
async fn speech_returns_binary_verbatim() {
    let (upstream, _hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("speech.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["tts-1".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "tts-1".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{address}/v1/audio/speech"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"tts-1","input":"hello","voice":"alloy"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "audio/mpeg"
    );
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        &[0x49, 0x44, 0x33, 0x04]
    );
}

#[tokio::test]
async fn video_task_poll_pins_submitting_channel_and_is_client_scoped() {
    let (upstream, hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("video.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    // First key is revoked: creation rotates to the second key, and the poll
    // must reuse exactly that key instead of restarting at the first one.
    let provider = store
        .create_provider(
            NewProvider {
                name: "video".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["sora-2".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "bad-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    store
        .add_provider_key(provider.id, "good-key", None, None, &keys)
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "sora-2".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let other = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{address}/v1/videos"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"sora-2","prompt":"a cat"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["id"], "vid-1");

    let response = client
        .get(format!("http://{address}/v1/videos/vid-1"))
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "completed");
    let paths = hits.0.lock().unwrap().clone();
    let polls: Vec<_> = paths
        .iter()
        .filter(|(path, _)| path == "/v1/videos/vid-1")
        .collect();
    assert_eq!(polls.len(), 1);
    assert_eq!(polls[0].1, "Bearer good-key");

    // Another client key cannot poll this task.
    let response = client
        .get(format!("http://{address}/v1/videos/vid-1"))
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn suno_submit_and_fetch_use_suno_paths() {
    let (upstream, hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("suno.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "suno".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["suno_music".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "suno_music".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{address}/suno/submit/music"))
        .bearer_auth(&plaintext)
        .json(&json!({"prompt":"a song","mv":"chirp-v4"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"], "song-1");

    let response = client
        .post(format!("http://{address}/suno/fetch"))
        .bearer_auth(&plaintext)
        .json(&json!({"ids":["song-1"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"][0]["status"], "SUCCESS");

    let response = client
        .get(format!("http://{address}/suno/fetch/song-1"))
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"]["task_id"], "song-1");

    let paths = hits.0.lock().unwrap().clone();
    let names: Vec<_> = paths.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        names,
        vec!["/suno/submit/music", "/suno/fetch", "/suno/fetch/song-1"]
    );
}

#[tokio::test]
async fn model_scope_denies_auxiliary_requests() {
    let (upstream, hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("scope.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "good".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["text-embedding-3-small".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "text-embedding-3-small".into())]).await;
    let plaintext = client_key(&store, vec!["gpt-*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let (client, cookie) = admin(address, &store).await;

    let response = client
        .post(format!("http://{address}/v1/embeddings"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"text-embedding-3-small","input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert!(hits.0.lock().unwrap().is_empty());
    let requests: Value = client
        .get(format!("http://{address}/api/v1/requests"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(requests[0]["status"], "denied");
}

#[tokio::test]
async fn search_aliases_forward_to_their_upstream_paths() {
    let (upstream, hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("search.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "search".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["search-model".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "search-model".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();
    for (client_path, upstream_path, marker) in [
        ("/v1/search", "/v1/search", "result"),
        ("/v1/alpha/search", "/v1/alpha/search", "alpha"),
        ("/v1/tools/search", "/v1/tools/search", "result"),
        ("/v1/tools/fetch", "/v1/tools/fetch", "fetched page"),
    ] {
        let response = client
            .post(format!("http://{address}{client_path}"))
            .bearer_auth(&plaintext)
            .json(&json!({"model":"search-model","query":"sepia"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{client_path}");
        let text = response.text().await.unwrap();
        assert!(text.contains(marker), "{client_path}: {text}");
        assert!(
            hits.0
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == upstream_path),
            "upstream must receive {upstream_path}"
        );
    }
}

#[tokio::test]
async fn video_generations_poll_pins_submitting_channel() {
    let (upstream, hits) = upstream().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("videogen.db"))
        .await
        .unwrap();
    let keys = KeyStore::testing();
    let provider = store
        .create_provider(
            NewProvider {
                name: "videogen".into(),
                protocol: ProviderProtocol::OpenAI,
                endpoint: format!("http://{upstream}/v1"),
                models: vec!["sora-2".into()],
                plugin_id: None,
                mode: "single".into(),
            },
            "good-key",
            &[],
            &keys,
        )
        .await
        .unwrap();
    default_route(&store, vec![(provider.id, "sora-2".into())]).await;
    let plaintext = client_key(&store, vec!["*".into()]).await;
    let other = client_key(&store, vec!["*".into()]).await;
    let address = serve(app(AppState::for_testing(store.clone()).await)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{address}/v1/video/generations"))
        .bearer_auth(&plaintext)
        .json(&json!({"model":"sora-2","prompt":"a cat"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["task_id"], "gen-1");

    let response = client
        .get(format!("http://{address}/v1/video/generations/gen-1"))
        .bearer_auth(&plaintext)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "completed");
    assert!(
        hits.0
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == "/v1/video/generations/gen-1")
    );

    let response = client
        .get(format!("http://{address}/v1/video/generations/gen-1"))
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}
