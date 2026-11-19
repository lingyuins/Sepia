//! Integration coverage for console users, role enforcement and passkey login.
//! The passkey ceremonies are driven by a hand-built ES256 software authenticator
//! (real signatures over real authData/clientDataJSON), not mocks of the verifier.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use sepia_gateway::{
    KeyStore, Store,
    server::{AppState, app},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Minimal WebAuthn software authenticator: one P-256 key per credential.
struct Authenticator {
    key: SigningKey,
    cred_id: Vec<u8>,
    counter: u32,
}

impl Authenticator {
    fn new() -> Self {
        Self {
            key: loop {
                let bytes: [u8; 32] = rand::random();
                if let Ok(key) = SigningKey::from_bytes(&bytes.into()) {
                    break key;
                }
            },
            cred_id: (0..32).map(|i| i as u8).collect(),
            counter: 0,
        }
    }

    fn cose_public_key(&self) -> Vec<u8> {
        let point = self.key.verifying_key().to_sec1_point(false);
        let map = serde_cbor_2::Value::Map(
            vec![
                (
                    serde_cbor_2::Value::Integer(1),
                    serde_cbor_2::Value::Integer(2), // kty EC2
                ),
                (
                    serde_cbor_2::Value::Integer(3),
                    serde_cbor_2::Value::Integer(-7), // alg ES256
                ),
                (
                    serde_cbor_2::Value::Integer(-1),
                    serde_cbor_2::Value::Integer(1), // crv P-256
                ),
                (
                    serde_cbor_2::Value::Integer(-2),
                    serde_cbor_2::Value::Bytes(point.x().unwrap().to_vec()),
                ),
                (
                    serde_cbor_2::Value::Integer(-3),
                    serde_cbor_2::Value::Bytes(point.y().unwrap().to_vec()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        serde_cbor_2::to_vec(&map).unwrap()
    }

    fn auth_data(&self, rp_id: &str, flags: u8, with_acd: bool) -> Vec<u8> {
        let mut data = sha256(rp_id.as_bytes()).to_vec();
        data.push(flags);
        data.extend_from_slice(&self.counter.to_be_bytes());
        if with_acd {
            data.extend_from_slice(&[0; 16]); // aaguid
            data.extend_from_slice(&(self.cred_id.len() as u16).to_be_bytes());
            data.extend_from_slice(&self.cred_id);
            data.extend_from_slice(&self.cose_public_key());
        }
        data
    }

    fn sign(&self, auth_data: &[u8], client_data: &[u8]) -> Vec<u8> {
        let mut message = auth_data.to_vec();
        message.extend_from_slice(&sha256(client_data));
        let signature: Signature = self.key.sign(&message);
        signature.to_der().as_bytes().to_vec()
    }

    /// Produce the browser-shaped JSON of navigator.credentials.create().
    fn register(&mut self, options: &Value, origin: &str) -> Value {
        let public_key = &options["publicKey"];
        let rp_id = public_key["rp"]["id"].as_str().unwrap();
        let challenge = public_key["challenge"].as_str().unwrap();
        let client_data = json!({
            "type": "webauthn.create",
            "challenge": challenge,
            "origin": origin,
            "crossOrigin": false
        })
        .to_string();
        let auth_data = self.auth_data(rp_id, 0x45, true); // UP|UV|ACD
        let attestation = serde_cbor_2::Value::Map(
            vec![
                (
                    serde_cbor_2::Value::Text("fmt".into()),
                    serde_cbor_2::Value::Text("none".into()),
                ),
                (
                    serde_cbor_2::Value::Text("attStmt".into()),
                    serde_cbor_2::Value::Map(Default::default()),
                ),
                (
                    serde_cbor_2::Value::Text("authData".into()),
                    serde_cbor_2::Value::Bytes(auth_data),
                ),
            ]
            .into_iter()
            .collect(),
        );
        self.counter += 1;
        json!({
            "id": B64.encode(&self.cred_id),
            "rawId": B64.encode(&self.cred_id),
            "type": "public-key",
            "response": {
                "attestationObject": B64.encode(serde_cbor_2::to_vec(&attestation).unwrap()),
                "clientDataJSON": B64.encode(client_data.as_bytes()),
                "transports": ["internal"]
            },
            "clientExtensionResults": {}
        })
    }

    /// Produce the browser-shaped JSON of navigator.credentials.get().
    fn authenticate(&mut self, options: &Value, origin: &str, user_handle: &[u8]) -> Value {
        let public_key = &options["publicKey"];
        let rp_id = public_key["rpId"].as_str().unwrap();
        let challenge = public_key["challenge"].as_str().unwrap();
        let client_data = json!({
            "type": "webauthn.get",
            "challenge": challenge,
            "origin": origin,
            "crossOrigin": false
        })
        .to_string();
        self.counter += 1;
        let auth_data = self.auth_data(rp_id, 0x05, false); // UP|UV
        let signature = self.sign(&auth_data, client_data.as_bytes());
        json!({
            "id": B64.encode(&self.cred_id),
            "rawId": B64.encode(&self.cred_id),
            "type": "public-key",
            "response": {
                "authenticatorData": B64.encode(&auth_data),
                "clientDataJSON": B64.encode(client_data.as_bytes()),
                "signature": B64.encode(&signature),
                "userHandle": B64.encode(user_handle)
            },
            "clientExtensionResults": {}
        })
    }
}

async fn harness() -> (
    String,
    reqwest::Client,
    tempfile::TempDir,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&directory.path().join("users.db"))
        .await
        .unwrap();
    let state = AppState {
        store: store.clone(),
        keys: KeyStore::testing(),
        http: reqwest::Client::new(),
        bootstrap_token: "bootstrap".into(),
        data_directory: directory.path().to_path_buf(),
        frontend_directory: "frontend/dist".into(),
        snapshots: std::sync::Arc::new(
            sepia_gateway::snapshot::Snapshots::load(&store)
                .await
                .unwrap(),
        ),
        key_health: Default::default(),
        secure_cookie: false,
        metadata_source: sepia_gateway::metadata::DEFAULT_SOURCE.into(),
        plugins: sepia_gateway::plugin::shared(
            sepia_gateway::plugin::PluginRuntime::new().unwrap(),
        ),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
    });
    (
        format!("http://{address}"),
        reqwest::Client::new(),
        directory,
        server,
    )
}

async fn bootstrap(client: &reqwest::Client, base: &str, password: &str) {
    let response = client
        .post(format!("{base}/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":password,"token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

async fn login_cookie(
    client: &reqwest::Client,
    base: &str,
    username: &str,
    password: &str,
) -> String {
    let response = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({"username":username,"password":password}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "login failed for {username}");
    response
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

#[tokio::test]
async fn bootstrap_accepts_eight_character_password_and_rejects_seven() {
    let (base, client, _directory, _server) = harness().await;
    let short = client
        .post(format!("{base}/api/v1/auth/bootstrap"))
        .json(&json!({"username":"admin","password":"1234567","token":"bootstrap"}))
        .send()
        .await
        .unwrap();
    assert_eq!(short.status(), 400);
    let body: Value = short.json().await.unwrap();
    assert_eq!(
        body["error"]["message"],
        "username and a password of at least 8 characters required"
    );
    bootstrap(&client, &base, "12345678").await;
    let cookie = login_cookie(&client, &base, "admin", "12345678").await;
    let session: Value = client
        .get(format!("{base}/api/v1/auth/session"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(session["username"], "admin");
    assert_eq!(session["role"], "admin");
    assert_eq!(session["can_edit"], true);
}

#[tokio::test]
async fn roles_enforce_admin_editor_viewer_boundaries() {
    let (base, client, _directory, _server) = harness().await;
    bootstrap(&client, &base, "correct horse").await;
    let admin = login_cookie(&client, &base, "admin", "correct horse").await;
    let provider: Value = client
        .post(format!("{base}/api/v1/providers"))
        .header("cookie", &admin)
        .json(&json!({
            "name":"Role fixture","protocol":"openai",
            "endpoint":"http://127.0.0.1:9/v1","models":["model"],
            "secret":"upstream-secret","group_id":"official","enabled":true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let provider_id = provider["id"].as_i64().unwrap();
    for (username, role) in [("editor", "editor"), ("viewer", "viewer")] {
        let created = client
            .post(format!("{base}/api/v1/users"))
            .header("cookie", &admin)
            .json(&json!({"username":username,"password":"password123","role":role}))
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 200);
    }
    let editor = login_cookie(&client, &base, "editor", "password123").await;
    let viewer = login_cookie(&client, &base, "viewer", "password123").await;

    // Viewer: reads allowed, every mutation rejected.
    let read = client
        .get(format!("{base}/api/v1/providers"))
        .header("cookie", &viewer)
        .send()
        .await
        .unwrap();
    assert_eq!(read.status(), 200);
    let write = client
        .post(format!("{base}/api/v1/routes"))
        .header("cookie", &viewer)
        .json(&json!({"slug":"x","name":"x","description":"","patterns":[],"candidates":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(write.status(), 403);
    let viewer_users = client
        .post(format!("{base}/api/v1/users"))
        .header("cookie", &viewer)
        .json(&json!({"username":"x","password":"password123","role":"viewer"}))
        .send()
        .await
        .unwrap();
    assert_eq!(viewer_users.status(), 403);

    // Editor: ordinary management allowed, admin-only surfaces rejected.
    let editor_route = client
        .post(format!("{base}/api/v1/routes"))
        .header("cookie", &editor)
        .json(&json!({"slug":"editor-route","name":"Editor route","description":"","patterns":["editor-route"],"candidates":[[provider_id,"model"]]}))
        .send()
        .await
        .unwrap();
    assert_eq!(editor_route.status(), 200);
    for (method, path, body) in [
        (
            "POST",
            "/api/v1/users",
            json!({"username":"denied","password":"password123","role":"viewer"}),
        ),
        ("POST", "/api/v1/settings", json!({"log_bodies":true})),
        ("POST", "/api/v1/keys", json!({"name":"denied","models":[]})),
        ("POST", "/api/v1/plugins/install", json!({})),
        ("POST", "/api/v1/update", json!({})),
    ] {
        let denied = client
            .request(method.parse().unwrap(), format!("{base}{path}"))
            .header("cookie", &editor)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 403, "editor must not access {path}");
    }

    // Last administrator cannot be demoted or deleted.
    let users: Value = client
        .get(format!("{base}/api/v1/users"))
        .header("cookie", &admin)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let admin_id = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["username"] == "admin")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let demote = client
        .post(format!("{base}/api/v1/users/{admin_id}/role"))
        .header("cookie", &admin)
        .json(&json!({"role":"viewer"}))
        .send()
        .await
        .unwrap();
    assert_eq!(demote.status(), 400);
    let delete = client
        .delete(format!("{base}/api/v1/users/{admin_id}"))
        .header("cookie", &admin)
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 400);

    // Role change takes effect on the next request; deletion kills sessions.
    let viewer_id = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["username"] == "viewer")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let promote = client
        .post(format!("{base}/api/v1/users/{viewer_id}/role"))
        .header("cookie", &admin)
        .json(&json!({"role":"editor"}))
        .send()
        .await
        .unwrap();
    assert_eq!(promote.status(), 200);
    let now_allowed = client
        .post(format!("{base}/api/v1/routes"))
        .header("cookie", &viewer)
        .json(&json!({"slug":"viewer-route","name":"Viewer route","description":"","patterns":["viewer-route"],"candidates":[[provider_id,"model"]]}))
        .send()
        .await
        .unwrap();
    assert_eq!(now_allowed.status(), 200);
    let removed = client
        .delete(format!("{base}/api/v1/users/{viewer_id}"))
        .header("cookie", &admin)
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 200);
    let dead_session = client
        .get(format!("{base}/api/v1/providers"))
        .header("cookie", &viewer)
        .send()
        .await
        .unwrap();
    assert_eq!(dead_session.status(), 401);
    let dead_login = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({"username":"viewer","password":"password123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(dead_login.status(), 401);
}

#[tokio::test]
async fn passkey_registration_and_discoverable_login_issue_real_sessions() {
    let (base, client, directory, _server) = harness().await;
    bootstrap(&client, &base, "correct horse").await;
    let cookie = login_cookie(&client, &base, "admin", "correct horse").await;
    let origin = base.clone();

    let options: Value = client
        .post(format!("{base}/api/v1/auth/passkey/register/options"))
        .header("cookie", &cookie)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let challenge_token = options["challenge"].as_str().unwrap().to_string();
    assert_eq!(
        options["options"]["publicKey"]["authenticatorSelection"]["residentKey"],
        "required"
    );
    let user_handle = B64
        .decode(
            options["options"]["publicKey"]["user"]["id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let mut authenticator = Authenticator::new();
    let credential = authenticator.register(&options["options"], &origin);
    let registered = client
        .post(format!("{base}/api/v1/auth/passkey/register/finish"))
        .header("cookie", &cookie)
        .json(&json!({"challenge":challenge_token,"name":"Laptop","credential":credential}))
        .send()
        .await
        .unwrap();
    assert_eq!(registered.status(), 200);
    let registered: Value = registered.json().await.unwrap();
    assert_eq!(registered["name"], "Laptop");

    // The challenge is one-time: replaying it must fail.
    let replay = client
        .post(format!("{base}/api/v1/auth/passkey/register/finish"))
        .header("cookie", &cookie)
        .json(&json!({"challenge":challenge_token,"name":"Replay","credential":credential}))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 400);

    // Usernameless login with the discoverable credential.
    let login_options: Value = client
        .post(format!("{base}/api/v1/auth/passkey/login/options"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let assertion = authenticator.authenticate(&login_options["options"], &origin, &user_handle);
    let finished = client
        .post(format!("{base}/api/v1/auth/passkey/login/finish"))
        .json(&json!({"challenge":login_options["challenge"],"credential":assertion}))
        .send()
        .await
        .unwrap();
    assert_eq!(finished.status(), 200);
    let passkey_cookie = finished
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let finished_body: Value = finished.json().await.unwrap();
    assert_eq!(finished_body["username"], "admin");
    assert_eq!(finished_body["role"], "admin");
    let session: Value = client
        .get(format!("{base}/api/v1/auth/session"))
        .header("cookie", &passkey_cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(session["username"], "admin");

    // A forged signature must be rejected.
    let second_options: Value = client
        .post(format!("{base}/api/v1/auth/passkey/login/options"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut forged = authenticator.authenticate(&second_options["options"], &origin, &user_handle);
    forged["response"]["signature"] = json!(B64.encode([0u8; 70]));
    let rejected = client
        .post(format!("{base}/api/v1/auth/passkey/login/finish"))
        .json(&json!({"challenge":second_options["challenge"],"credential":forged}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 401);

    // Counter advanced and was persisted (second real assertion still works).
    let third_options: Value = client
        .post(format!("{base}/api/v1/auth/passkey/login/options"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let assertion = authenticator.authenticate(&third_options["options"], &origin, &user_handle);
    let again = client
        .post(format!("{base}/api/v1/auth/passkey/login/finish"))
        .json(&json!({"challenge":third_options["challenge"],"credential":assertion}))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 200);

    // Listing and deleting the passkey revokes future logins.
    let listed: Value = client
        .get(format!("{base}/api/v1/passkeys"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let passkey_id = listed["passkeys"][0]["id"].as_i64().unwrap();
    assert!(listed["passkeys"][0]["last_used_at"].is_string());
    let deleted = client
        .delete(format!("{base}/api/v1/passkeys/{passkey_id}"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);
    let fourth_options: Value = client
        .post(format!("{base}/api/v1/auth/passkey/login/options"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let assertion = authenticator.authenticate(&fourth_options["options"], &origin, &user_handle);
    let orphaned = client
        .post(format!("{base}/api/v1/auth/passkey/login/finish"))
        .json(&json!({"challenge":fourth_options["challenge"],"credential":assertion}))
        .send()
        .await
        .unwrap();
    assert_eq!(orphaned.status(), 401);
    let _ = directory;
}
