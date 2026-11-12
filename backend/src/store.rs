use crate::{
    ClientKey,
    crypto::{KeyStore, password_hash, random_token, token_hash, verify_password},
};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
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
}
impl ProviderProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "openai" => Ok(Self::OpenAI),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            _ => Err("unknown provider protocol".into()),
        }
    }
}
#[derive(Debug, Clone)]
pub struct NewProvider {
    pub name: String,
    pub protocol: ProviderProtocol,
    pub endpoint: String,
    pub models: Vec<String>,
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
pub struct AdminSession {
    pub username: String,
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
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self { pool })
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
        if supplied_token != expected_token {
            return Err("invalid bootstrap token".into());
        }
        if username.trim().is_empty() || password.len() < 12 {
            return Err("username and a password of at least 12 characters required".into());
        }
        sqlx::query(
            "INSERT INTO admins (id, username, password_hash, created_at) VALUES (1, ?, ?, ?)",
        )
        .bind(username)
        .bind(password_hash(password)?)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn login(&self, username: &str, password: &str) -> Result<String, String> {
        let row = sqlx::query("SELECT password_hash FROM admins WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("invalid credentials")?;
        if !verify_password(row.get("password_hash"), password) {
            return Err("invalid credentials".into());
        }
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
    pub async fn session(&self, token: &str) -> Result<Option<AdminSession>, String> {
        let Some(row) =
            sqlx::query("SELECT username, expires_at FROM sessions WHERE token_hash = ?")
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
    pub async fn create_provider(
        &self,
        input: NewProvider,
        secret: &str,
        key_store: &KeyStore,
    ) -> Result<Provider, String> {
        if input.name.trim().is_empty() || input.models.is_empty() {
            return Err("provider name and models required".into());
        }
        let parsed = url::Url::parse(&input.endpoint).map_err(|e| e.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("provider endpoint must be HTTP(S)".into());
        }
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO channel_groups (id, name, description) VALUES ('official', 'Official', 'Built-in provider group') ON CONFLICT(id) DO NOTHING")
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        let id = sqlx::query("INSERT INTO providers (name, protocol, endpoint) VALUES (?, ?, ?)")
            .bind(input.name)
            .bind(input.protocol.as_str())
            .bind(input.endpoint.trim_end_matches('/'))
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
        sqlx::query("INSERT INTO provider_keys (provider_id, nonce, ciphertext) VALUES (?, ?, ?)")
            .bind(id)
            .bind(nonce)
            .bind(ciphertext)
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
        let rows = sqlx::query("SELECT p.id, p.name, p.protocol, p.endpoint, p.group_id, p.enabled, COUNT(k.id) key_count FROM providers p LEFT JOIN provider_keys k ON k.provider_id = p.id AND k.disabled = 0 GROUP BY p.id ORDER BY p.id").fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
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
            });
        }
        Ok(providers)
    }
    pub async fn add_provider_key(
        &self,
        provider_id: i64,
        secret: &str,
        key_store: &KeyStore,
    ) -> Result<i64, String> {
        let (nonce, ciphertext) = key_store.encrypt(secret)?;
        Ok(sqlx::query(
            "INSERT INTO provider_keys (provider_id, nonce, ciphertext) VALUES (?, ?, ?)",
        )
        .bind(provider_id)
        .bind(nonce)
        .bind(ciphertext)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid())
    }
    pub async fn upstream_key_for_attempt(
        &self,
        provider_id: i64,
        start_index: usize,
        cooldowns: &std::collections::HashMap<i64, chrono::DateTime<Utc>>,
        key_store: &KeyStore,
    ) -> Result<Option<(i64, String)>, String> {
        let rows = sqlx::query("SELECT id, nonce, ciphertext FROM provider_keys WHERE provider_id = ? AND disabled = 0 ORDER BY id").bind(provider_id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
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
    pub async fn routes(&self) -> Result<Vec<Route>, String> {
        let rows = sqlx::query(
            "SELECT id, slug, name, description, patterns, is_default, list_members, enabled FROM routes ORDER BY position",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let mut routes = Vec::new();
        for row in rows {
            let id = row.get("id");
            let candidates = sqlx::query("SELECT provider_id, model FROM route_candidates WHERE route_id = ? ORDER BY position").bind(id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|r| (r.get("provider_id"), r.get("model"))).collect();
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
        let plaintext = random_token("sk-sepia-");
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
        Ok(Some(ClientKey {
            id: row.get("id"),
            name: row.get("name"),
            models: serde_json::from_str(row.get("models")).map_err(|e| e.to_string())?,
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
