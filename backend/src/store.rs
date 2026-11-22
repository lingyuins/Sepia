use crate::{
    ClientKey,
    crypto::{KeyStore, password_hash, random_token, token_hash, verify_password},
};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{path::Path, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderProtocol {
    OpenAI,
    Anthropic,
    Gemini,
    #[serde(rename = "openai-responses")]
    OpenAIResponses,
}
impl ProviderProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::OpenAIResponses => "openai-responses",
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "openai" => Ok(Self::OpenAI),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            "openai-responses" => Ok(Self::OpenAIResponses),
            _ => Err("unknown provider protocol".into()),
        }
    }
}
fn provider_key_models(
    key_scopes: &[(String, Vec<String>)],
    secret: &str,
) -> Result<Option<String>, String> {
    key_scopes
        .iter()
        .find(|(key, _)| key == secret)
        .map(|(_, models)| serde_json::to_string(models))
        .transpose()
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone)]
pub struct NewProvider {
    pub name: String,
    pub protocol: ProviderProtocol,
    pub endpoint: String,
    pub models: Vec<String>,
    pub plugin_id: Option<String>,
    /// `single` (one address) or `unified` (several addresses, one model set).
    pub mode: String,
}
#[derive(Debug, Clone)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub endpoint: String,
    pub models: Vec<String>,
    pub key_count: i64,
    pub group_id: String,
    pub enabled: bool,
    pub plugin_id: Option<String>,
}
#[derive(Debug, Clone)]
pub struct ProviderEndpoint {
    pub id: i64,
    pub endpoint: String,
}
#[derive(Debug, Clone)]
pub struct ProviderKeyInfo {
    pub id: i64,
    pub models: Option<Vec<String>>,
    pub endpoint_id: Option<i64>,
}
#[derive(Debug, Clone)]
pub struct NewRoute {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub patterns: Vec<String>,
    pub provider_id: i64,
    pub model: String,
    pub is_default: bool,
    pub list_members: bool,
    pub candidates: Vec<(i64, String)>,
}
#[derive(Debug, Clone)]
pub struct Route {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub patterns: Vec<String>,
    pub is_default: bool,
    pub list_members: bool,
    pub enabled: bool,
    pub candidates: Vec<(i64, String)>,
    /// Built-in candidate ordering algorithm (see `crate::strategy`).
    pub strategy: String,
    /// Per-candidate weights, parallel to `candidates`; default 1.
    pub weights: Vec<i64>,
}
#[derive(Debug, Clone)]
pub struct UpsertClientKey {
    pub name: String,
    pub models: Vec<String>,
    pub token_limit: i64,
    pub max_amount_micros: i64,
    pub rpm: i64,
    pub tpm: i64,
    pub expires_at: Option<String>,
    pub secret: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ChannelGroup {
    pub id: String,
    pub name: String,
    pub description: String,
}
pub struct CreatedClientKey {
    pub id: i64,
    pub plaintext: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    Editor,
    Viewer,
}
impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Editor => "editor",
            Self::Viewer => "viewer",
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "admin" => Ok(Self::Admin),
            "editor" => Ok(Self::Editor),
            "viewer" => Ok(Self::Viewer),
            _ => Err("unknown user role".into()),
        }
    }
    pub fn can_edit(self) -> bool {
        matches!(self, Self::Admin | Self::Editor)
    }
}
#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: UserRole,
    pub created_at: String,
}
#[derive(Debug, Clone)]
pub struct PasskeyInfo {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}
#[derive(Debug, Clone)]
pub struct AdminSession {
    pub username: String,
    pub role: UserRole,
}
#[derive(Debug, Clone)]
pub struct PluginRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub status: String,
    pub manifest: crate::plugin::Manifest,
    pub config: Value,
    pub wasm: Vec<u8>,
    pub ui_bundle: Value,
    pub sha256: String,
    pub installed_at: String,
    pub updated_at: String,
}
#[derive(Clone)]
pub struct Store {
    pub(crate) pool: SqlitePool,
}
impl Store {
    pub async fn connect(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
            .map_err(|e| e.to_string())?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .map_err(|e| e.to_string())?;
        crate::MIGRATIONS
            .run(&pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self { pool })
    }
    pub async fn verify_keystore(&self, keys: &KeyStore) -> Result<(), String> {
        let marker_table = sqlx::query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'keystore_metadata'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        if marker_table.is_none() {
            crate::MIGRATIONS
                .run(&self.pool)
                .await
                .map_err(|error| error.to_string())?;
        }
        let marker = sqlx::query("SELECT nonce, ciphertext FROM keystore_metadata WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| error.to_string())?;
        match marker {
            Some(marker) => {
                let nonce: String = sqlx::Row::get(&marker, "nonce");
                let ciphertext: String = sqlx::Row::get(&marker, "ciphertext");
                match keys.decrypt(&nonce, &ciphertext) {
                    Ok(value) if &*value == "sepia-key-check-v1" => Ok(()),
                    _ => Err("master key cannot decrypt existing credentials".into()),
                }
            }
            None => {
                let (nonce, ciphertext) = keys.encrypt("sepia-key-check-v1")?;
                sqlx::query(
                    "INSERT INTO keystore_metadata (id, nonce, ciphertext) VALUES (1, ?, ?)",
                )
                .bind(nonce)
                .bind(ciphertext)
                .execute(&self.pool)
                .await
                .map_err(|error| error.to_string())?;
                Ok(())
            }
        }
    }
    pub async fn initialized(&self) -> Result<bool, String> {
        Ok(sqlx::query("SELECT 1 FROM admins LIMIT 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .is_some())
    }
    pub async fn bootstrap(
        &self,
        username: &str,
        password: &str,
        supplied_token: &str,
        expected_token: &str,
    ) -> Result<(), String> {
        if self.initialized().await? {
            return Ok(());
        }
        if supplied_token != expected_token {
            return Err("invalid bootstrap token".into());
        }
        if username.trim().is_empty() || password.len() < 8 {
            return Err("username and a password of at least 8 characters required".into());
        }
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        if sqlx::query("SELECT 1 FROM admins LIMIT 1")
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Ok(());
        }
        let hash = password_hash(password)?;
        let created_at = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO admins (id, username, password_hash, created_at) VALUES (1, ?, ?, ?)",
        )
        .bind(username)
        .bind(&hash)
        .bind(&created_at)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO users (username, password_hash, role, webauthn_user_id, created_at) VALUES (?, ?, 'admin', ?, ?)",
        )
        .bind(username)
        .bind(&hash)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&created_at)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
        transaction.commit().await.map_err(|e| e.to_string())
    }
    pub async fn login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(String, UserRole), String> {
        let row = sqlx::query("SELECT password_hash, role FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("invalid credentials")?;
        if !verify_password(row.get("password_hash"), password) {
            return Err("invalid credentials".into());
        }
        let role = UserRole::parse(&row.get::<String, _>("role"))?;
        Ok((self.login_token(username).await?, role))
    }
    pub async fn session(&self, token: &str) -> Result<Option<AdminSession>, String> {
        let Some(row) = sqlx::query(
            "SELECT sessions.username, sessions.expires_at, users.role FROM sessions JOIN users ON users.username = sessions.username WHERE sessions.token_hash = ?",
        )
        .bind(token_hash(token))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        if DateTime::parse_from_rfc3339(&row.get::<String, _>("expires_at"))
            .map_err(|e| e.to_string())?
            < Utc::now()
        {
            return Ok(None);
        }
        Ok(Some(AdminSession {
            username: row.get("username"),
            role: UserRole::parse(&row.get::<String, _>("role"))?,
        }))
    }
    pub async fn logout(&self, token: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash(token))
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn users(&self) -> Result<Vec<User>, String> {
        let rows = sqlx::query("SELECT id, username, role, created_at FROM users ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        rows.iter()
            .map(|row| {
                Ok(User {
                    id: row.get("id"),
                    username: row.get("username"),
                    role: UserRole::parse(&row.get::<String, _>("role"))?,
                    created_at: row.get("created_at"),
                })
            })
            .collect()
    }
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: UserRole,
    ) -> Result<User, String> {
        let username = username.trim();
        if username.is_empty() || password.len() < 8 {
            return Err("username and a password of at least 8 characters required".into());
        }
        if sqlx::query("SELECT 1 FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("username already exists".into());
        }
        let created_at = Utc::now().to_rfc3339();
        let id = sqlx::query(
            "INSERT INTO users (username, password_hash, role, webauthn_user_id, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(username)
        .bind(password_hash(password)?)
        .bind(role.as_str())
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid();
        Ok(User {
            id,
            username: username.into(),
            role,
            created_at,
        })
    }
    async fn last_admin_guard(&self, id: i64) -> Result<(), String> {
        let row = sqlx::query(
            "SELECT role, (SELECT COUNT(*) FROM users WHERE role = 'admin') AS admins FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("user not found")?;
        if row.get::<String, _>("role") == "admin" && row.get::<i64, _>("admins") <= 1 {
            return Err("cannot remove the last administrator".into());
        }
        Ok(())
    }
    pub async fn set_user_role(&self, id: i64, role: UserRole) -> Result<(), String> {
        if role != UserRole::Admin {
            self.last_admin_guard(id).await?;
        }
        let result = sqlx::query("UPDATE users SET role = ? WHERE id = ?")
            .bind(role.as_str())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("user not found".into());
        }
        Ok(())
    }
    pub async fn reset_user_password(&self, id: i64, password: &str) -> Result<(), String> {
        if password.len() < 8 {
            return Err("a password of at least 8 characters required".into());
        }
        let result = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(password_hash(password)?)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("user not found".into());
        }
        Ok(())
    }
    pub async fn delete_user(&self, id: i64) -> Result<(), String> {
        self.last_admin_guard(id).await?;
        let username: String = sqlx::query("SELECT username FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("user not found")?
            .get("username");
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM sessions WHERE username = ?")
            .bind(&username)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        transaction.commit().await.map_err(|e| e.to_string())
    }
    pub async fn user_by_name(&self, username: &str) -> Result<Option<User>, String> {
        let Some(row) =
            sqlx::query("SELECT id, username, role, created_at FROM users WHERE username = ?")
                .bind(username)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        Ok(Some(User {
            id: row.get("id"),
            username: row.get("username"),
            role: UserRole::parse(&row.get::<String, _>("role"))?,
            created_at: row.get("created_at"),
        }))
    }
    pub async fn passkey_user(&self, id: i64) -> Result<(String, String, Vec<Vec<u8>>), String> {
        let row = sqlx::query("SELECT username, webauthn_user_id FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("user not found")?;
        let username: String = row.get("username");
        let handle: String = row.get("webauthn_user_id");
        let credentials =
            sqlx::query("SELECT credential FROM webauthn_credentials WHERE user_id = ?")
                .bind(id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        for row in &credentials {
            let value: serde_json::Value =
                serde_json::from_str(&row.get::<String, _>("credential"))
                    .map_err(|e| e.to_string())?;
            let encoded = value["cred_id"]
                .as_str()
                .ok_or("stored passkey is malformed")?;
            ids.push(
                base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, encoded)
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok((username, handle, ids))
    }
    pub async fn passkey_user_by_handle(
        &self,
        handle: &str,
    ) -> Result<Option<(User, Vec<String>)>, String> {
        let Some(row) = sqlx::query(
            "SELECT id, username, role, created_at FROM users WHERE webauthn_user_id = ?",
        )
        .bind(handle)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let user = User {
            id: row.get("id"),
            username: row.get("username"),
            role: UserRole::parse(&row.get::<String, _>("role"))?,
            created_at: row.get("created_at"),
        };
        let rows = sqlx::query("SELECT credential FROM webauthn_credentials WHERE user_id = ?")
            .bind(user.id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some((
            user,
            rows.iter().map(|row| row.get("credential")).collect(),
        )))
    }
    pub async fn passkeys(&self, user_id: i64) -> Result<Vec<PasskeyInfo>, String> {
        let rows = sqlx::query(
            "SELECT id, name, created_at, last_used_at FROM webauthn_credentials WHERE user_id = ? ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|row| PasskeyInfo {
                id: row.get("id"),
                name: row.get("name"),
                created_at: row.get("created_at"),
                last_used_at: row.get("last_used_at"),
            })
            .collect())
    }
    pub async fn add_passkey(
        &self,
        user_id: i64,
        name: &str,
        credential: &str,
    ) -> Result<PasskeyInfo, String> {
        let created_at = Utc::now().to_rfc3339();
        let id = sqlx::query(
            "INSERT INTO webauthn_credentials (user_id, name, credential, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(name)
        .bind(credential)
        .bind(&created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid();
        Ok(PasskeyInfo {
            id,
            name: name.into(),
            created_at,
            last_used_at: None,
        })
    }
    pub async fn delete_passkey(&self, user_id: i64, id: i64) -> Result<(), String> {
        let result = sqlx::query("DELETE FROM webauthn_credentials WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("passkey not found".into());
        }
        Ok(())
    }
    pub async fn update_passkey(
        &self,
        user_id: i64,
        cred_id: &[u8],
        credential: &str,
    ) -> Result<(), String> {
        let result = sqlx::query(
            "UPDATE webauthn_credentials SET credential = ?, last_used_at = ? WHERE user_id = ? AND json_extract(credential, '$.cred_id') = ?",
        )
        .bind(credential)
        .bind(Utc::now().to_rfc3339())
        .bind(user_id)
        .bind(base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            cred_id,
        ))
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("passkey not found".into());
        }
        Ok(())
    }
    pub async fn create_passkey_challenge(
        &self,
        kind: &str,
        user_id: Option<i64>,
        state: &str,
    ) -> Result<String, String> {
        // Expired rows are never taken again; drop them so unauthenticated
        // login-options calls cannot grow the table without bound.
        let _ = sqlx::query("DELETE FROM webauthn_challenges WHERE expires_at < ?")
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await;
        let token = random_token("sepia_wa_");
        sqlx::query(
            "INSERT INTO webauthn_challenges (token_hash, kind, user_id, state, expires_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(token_hash(&token))
        .bind(kind)
        .bind(user_id)
        .bind(state)
        .bind((Utc::now() + chrono::Duration::minutes(5)).to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(token)
    }
    pub async fn take_passkey_challenge(
        &self,
        token: &str,
        kind: &str,
    ) -> Result<Option<(Option<i64>, String)>, String> {
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        let Some(row) = sqlx::query(
            "SELECT kind, user_id, state, expires_at FROM webauthn_challenges WHERE token_hash = ?",
        )
        .bind(token_hash(token))
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        sqlx::query("DELETE FROM webauthn_challenges WHERE token_hash = ?")
            .bind(token_hash(token))
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        transaction.commit().await.map_err(|e| e.to_string())?;
        if row.get::<String, _>("kind") != kind
            || DateTime::parse_from_rfc3339(&row.get::<String, _>("expires_at"))
                .map_err(|e| e.to_string())?
                < Utc::now()
        {
            return Ok(None);
        }
        Ok(Some((row.get("user_id"), row.get("state"))))
    }
    pub async fn login_token(&self, username: &str) -> Result<String, String> {
        let token = random_token("sepia_session_");
        sqlx::query("INSERT INTO sessions (token_hash, username, expires_at) VALUES (?, ?, ?)")
            .bind(token_hash(&token))
            .bind(username)
            .bind((Utc::now() + chrono::Duration::hours(12)).to_rfc3339())
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(token)
    }
    pub async fn create_provider(
        &self,
        input: NewProvider,
        secret: &str,
        key_scopes: &[(String, Vec<String>)],
        key_store: &KeyStore,
    ) -> Result<Provider, String> {
        if input.name.trim().is_empty() || input.models.is_empty() {
            return Err("provider name and models required".into());
        }
        if input.plugin_id.is_none() {
            let parsed = url::Url::parse(&input.endpoint).map_err(|e| e.to_string())?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err("provider endpoint must be HTTP(S)".into());
            }
        }
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO channel_groups (id, name, description) VALUES ('official', 'Official', 'Built-in provider group') ON CONFLICT(id) DO NOTHING")
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        let endpoint = input.endpoint.trim_end_matches('/');
        let id = sqlx::query(
            "INSERT INTO providers (name, protocol, endpoint, plugin_id, mode) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(input.name)
        .bind(input.protocol.as_str())
        .bind(endpoint)
        .bind(input.plugin_id)
        .bind(input.mode)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid();
        let endpoint_id = sqlx::query(
            "INSERT INTO provider_endpoints (provider_id, endpoint, position) VALUES (?, ?, 0)",
        )
        .bind(id)
        .bind(endpoint)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid();
        for model in input.models {
            sqlx::query("INSERT INTO provider_models (provider_id, model) VALUES (?, ?)")
                .bind(id)
                .bind(model)
                .execute(&mut *transaction)
                .await
                .map_err(|e| e.to_string())?;
        }
        let (nonce, ciphertext) = key_store.encrypt(secret)?;
        sqlx::query("INSERT INTO provider_keys (provider_id, nonce, ciphertext, models, endpoint_id) VALUES (?, ?, ?, ?, ?)")
            .bind(id)
            .bind(nonce)
            .bind(ciphertext)
            .bind(provider_key_models(key_scopes, secret)?)
            .bind(endpoint_id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        transaction.commit().await.map_err(|e| e.to_string())?;
        self.providers()
            .await?
            .into_iter()
            .find(|provider| provider.id == id)
            .ok_or("provider not saved".into())
    }
    pub async fn providers(&self) -> Result<Vec<Provider>, String> {
        let rows = sqlx::query("SELECT p.id, p.name, p.protocol, p.endpoint, p.group_id, p.enabled, p.plugin_id, COUNT(k.id) key_count FROM providers p LEFT JOIN provider_keys k ON k.provider_id = p.id AND k.disabled = 0 GROUP BY p.id ORDER BY p.id").fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        let mut providers = Vec::new();
        for row in rows {
            let id = row.get("id");
            let models = sqlx::query(
                "SELECT model FROM provider_models WHERE provider_id = ? ORDER BY model",
            )
            .bind(id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| r.get("model"))
            .collect();
            providers.push(Provider {
                id,
                name: row.get("name"),
                protocol: ProviderProtocol::parse(row.get("protocol"))?,
                endpoint: row.get("endpoint"),
                models,
                key_count: row.get("key_count"),
                group_id: row.get("group_id"),
                enabled: row.get("enabled"),
                plugin_id: row.get("plugin_id"),
            });
        }
        Ok(providers)
    }
    pub async fn provider_mode(&self, provider_id: i64) -> Result<String, String> {
        Ok(sqlx::query("SELECT mode FROM providers WHERE id = ?")
            .bind(provider_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("provider not found")?
            .get("mode"))
    }
    pub async fn provider_endpoints(
        &self,
        provider_id: i64,
    ) -> Result<Vec<ProviderEndpoint>, String> {
        let rows = sqlx::query("SELECT id, endpoint FROM provider_endpoints WHERE provider_id = ? ORDER BY position, id").bind(provider_id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| ProviderEndpoint {
                id: row.get("id"),
                endpoint: row.get("endpoint"),
            })
            .collect())
    }
    /// Reconcile a unified provider's address list by position: existing rows
    /// keep their id (and therefore their keys), new addresses are appended,
    /// removed addresses cascade-delete their keys. The first address stays
    /// mirrored in `providers.endpoint` for discovery and plugin descriptors.
    pub async fn sync_provider_endpoints(
        &self,
        provider_id: i64,
        endpoints: &[String],
    ) -> Result<Vec<ProviderEndpoint>, String> {
        if endpoints.is_empty() {
            return Err("at least one endpoint required".into());
        }
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        let existing = sqlx::query(
            "SELECT id FROM provider_endpoints WHERE provider_id = ? ORDER BY position, id",
        )
        .bind(provider_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
        for (index, endpoint) in endpoints.iter().enumerate() {
            let endpoint = endpoint.trim_end_matches('/');
            if let Some(row) = existing.get(index) {
                sqlx::query(
                    "UPDATE provider_endpoints SET endpoint = ?, position = ? WHERE id = ?",
                )
                .bind(endpoint)
                .bind(index as i64)
                .bind(row.get::<i64, _>("id"))
                .execute(&mut *transaction)
                .await
                .map_err(|e| e.to_string())?;
            } else {
                sqlx::query("INSERT INTO provider_endpoints (provider_id, endpoint, position) VALUES (?, ?, ?)")
                    .bind(provider_id)
                    .bind(endpoint)
                    .bind(index as i64)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        for row in existing.iter().skip(endpoints.len()) {
            sqlx::query("DELETE FROM provider_endpoints WHERE id = ?")
                .bind(row.get::<i64, _>("id"))
                .execute(&mut *transaction)
                .await
                .map_err(|e| e.to_string())?;
        }
        sqlx::query("UPDATE providers SET endpoint = ? WHERE id = ?")
            .bind(endpoints[0].trim_end_matches('/'))
            .bind(provider_id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        transaction.commit().await.map_err(|e| e.to_string())?;
        self.provider_endpoints(provider_id).await
    }
    pub async fn append_provider_endpoint(
        &self,
        provider_id: i64,
        endpoint: &str,
    ) -> Result<i64, String> {
        let position = sqlx::query(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM provider_endpoints WHERE provider_id = ?",
        )
        .bind(provider_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .get::<i64, _>(0);
        Ok(sqlx::query(
            "INSERT INTO provider_endpoints (provider_id, endpoint, position) VALUES (?, ?, ?)",
        )
        .bind(provider_id)
        .bind(endpoint.trim_end_matches('/'))
        .bind(position)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid())
    }
    pub async fn add_provider_key(
        &self,
        provider_id: i64,
        secret: &str,
        models: Option<&[String]>,
        endpoint_id: Option<i64>,
        key_store: &KeyStore,
    ) -> Result<i64, String> {
        let endpoint_id = match endpoint_id {
            Some(id) => Some(id),
            None => self
                .provider_endpoints(provider_id)
                .await?
                .first()
                .map(|endpoint| endpoint.id),
        };
        let (nonce, ciphertext) = key_store.encrypt(secret)?;
        Ok(sqlx::query(
            "INSERT INTO provider_keys (provider_id, nonce, ciphertext, models, endpoint_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(provider_id)
        .bind(nonce)
        .bind(ciphertext)
        .bind(models.map(serde_json::to_string).transpose().map_err(|e| e.to_string())?)
        .bind(endpoint_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid())
    }
    pub async fn provider_keys(&self, provider_id: i64) -> Result<Vec<ProviderKeyInfo>, String> {
        let rows = sqlx::query("SELECT id, models, endpoint_id FROM provider_keys WHERE provider_id = ? AND disabled = 0 ORDER BY id").bind(provider_id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                let models: Option<String> = row.get("models");
                Ok(ProviderKeyInfo {
                    id: row.get("id"),
                    endpoint_id: row.get("endpoint_id"),
                    models: models
                        .map(|models| serde_json::from_str(&models))
                        .transpose()
                        .map_err(|e| e.to_string())?,
                })
            })
            .collect()
    }
    pub async fn model_prices(&self) -> Result<Vec<(i64, String, i64, i64)>, String> {
        let rows = sqlx::query("SELECT provider_id, model, input_price_micros, output_price_micros FROM provider_models WHERE input_price_micros > 0 AND output_price_micros > 0")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get("provider_id"),
                    row.get("model"),
                    row.get("input_price_micros"),
                    row.get("output_price_micros"),
                )
            })
            .collect())
    }
    pub async fn set_provider_key_models(
        &self,
        provider_id: i64,
        key_id: i64,
        models: Option<&[String]>,
    ) -> Result<(), String> {
        let result =
            sqlx::query("UPDATE provider_keys SET models = ? WHERE id = ? AND provider_id = ?")
                .bind(
                    models
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|e| e.to_string())?,
                )
                .bind(key_id)
                .bind(provider_id)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("provider key not found".into());
        }
        Ok(())
    }
    pub async fn upstream_key_for_attempt(
        &self,
        provider_id: i64,
        endpoint_id: Option<i64>,
        model: &str,
        start_index: usize,
        cooldowns: &std::collections::HashMap<i64, chrono::DateTime<Utc>>,
        key_store: &KeyStore,
    ) -> Result<Option<(i64, String)>, String> {
        let rows = match endpoint_id {
            Some(endpoint_id) => sqlx::query("SELECT id, nonce, ciphertext, models FROM provider_keys WHERE provider_id = ? AND endpoint_id = ? AND disabled = 0 ORDER BY id").bind(provider_id).bind(endpoint_id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?,
            None => sqlx::query("SELECT id, nonce, ciphertext, models FROM provider_keys WHERE provider_id = ? AND disabled = 0 ORDER BY id").bind(provider_id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?,
        };
        // A scoped key only serves requests for models inside its scope; an
        // unscoped (NULL) key serves every provider model.
        let rows: Vec<_> = rows
            .into_iter()
            .filter(|row| {
                let models: Option<String> = row.get("models");
                match models {
                    None => true,
                    Some(models) => serde_json::from_str::<Vec<String>>(&models)
                        .is_ok_and(|models| models.iter().any(|item| item == model)),
                }
            })
            .collect();
        if rows.is_empty() {
            return Ok(None);
        }
        for offset in 0..rows.len() {
            let row = &rows[(start_index + offset) % rows.len()];
            let id = row.get("id");
            if cooldowns.get(&id).is_some_and(|until| *until > Utc::now()) {
                continue;
            }
            return Ok(Some((
                id,
                key_store
                    .decrypt(row.get("nonce"), row.get("ciphertext"))?
                    .to_string(),
            )));
        }
        Ok(None)
    }
    pub async fn upstream_key_by_id(
        &self,
        provider_id: i64,
        key_id: i64,
        key_store: &KeyStore,
    ) -> Result<String, String> {
        let row = sqlx::query("SELECT nonce, ciphertext FROM provider_keys WHERE id = ? AND provider_id = ? AND disabled = 0").bind(key_id).bind(provider_id).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?.ok_or("provider key not found")?;
        Ok(key_store
            .decrypt(row.get("nonce"), row.get("ciphertext"))?
            .to_string())
    }
    pub async fn upstream_key(
        &self,
        provider_id: i64,
        key_store: &KeyStore,
    ) -> Result<String, String> {
        let row = sqlx::query("SELECT nonce, ciphertext FROM provider_keys WHERE provider_id = ? AND disabled = 0 ORDER BY id LIMIT 1").bind(provider_id).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?.ok_or("provider has no enabled key")?;
        Ok(key_store
            .decrypt(row.get("nonce"), row.get("ciphertext"))?
            .to_string())
    }
    pub async fn create_route(&self, input: NewRoute) -> Result<Route, String> {
        if input.slug.is_empty() || input.patterns.is_empty() {
            return Err("route slug and patterns required".into());
        }
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        let position = sqlx::query("SELECT COALESCE(MAX(position), -1) + 1 FROM routes")
            .fetch_one(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?
            .get::<i64, _>(0);
        let id = sqlx::query(
            "INSERT INTO routes (slug, name, description, patterns, is_default, list_members, position) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(input.slug)
        .bind(input.name)
        .bind(input.description)
        .bind(serde_json::to_string(&input.patterns).map_err(|e| e.to_string())?)
        .bind(input.is_default)
        .bind(input.list_members)
        .bind(position)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid();
        let candidates = if input.candidates.is_empty() {
            vec![(input.provider_id, input.model)]
        } else {
            input.candidates
        };
        for (position, (provider_id, model)) in candidates.into_iter().enumerate() {
            sqlx::query("INSERT INTO route_candidates (route_id, provider_id, model, position) VALUES (?, ?, ?, ?)").bind(id).bind(provider_id).bind(model).bind(position as i64).execute(&mut *transaction).await.map_err(|e| e.to_string())?;
        }
        transaction.commit().await.map_err(|e| e.to_string())?;
        self.routes()
            .await?
            .into_iter()
            .find(|route| route.id == id)
            .ok_or("route not saved".into())
    }
    /// Sets the route's ordering strategy and per-candidate weights.
    /// `weights` must be parallel to the stored candidates; values clamp to 1..=100.
    pub async fn set_route_strategy(
        &self,
        id: i64,
        strategy: &str,
        weights: &[i64],
    ) -> Result<(), String> {
        if crate::strategy::Strategy::parse(strategy).is_none() {
            return Err(format!("unknown route strategy: {strategy}"));
        }
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("UPDATE routes SET strategy = ? WHERE id = ?")
            .bind(strategy)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        let candidate_ids: Vec<i64> =
            sqlx::query("SELECT id FROM route_candidates WHERE route_id = ? ORDER BY position")
                .bind(id)
                .fetch_all(&mut *transaction)
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|row| row.get("id"))
                .collect();
        for (index, candidate_id) in candidate_ids.iter().enumerate() {
            let weight = weights.get(index).copied().unwrap_or(1).clamp(1, 100);
            sqlx::query("UPDATE route_candidates SET weight = ? WHERE id = ?")
                .bind(weight)
                .bind(candidate_id)
                .execute(&mut *transaction)
                .await
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().await.map_err(|e| e.to_string())
    }
    pub async fn routes(&self) -> Result<Vec<Route>, String> {
        let rows = sqlx::query(
            "SELECT id, slug, name, description, patterns, is_default, list_members, enabled, strategy FROM routes ORDER BY position",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let mut routes = Vec::new();
        for row in rows {
            let id = row.get("id");
            let candidate_rows = sqlx::query("SELECT provider_id, model, weight FROM route_candidates WHERE route_id = ? ORDER BY position").bind(id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
            let candidates = candidate_rows
                .iter()
                .map(|r| (r.get("provider_id"), r.get("model")))
                .collect();
            let weights = candidate_rows.iter().map(|r| r.get("weight")).collect();
            routes.push(Route {
                id,
                slug: row.get("slug"),
                name: row.get("name"),
                description: row.get("description"),
                patterns: serde_json::from_str(row.get("patterns")).map_err(|e| e.to_string())?,
                is_default: row.get("is_default"),
                list_members: row.get("list_members"),
                enabled: row.get("enabled"),
                candidates,
                strategy: row.get("strategy"),
                weights,
            });
        }
        Ok(routes)
    }
    pub async fn create_client_key(
        &self,
        input: UpsertClientKey,
    ) -> Result<CreatedClientKey, String> {
        let expires_at = input
            .expires_at
            .as_deref()
            .map(normalize_expiration)
            .transpose()?;
        if let Some(expiry) = expires_at.as_deref() {
            let expiry = DateTime::parse_from_rfc3339(expiry)
                .map_err(|error| format!("invalid client key expiration: {error}"))?;
            if expiry < Utc::now() {
                return Err("client key expiration must be in the future".into());
            }
        }
        let plaintext = input
            .secret
            .filter(|secret| !secret.trim().is_empty())
            .unwrap_or_else(|| random_token("sk-sepia-"));
        let id = sqlx::query("INSERT INTO client_keys (token_hash, name, models, token_limit, max_amount_micros, rpm, tpm, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)").bind(token_hash(&plaintext)).bind(input.name).bind(serde_json::to_string(&input.models).map_err(|e| e.to_string())?).bind(input.token_limit).bind(input.max_amount_micros).bind(input.rpm).bind(input.tpm).bind(expires_at).execute(&self.pool).await.map_err(|e| e.to_string())?.last_insert_rowid();
        Ok(CreatedClientKey { id, plaintext })
    }
    pub async fn authenticate_client(&self, token: &str) -> Result<Option<ClientKey>, String> {
        let Some(row) = sqlx::query("SELECT id, name, models, token_limit, max_amount_micros, rpm, tpm, expires_at FROM client_keys WHERE token_hash = ? AND enabled = 1").bind(token_hash(token)).fetch_optional(&self.pool).await.map_err(|e| e.to_string())? else { return Ok(None); };
        if row
            .get::<Option<String>, _>("expires_at")
            .map(|v| DateTime::parse_from_rfc3339(&v).map(|d| d < Utc::now()))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(false)
        {
            return Ok(None);
        }
        let id: i64 = row.get("id");
        let mut models: Vec<String> =
            serde_json::from_str(row.get("models")).map_err(|e| e.to_string())?;
        // Model groups widen the key's scope: effective models are the union
        // of the key's own patterns and every referenced group's members.
        for member in self.model_group_members(id).await? {
            if !models.contains(&member) {
                models.push(member);
            }
        }
        Ok(Some(ClientKey {
            id,
            name: row.get("name"),
            models,
            token_limit: row.get("token_limit"),
            max_amount_micros: row.get("max_amount_micros"),
            rpm: row.get("rpm"),
            tpm: row.get("tpm"),
            expires_at: row.get("expires_at"),
        }))
    }
    pub async fn revoke_client_key(&self, id: i64) -> Result<(), String> {
        sqlx::query("UPDATE client_keys SET enabled = 0 WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn client_keys(&self) -> Result<Vec<ClientKey>, String> {
        let rows = sqlx::query("SELECT id, name, models, token_limit, max_amount_micros, rpm, tpm, expires_at FROM client_keys WHERE enabled = 1 ORDER BY id").fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok(ClientKey {
                    id: row.get("id"),
                    name: row.get("name"),
                    models: serde_json::from_str(row.get("models")).map_err(|e| e.to_string())?,
                    token_limit: row.get("token_limit"),
                    max_amount_micros: row.get("max_amount_micros"),
                    rpm: row.get("rpm"),
                    tpm: row.get("tpm"),
                    expires_at: row.get("expires_at"),
                })
            })
            .collect()
    }
    pub async fn plugin_records(&self) -> Result<Vec<PluginRecord>, String> {
        let rows = sqlx::query("SELECT id, name, version, enabled, status, manifest, config, wasm, ui_bundle, sha256, installed_at, updated_at FROM plugins ORDER BY id").fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok(PluginRecord {
                    id: row.get("id"),
                    name: row.get("name"),
                    version: row.get("version"),
                    enabled: row.get::<i64, _>("enabled") != 0,
                    status: row.get("status"),
                    manifest: serde_json::from_str(row.get("manifest"))
                        .map_err(|e| e.to_string())?,
                    config: serde_json::from_str(row.get("config")).map_err(|e| e.to_string())?,
                    wasm: row.get("wasm"),
                    ui_bundle: serde_json::from_str(row.get("ui_bundle"))
                        .map_err(|e| e.to_string())?,
                    sha256: row.get("sha256"),
                    installed_at: row.get("installed_at"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect()
    }
    pub async fn plugin(&self, id: &str) -> Result<Option<PluginRecord>, String> {
        Ok(self
            .plugin_records()
            .await?
            .into_iter()
            .find(|record| record.id == id))
    }
    pub async fn insert_plugin(&self, record: &PluginRecord, replace: bool) -> Result<(), String> {
        let manifest = serde_json::to_string(&record.manifest).map_err(|e| e.to_string())?;
        let config = serde_json::to_string(&record.config).map_err(|e| e.to_string())?;
        let ui_bundle = serde_json::to_string(&record.ui_bundle).map_err(|e| e.to_string())?;
        if replace {
            sqlx::query("UPDATE plugins SET name = ?, version = ?, status = ?, manifest = ?, wasm = ?, ui_bundle = ?, sha256 = ?, updated_at = ? WHERE id = ?")
                .bind(&record.name)
                .bind(&record.version)
                .bind(&record.status)
                .bind(&manifest)
                .bind(&record.wasm)
                .bind(&ui_bundle)
                .bind(&record.sha256)
                .bind(&record.updated_at)
                .bind(&record.id)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        } else {
            sqlx::query("INSERT INTO plugins (id, name, version, enabled, status, manifest, config, wasm, ui_bundle, sha256, installed_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&record.id)
                .bind(&record.name)
                .bind(&record.version)
                .bind(record.enabled as i64)
                .bind(&record.status)
                .bind(&manifest)
                .bind(&config)
                .bind(&record.wasm)
                .bind(&ui_bundle)
                .bind(&record.sha256)
                .bind(&record.installed_at)
                .bind(&record.updated_at)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub async fn set_plugin_enabled(
        &self,
        id: &str,
        enabled: bool,
        status: &str,
    ) -> Result<(), String> {
        sqlx::query("UPDATE plugins SET enabled = ?, status = ?, updated_at = ? WHERE id = ?")
            .bind(enabled as i64)
            .bind(status)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn set_plugin_config(&self, id: &str, config: &Value) -> Result<(), String> {
        sqlx::query("UPDATE plugins SET config = ?, updated_at = ? WHERE id = ?")
            .bind(serde_json::to_string(config).map_err(|e| e.to_string())?)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn delete_plugin(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM plugins WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn plugin_kv_get(
        &self,
        plugin_id: &str,
        key: &str,
    ) -> Result<Option<String>, String> {
        let row = sqlx::query("SELECT value FROM plugin_kv WHERE plugin_id = ? AND key = ?")
            .bind(plugin_id)
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map(|row| row.get("value")))
    }
    pub async fn plugin_kv_set(
        &self,
        plugin_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        sqlx::query("INSERT INTO plugin_kv (plugin_id, key, value) VALUES (?, ?, ?) ON CONFLICT(plugin_id, key) DO UPDATE SET value = excluded.value")
            .bind(plugin_id)
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn plugin_kv_delete(&self, plugin_id: &str, key: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM plugin_kv WHERE plugin_id = ? AND key = ?")
            .bind(plugin_id)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn plugin_kv_keys(&self, plugin_id: &str) -> Result<Vec<String>, String> {
        let rows = sqlx::query("SELECT key FROM plugin_kv WHERE plugin_id = ? ORDER BY key")
            .bind(plugin_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|row| row.get("key")).collect())
    }
    pub async fn plugin_secret_set(
        &self,
        plugin_id: &str,
        key: &str,
        nonce: &str,
        ciphertext: &str,
    ) -> Result<(), String> {
        sqlx::query("INSERT INTO plugin_secrets (plugin_id, key, nonce, ciphertext) VALUES (?, ?, ?, ?) ON CONFLICT(plugin_id, key) DO UPDATE SET nonce = excluded.nonce, ciphertext = excluded.ciphertext")
            .bind(plugin_id)
            .bind(key)
            .bind(nonce)
            .bind(ciphertext)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn plugin_secret_get(
        &self,
        plugin_id: &str,
        key: &str,
    ) -> Result<Option<(String, String)>, String> {
        let row = sqlx::query(
            "SELECT nonce, ciphertext FROM plugin_secrets WHERE plugin_id = ? AND key = ?",
        )
        .bind(plugin_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(|row| (row.get("nonce"), row.get("ciphertext"))))
    }
    pub async fn plugin_secret_delete(&self, plugin_id: &str, key: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM plugin_secrets WHERE plugin_id = ? AND key = ?")
            .bind(plugin_id)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn normalize_expiration(value: &str) -> Result<String, String> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed
            .to_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .map(|local| {
            local
                .and_utc()
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
        .map_err(|error| format!("invalid client key expiration: {error}"))
}
