//! AI service pool: optional OpenAI-compatible services used for AI-assisted
//! management tasks (route generation today). The pool is disabled by default
//! (`ai_pool_enabled` workspace setting); when enabled, generation batches are
//! dispatched dynamically across services, each bounded by its own concurrency.

use crate::{KeyStore, Provider, ProviderProtocol, Store, server::AppState};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::VecDeque;
use std::sync::Arc;

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":{"message":message.into()}}))).into_response()
}

#[derive(Debug, Clone, Serialize)]
pub struct AiPoolService {
    pub id: i64,
    pub name: String,
    pub endpoint: String,
    pub model: String,
    pub models: Vec<String>,
    pub timeout_seconds: i64,
    pub concurrency: i64,
    pub enabled: bool,
    /// When set, endpoint and key are derived live from this gateway provider
    /// instead of the stored custom-service credentials.
    pub provider_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct AiPoolServiceInput {
    pub name: String,
    pub endpoint: String,
    /// Empty on update keeps the stored key. Ignored for provider-backed services.
    pub secret: String,
    pub model: String,
    pub models: Vec<String>,
    pub timeout_seconds: i64,
    pub concurrency: i64,
    pub enabled: bool,
    pub provider_id: Option<i64>,
}

fn validate(input: &AiPoolServiceInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("service name required".into());
    }
    if input.provider_id.is_none() {
        let parsed = url::Url::parse(input.endpoint.trim()).map_err(|e| e.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("service endpoint must be HTTP(S)".into());
        }
    }
    if input.model.trim().is_empty() {
        return Err("service model required; detect models first".into());
    }
    if !(1..=600).contains(&input.timeout_seconds) {
        return Err("timeout must be between 1 and 600 seconds".into());
    }
    if !(1..=16).contains(&input.concurrency) {
        return Err("concurrency must be between 1 and 16".into());
    }
    Ok(())
}

impl Store {
    pub async fn ai_pool_services(&self) -> Result<Vec<AiPoolService>, String> {
        let rows = sqlx::query("SELECT id, name, endpoint, model, models, timeout_seconds, concurrency, enabled, provider_id, created_at, updated_at FROM ai_pool_services ORDER BY position, id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok(AiPoolService {
                    id: row.get("id"),
                    name: row.get("name"),
                    endpoint: row.get("endpoint"),
                    model: row.get("model"),
                    models: serde_json::from_str(row.get("models")).map_err(|e| e.to_string())?,
                    timeout_seconds: row.get("timeout_seconds"),
                    concurrency: row.get("concurrency"),
                    enabled: row.get("enabled"),
                    provider_id: row.get("provider_id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect()
    }

    pub async fn create_ai_pool_service(
        &self,
        input: AiPoolServiceInput,
        key_store: &KeyStore,
    ) -> Result<AiPoolService, String> {
        validate(&input)?;
        // Provider-backed services borrow the provider's endpoint and key live;
        // store an empty encrypted secret and resolve credentials on demand.
        let (endpoint, nonce, ciphertext) = match input.provider_id {
            Some(provider_id) => {
                let provider = self
                    .providers()
                    .await?
                    .into_iter()
                    .find(|provider| provider.id == provider_id)
                    .ok_or("provider not found")?;
                if provider.key_count == 0 {
                    return Err("provider has no enabled key".into());
                }
                let (nonce, ciphertext) = key_store.encrypt("")?;
                (provider.endpoint, nonce, ciphertext)
            }
            None => {
                if input.secret.trim().is_empty() {
                    return Err("service API key required".into());
                }
                let (nonce, ciphertext) = key_store.encrypt(input.secret.trim())?;
                (
                    input.endpoint.trim().trim_end_matches('/').to_string(),
                    nonce,
                    ciphertext,
                )
            }
        };
        let position = sqlx::query("SELECT COALESCE(MAX(position), -1) + 1 FROM ai_pool_services")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .get::<i64, _>(0);
        let id = sqlx::query("INSERT INTO ai_pool_services (name, endpoint, nonce, ciphertext, model, models, timeout_seconds, concurrency, enabled, provider_id, position) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(input.name.trim())
            .bind(endpoint)
            .bind(nonce)
            .bind(ciphertext)
            .bind(input.model.trim())
            .bind(serde_json::to_string(&input.models).map_err(|e| e.to_string())?)
            .bind(input.timeout_seconds)
            .bind(input.concurrency)
            .bind(input.enabled)
            .bind(input.provider_id)
            .bind(position)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .last_insert_rowid();
        self.ai_pool_services()
            .await?
            .into_iter()
            .find(|service| service.id == id)
            .ok_or("service not saved".into())
    }

    pub async fn update_ai_pool_service(
        &self,
        id: i64,
        input: AiPoolServiceInput,
        key_store: &KeyStore,
    ) -> Result<AiPoolService, String> {
        validate(&input)?;
        let existing = sqlx::query("SELECT nonce, ciphertext FROM ai_pool_services WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("service not found".to_string())?;
        let (endpoint, nonce, ciphertext) = match input.provider_id {
            Some(provider_id) => {
                let provider = self
                    .providers()
                    .await?
                    .into_iter()
                    .find(|provider| provider.id == provider_id)
                    .ok_or("provider not found")?;
                if provider.key_count == 0 {
                    return Err("provider has no enabled key".into());
                }
                let (nonce, ciphertext) = key_store.encrypt("")?;
                (provider.endpoint, nonce, ciphertext)
            }
            None => {
                let (nonce, ciphertext) = if input.secret.trim().is_empty() {
                    (
                        existing.get::<String, _>("nonce"),
                        existing.get::<String, _>("ciphertext"),
                    )
                } else {
                    key_store.encrypt(input.secret.trim())?
                };
                (
                    input.endpoint.trim().trim_end_matches('/').to_string(),
                    nonce,
                    ciphertext,
                )
            }
        };
        sqlx::query("UPDATE ai_pool_services SET name = ?, endpoint = ?, nonce = ?, ciphertext = ?, model = ?, models = ?, timeout_seconds = ?, concurrency = ?, enabled = ?, provider_id = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(input.name.trim())
            .bind(endpoint)
            .bind(nonce)
            .bind(ciphertext)
            .bind(input.model.trim())
            .bind(serde_json::to_string(&input.models).map_err(|e| e.to_string())?)
            .bind(input.timeout_seconds)
            .bind(input.concurrency)
            .bind(input.enabled)
            .bind(input.provider_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        self.ai_pool_services()
            .await?
            .into_iter()
            .find(|service| service.id == id)
            .ok_or("service not saved".into())
    }

    pub async fn delete_ai_pool_service(&self, id: i64) -> Result<(), String> {
        let result = sqlx::query("DELETE FROM ai_pool_services WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("service not found".into());
        }
        Ok(())
    }

    pub async fn ai_pool_secret(&self, id: i64, key_store: &KeyStore) -> Result<String, String> {
        let row =
            sqlx::query("SELECT nonce, ciphertext, provider_id FROM ai_pool_services WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("service not found".to_string())?;
        if let Some(provider_id) = row.get::<Option<i64>, _>("provider_id") {
            return self.upstream_key(provider_id, key_store).await;
        }
        Ok(key_store
            .decrypt(row.get("nonce"), row.get("ciphertext"))?
            .to_string())
    }

    /// Live endpoint for a service: provider-backed services follow the
    /// provider's current first address, custom services use the stored one.
    pub async fn ai_pool_endpoint(&self, service: &AiPoolService) -> Result<String, String> {
        match service.provider_id {
            Some(provider_id) => Ok(self
                .providers()
                .await?
                .into_iter()
                .find(|provider| provider.id == provider_id)
                .ok_or("provider not found")?
                .endpoint),
            None => Ok(service.endpoint.clone()),
        }
    }

    pub async fn ai_pool_enabled(&self) -> Result<bool, String> {
        Ok(self
            .settings()
            .await?
            .get("ai_pool_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }
}

#[derive(Deserialize)]
pub struct PoolServiceBody {
    name: String,
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    provider_id: Option<i64>,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_seconds: i64,
    #[serde(default = "default_concurrency")]
    concurrency: i64,
    #[serde(default = "default_enabled")]
    enabled: bool,
}
fn default_timeout() -> i64 {
    60
}
fn default_concurrency() -> i64 {
    2
}
fn default_enabled() -> bool {
    true
}
impl PoolServiceBody {
    fn into_input(self) -> AiPoolServiceInput {
        AiPoolServiceInput {
            name: self.name,
            endpoint: self.endpoint,
            secret: self.secret,
            model: self.model,
            models: self.models,
            timeout_seconds: self.timeout_seconds,
            concurrency: self.concurrency,
            enabled: self.enabled,
            provider_id: self.provider_id,
        }
    }
}

async fn list_pool(State(state): State<AppState>) -> Response {
    match state.store.ai_pool_services().await {
        Ok(services) => Json(json!({"services":services})).into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

async fn create_pool_service(
    State(state): State<AppState>,
    Json(body): Json<PoolServiceBody>,
) -> Response {
    match state
        .store
        .create_ai_pool_service(body.into_input(), &state.keys)
        .await
    {
        Ok(service) => Json(service).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}

async fn update_pool_service(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PoolServiceBody>,
) -> Response {
    match state
        .store
        .update_ai_pool_service(id, body.into_input(), &state.keys)
        .await
    {
        Ok(service) => Json(service).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}

async fn delete_pool_service(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.store.delete_ai_pool_service(id).await {
        Ok(()) => Json(json!({"deleted":true})).into_response(),
        Err(message) => error(StatusCode::NOT_FOUND, message),
    }
}

#[derive(Deserialize)]
pub struct PoolTestBody {
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    provider_id: Option<i64>,
}

/// Catalog discovery for an unsaved service draft. A provider-backed draft
/// borrows the gateway provider's endpoint and first key; custom drafts need
/// both endpoint and API key (OpenAI-compatible only).
async fn test_pool_draft(
    State(state): State<AppState>,
    Json(body): Json<PoolTestBody>,
) -> Response {
    if let Some(provider_id) = body.provider_id {
        let provider = match state.store.providers().await {
            Ok(providers) => providers
                .into_iter()
                .find(|provider| provider.id == provider_id),
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        let Some(provider) = provider else {
            return error(StatusCode::NOT_FOUND, "provider not found");
        };
        let secret = match state.store.upstream_key(provider_id, &state.keys).await {
            Ok(secret) => secret,
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        };
        return discover_pool_models(&state, &provider.endpoint, &secret).await;
    }
    if body.endpoint.trim().is_empty() || body.secret.trim().is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "endpoint and API key required for detection",
        );
    }
    discover_pool_models(&state, &body.endpoint, &body.secret).await
}

/// Catalog discovery for a saved service; a blank secret falls back to the stored key.
async fn test_pool_service(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PoolTestBody>,
) -> Response {
    let services = match state.store.ai_pool_services().await {
        Ok(services) => services,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let Some(service) = services.into_iter().find(|service| service.id == id) else {
        return error(StatusCode::NOT_FOUND, "service not found");
    };
    let endpoint = if body.endpoint.trim().is_empty() {
        match state.store.ai_pool_endpoint(&service).await {
            Ok(endpoint) => endpoint,
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        }
    } else {
        body.endpoint.trim().to_string()
    };
    let secret = if body.secret.trim().is_empty() {
        match state.store.ai_pool_secret(id, &state.keys).await {
            Ok(secret) => secret,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        }
    } else {
        body.secret.trim().to_string()
    };
    discover_pool_models(&state, &endpoint, &secret).await
}

async fn discover_pool_models(state: &AppState, endpoint: &str, secret: &str) -> Response {
    let probe = Provider {
        id: 0,
        name: "ai-pool".into(),
        protocol: ProviderProtocol::OpenAI,
        endpoint: endpoint.trim().trim_end_matches('/').to_string(),
        models: vec![],
        key_count: 1,
        group_id: "official".into(),
        enabled: true,
        plugin_id: None,
        rewrite: Default::default(),
    };
    match crate::discovery::discover_models(&state.http(), &probe, secret).await {
        Ok(models) => Json(json!({"models":models})).into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, message),
    }
}

#[derive(Deserialize)]
pub struct GenerateBody {
    instruction: String,
    #[serde(default)]
    batch_size: Option<i64>,
    /// Optional (provider_id, model) → family hints supplied by the frontend,
    /// which owns the configurable normalization rules. Hints that do not
    /// match a real catalog entry are ignored; see [`family_key`].
    #[serde(default)]
    families: Vec<FamilyHint>,
}

/// One optional family hint: the frontend's normalizer already collapsed
/// (provider_id, model) into a family key.
#[derive(Deserialize)]
pub struct FamilyHint {
    provider_id: i64,
    model: String,
    family: String,
}

/// Trim/lowercase a family key and cap it at 128 characters; empty hints are
/// rejected so callers fall back to the server-side derivation.
fn family_key(value: &str) -> Option<String> {
    let key: String = value.trim().to_lowercase().chars().take(128).collect();
    if key.is_empty() { None } else { Some(key) }
}

const GENERATE_SYSTEM: &str = "You are a routing planner for an AI gateway. You receive a catalog of provider models and a user instruction. Group models into smart routes: one route per logical model or model family, with ordered failover candidates (best provider first). Respond with ONLY a JSON object, no prose and no code fences: {\"routes\":[{\"slug\":\"lowercase-kebab-case\",\"name\":\"Human Readable Name\",\"description\":\"one sentence\",\"patterns\":[\"model-name-glob\"],\"candidates\":[{\"provider_id\":1,\"model\":\"exact model id\"}]}]}. Rules: use only provider_id/model pairs present in the catalog; never invent providers or models; name is a human-readable display name with the model family's natural capitalization and spaces, never the hyphenated slug (slug \"qwen4-max\" gets name \"Qwen4 Max\"); patterns are model names clients may request (default to the slug); keep candidates ordered by preference; every catalog line carries a family=<key> field, and lines sharing the same family value are the SAME logical model across providers: they MUST become one route whose candidates include every provider/model pair of that family, ordered by preference.";

/// One catalog entry offered to the planner.
struct CatalogEntry {
    provider_id: i64,
    provider: String,
    model: String,
    /// Logical model family used for batching and server-side merging.
    family: String,
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-').chars().take(64).collect::<String>();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() || !slug.chars().next().unwrap().is_ascii_alphanumeric() {
        return "ai-route".into();
    }
    slug
}

/// Extract the planner's route proposals from a chat reply (tolerates fences/prose).
fn parse_proposals(text: &str) -> Result<Vec<Value>, String> {
    let trimmed = text.trim();
    let start = trimmed.find('{').ok_or("reply contains no JSON object")?;
    let end = trimmed.rfind('}').ok_or("reply contains no JSON object")?;
    if end <= start {
        return Err("reply contains no JSON object".into());
    }
    let value: Value =
        serde_json::from_str(&trimmed[start..=end]).map_err(|e| format!("invalid JSON: {e}"))?;
    let routes = value
        .get("routes")
        .and_then(Value::as_array)
        .ok_or("reply JSON missing routes array")?;
    Ok(routes.clone())
}

async fn pool_chat(
    client: &reqwest::Client,
    endpoint: &str,
    secret: &str,
    model: &str,
    timeout_seconds: i64,
    user_content: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
    let response = client
        .post(url)
        .bearer_auth(secret)
        .timeout(std::time::Duration::from_secs(timeout_seconds as u64))
        .json(&json!({
            "model": model,
            "stream": false,
            "max_tokens": 4096,
            "messages": [
                {"role":"system","content":GENERATE_SYSTEM},
                {"role":"user","content":user_content}
            ]
        }))
        .send()
        .await
        .map_err(|_| "AI service connection failed or timed out".to_string())?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|_| format!("AI service returned HTTP {status} with an unreadable body"))?;
    if !status.is_success() {
        let message = body
            .get("error")
            .and_then(|value| {
                value.as_str().map(str::to_owned).or_else(|| {
                    value
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
            })
            .unwrap_or_default();
        return Err(if message.is_empty() {
            format!("AI service returned HTTP {status}")
        } else {
            format!("AI service returned HTTP {status}: {message}")
        });
    }
    let content = body
        .pointer("/choices/0/message/content")
        .ok_or("AI service reply missing message content")?;
    match content {
        Value::String(text) => Ok(text.clone()),
        Value::Array(blocks) => Ok(blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")),
        _ => Err("AI service reply content is not text".into()),
    }
}

/// Readable display name for a lowercase kebab-case route slug.
///
/// Splits the slug on '-', joins the words with single spaces and capitalizes
/// the first character of every word. A purely ASCII-alphabetic word is
/// lowercased after that first character ("qwen" → "Qwen"), while a word
/// containing digits keeps its remaining characters unchanged ("qwen4" →
/// "Qwen4", "4o" → "4o" because its first character is a digit). The result is
/// capped at 120 characters: "qwen4-max" → "Qwen4 Max".
fn display_name_from_slug(slug: &str) -> String {
    let mut name = String::new();
    for word in slug.split('-').filter(|word| !word.is_empty()) {
        if !name.is_empty() {
            name.push(' ');
        }
        let mut chars = word.chars();
        let Some(first) = chars.next() else {
            continue;
        };
        name.push(first.to_ascii_uppercase());
        if word.chars().all(|ch| ch.is_ascii_alphabetic()) {
            name.push_str(&chars.as_str().to_ascii_lowercase());
        } else {
            name.push_str(chars.as_str());
        }
    }
    name.chars().take(120).collect()
}

/// True when the planner echoed a slug back instead of naming the route.
fn name_repeats_slug(name: &str, raw_slug: &str, slug: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == slug || name == raw_slug.trim().to_ascii_lowercase()
}

/// Validate planner output against the real catalog and existing routes.
///
/// Proposals are first validated independently (candidates restricted to real
/// enabled provider-owned pairs, slug/name/patterns sanitized), then merged by
/// family: batches are packed so a family is never split, but the planner may
/// still split one family across replies, and two batches may propose the same
/// slug. Proposals sharing the family of their first validated candidate are
/// one route: the first arrival wins the slug/name/description, candidates and
/// patterns are unioned (first-seen order, ≤8 patterns). A proposal whose
/// candidates span several families therefore merges under its primary family
/// only; determinism comes from the sorted catalog packing plus arrival order.
fn sanitize_proposals(
    proposals: Vec<Value>,
    providers: &[Provider],
    families: &std::collections::HashMap<(i64, String), String>,
    existing_slugs: &mut std::collections::HashSet<String>,
) -> Vec<Value> {
    struct Validated {
        slug: String,
        name: String,
        description: String,
        patterns: Vec<String>,
        candidates: Vec<Value>,
        family: String,
    }
    let mut validated = Vec::<Validated>::new();
    for proposal in proposals {
        let Some(object) = proposal.as_object() else {
            continue;
        };
        let raw_slug = object.get("slug").and_then(Value::as_str).unwrap_or("");
        let slug = slugify(raw_slug);
        let mut candidates = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for candidate in object
            .get("candidates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(provider_id) = candidate.get("provider_id").and_then(Value::as_i64) else {
                continue;
            };
            let Some(model) = candidate.get("model").and_then(Value::as_str) else {
                continue;
            };
            let Some(provider) = providers
                .iter()
                .find(|provider| provider.id == provider_id && provider.enabled)
            else {
                continue;
            };
            if !provider.models.iter().any(|owned| owned == model) {
                continue;
            }
            if seen.insert((provider_id, model.to_string())) {
                candidates.push(
                    json!({"provider_id":provider_id,"provider":provider.name,"model":model}),
                );
            }
        }
        let Some(first) = candidates.first() else {
            continue;
        };
        let family = families
            .get(&(
                first["provider_id"].as_i64().unwrap_or_default(),
                first["model"].as_str().unwrap_or_default().to_string(),
            ))
            .cloned()
            .unwrap_or_else(|| {
                crate::metadata::base_model(first["model"].as_str().unwrap_or_default())
            });
        let raw_name = object
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty());
        // Missing names and names that merely repeat the slug are replaced by
        // a readable derivation so clients never see "qwen4-max"-style text.
        let name = match raw_name {
            Some(name) if !name_repeats_slug(name, raw_slug, &slug) => name.to_string(),
            _ => display_name_from_slug(&slug),
        };
        let name: String = name.chars().take(120).collect();
        let description = object
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .chars()
            .take(500)
            .collect::<String>();
        let mut patterns = object
            .get("patterns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
            .take(8)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if patterns.is_empty() {
            patterns.push(slug.clone());
        }
        validated.push(Validated {
            slug,
            name,
            description,
            patterns,
            candidates,
            family,
        });
    }
    let mut merged: Vec<Validated> = Vec::new();
    for proposal in validated {
        let Some(route) = merged
            .iter_mut()
            .find(|route| route.family == proposal.family)
        else {
            merged.push(proposal);
            continue;
        };
        for candidate in proposal.candidates {
            let key = (
                candidate["provider_id"].as_i64().unwrap_or_default(),
                candidate["model"].as_str().unwrap_or_default().to_string(),
            );
            let present = route.candidates.iter().any(|existing| {
                (
                    existing["provider_id"].as_i64().unwrap_or_default(),
                    existing["model"].as_str().unwrap_or_default().to_string(),
                ) == key
            });
            if !present {
                route.candidates.push(candidate);
            }
        }
        for pattern in proposal.patterns {
            if route.patterns.len() < 8 && !route.patterns.contains(&pattern) {
                route.patterns.push(pattern);
            }
        }
    }
    // Stored routes keep the legacy contract: a proposal that reproduces an
    // existing slug is dropped. Two proposals in this batch may still collide
    // (different families, same slug), and dropping the second one would lose
    // routable candidates, so it keeps its own route under a numeric suffix.
    let stored: std::collections::HashSet<String> = existing_slugs.clone();
    let mut used: std::collections::HashSet<String> = existing_slugs.clone();
    let mut routes = Vec::new();
    for route in merged {
        if stored.contains(&route.slug) {
            continue;
        }
        let (slug, name) = if used.contains(&route.slug) {
            disambiguate_slug(&route.slug, &route.name, &used)
        } else {
            (route.slug, route.name)
        };
        used.insert(slug.clone());
        existing_slugs.insert(slug.clone());
        routes.push(json!({"slug":slug,"name":name,"description":route.description,"patterns":route.patterns,"candidates":route.candidates}));
    }
    routes
}

/// Smallest numeric suffix that frees `slug` while respecting the 64-character
/// slug contract (the base is shortened when the suffix needs the room) and the
/// 120-character display-name cap; the name gains the same number so the two
/// routes stay distinguishable.
fn disambiguate_slug(
    slug: &str,
    name: &str,
    used: &std::collections::HashSet<String>,
) -> (String, String) {
    let mut suffix = 2u32;
    loop {
        let suffix_text = format!("-{suffix}");
        let slug_room = 64usize.saturating_sub(suffix_text.chars().count()).max(1);
        let base = slug
            .chars()
            .take(slug_room)
            .collect::<String>()
            .trim_end_matches('-')
            .to_string();
        let base = if base.is_empty() {
            "ai-route".to_string()
        } else {
            base
        };
        let candidate = format!("{base}{suffix_text}");
        if !used.contains(&candidate) {
            let name_suffix = format!(" {suffix}");
            let name_room = 120usize.saturating_sub(name_suffix.chars().count());
            let base_name = name
                .chars()
                .take(name_room)
                .collect::<String>()
                .trim_end()
                .to_string();
            return (candidate, format!("{base_name}{name_suffix}"));
        }
        suffix += 1;
    }
}

pub async fn generate_routes(
    State(state): State<AppState>,
    Json(body): Json<GenerateBody>,
) -> Response {
    if body.instruction.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "instruction required");
    }
    match state.store.ai_pool_enabled().await {
        Ok(true) => {}
        Ok(false) => {
            return error(
                StatusCode::CONFLICT,
                "AI service pool is disabled; enable it in Settings → AI",
            );
        }
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
    let services = match state.store.ai_pool_services().await {
        Ok(services) => services,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut active = Vec::new();
    for service in services
        .into_iter()
        .filter(|s| s.enabled && !s.model.is_empty())
    {
        match state.store.ai_pool_secret(service.id, &state.keys).await {
            Ok(secret) => active.push((service, secret)),
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
        }
    }
    if active.is_empty() {
        return error(
            StatusCode::CONFLICT,
            "no enabled AI service with a model; add one in Settings → AI",
        );
    }
    let providers = match state.store.providers().await {
        Ok(providers) => providers,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    // Frontend-supplied family hints win over the server-side derivation; both
    // sides are restricted to real (enabled provider, owned model) pairs so a
    // hint can never invent a family for a model that is not routable.
    let hint_map: std::collections::HashMap<(i64, String), String> = body
        .families
        .iter()
        .filter_map(|hint| {
            let family = family_key(&hint.family)?;
            Some(((hint.provider_id, hint.model.clone()), family))
        })
        .collect();
    let mut catalog: Vec<CatalogEntry> = providers
        .iter()
        .filter(|provider| provider.enabled && provider.key_count > 0)
        .flat_map(|provider| {
            provider.models.iter().map(|model| CatalogEntry {
                provider_id: provider.id,
                provider: provider.name.clone(),
                model: model.clone(),
                // `metadata::base_model` strips vendor/ path prefixes and
                // dated/free suffixes, so auto/kimi-k3.1-pro and
                // moonshotai/kimi-k3.1-pro share a family without hints.
                family: hint_map
                    .get(&(provider.id, model.clone()))
                    .cloned()
                    .unwrap_or_else(|| crate::metadata::base_model(model)),
            })
        })
        .collect();
    if catalog.is_empty() {
        return error(
            StatusCode::CONFLICT,
            "no enabled provider models available to route",
        );
    }
    // Group by family so variants of one logical model land in one batch;
    // within a family keep catalog order (model, then provider) stable.
    catalog.sort_by(|a, b| {
        a.family
            .cmp(&b.family)
            .then(a.model.cmp(&b.model))
            .then(a.provider.cmp(&b.provider))
            .then(a.provider_id.cmp(&b.provider_id))
    });
    let batch_size = body.batch_size.unwrap_or(10).clamp(1, 50) as usize;
    // Pack batches without ever splitting a family: a family that does not fit
    // in the current batch starts a new one, and a family larger than
    // batch_size gets an oversize batch of its own.
    let mut batch_groups: Vec<Vec<&CatalogEntry>> = Vec::new();
    for entry in &catalog {
        let starts_family = batch_groups
            .last()
            .and_then(|group| group.first())
            .is_none_or(|first| first.family != entry.family);
        let full = batch_groups
            .last()
            .is_some_and(|group| group.len() >= batch_size);
        if batch_groups.is_empty() || (starts_family && full) {
            batch_groups.push(Vec::new());
        }
        batch_groups.last_mut().unwrap().push(entry);
    }
    let batch_count_hint = batch_groups.len();
    let batches: Vec<String> = batch_groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let lines = group
                .iter()
                .map(|entry| {
                    format!(
                        "provider_id={} provider={} model={} family={}",
                        entry.provider_id, entry.provider, entry.model, entry.family
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Instruction: {}\n\nCatalog batch {} of {}:\n{}",
                body.instruction.trim(),
                index + 1,
                batch_count_hint,
                lines
            )
        })
        .collect();
    let existing = match state.store.routes().await {
        Ok(routes) => routes,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let mut existing_slugs: std::collections::HashSet<String> =
        existing.iter().map(|route| route.slug.clone()).collect();
    // Dynamic dispatch: every service runs `concurrency` workers that report
    // readiness to a central scheduler. The scheduler hands each ready worker
    // one batch and collects outcomes asynchronously, so faster services take
    // more batches. A failed batch is retried once, preferably by a different
    // service; when only the failing service has idle workers the scheduler
    // waits a short grace period for another service before forcing the retry.
    const MAX_ATTEMPTS: usize = 2;
    const REROUTE_GRACE: std::time::Duration = std::time::Duration::from_millis(300);
    struct Batch {
        index: usize,
        content: Arc<str>,
        attempts: usize,
        failed_service: Option<usize>,
        requeued_at: Option<std::time::Instant>,
    }
    enum WorkerEvent {
        Ready(usize),
        Outcome {
            service: usize,
            index: usize,
            content: Arc<str>,
            attempts: usize,
            result: Result<Vec<Value>, String>,
        },
    }
    struct Assignment {
        batch: Batch,
    }
    let batch_count = batches.len();
    let mut queue: VecDeque<Batch> = batches
        .into_iter()
        .enumerate()
        .map(|(index, content)| Batch {
            index,
            content: Arc::from(content.as_str()),
            attempts: 0,
            failed_service: None,
            requeued_at: None,
        })
        .collect();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<WorkerEvent>();
    let mut assignment_txs = Vec::new();
    let mut worker_services: Vec<usize> = Vec::new();
    let mut workers = tokio::task::JoinSet::new();
    for (service_index, (service, secret)) in active.iter().enumerate() {
        let endpoint = match state.store.ai_pool_endpoint(service).await {
            Ok(endpoint) => endpoint,
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        };
        for _ in 0..service.concurrency {
            // Channels are indexed by global worker id, never by service
            // index: several workers share one service, and the scheduler
            // must hand each ready worker its own channel.
            let worker_id = assignment_txs.len();
            let (assign_tx, mut assign_rx) = tokio::sync::mpsc::unbounded_channel::<Assignment>();
            assignment_txs.push(assign_tx);
            worker_services.push(service_index);
            let events = event_tx.clone();
            let client = state.http();
            let endpoint = endpoint.clone();
            let model = service.model.clone();
            let timeout = service.timeout_seconds;
            let secret = secret.clone();
            workers.spawn(async move {
                while events.send(WorkerEvent::Ready(worker_id)).is_ok() {
                    let Some(Assignment { batch }) = assign_rx.recv().await else {
                        break;
                    };
                    let result =
                        pool_chat(&client, &endpoint, &secret, &model, timeout, &batch.content)
                            .await
                            .and_then(|text| parse_proposals(&text));
                    let sent = events.send(WorkerEvent::Outcome {
                        service: service_index,
                        index: batch.index,
                        content: batch.content,
                        attempts: batch.attempts,
                        result,
                    });
                    if sent.is_err() {
                        break;
                    }
                }
            });
        }
    }
    drop(event_tx);
    let mut proposals = Vec::<Value>::new();
    let mut errors = Vec::<String>::new();
    let mut remaining = batch_count;
    let mut in_flight = 0usize;
    let mut ready_workers: VecDeque<usize> = VecDeque::new();
    let eligible = |batch: &Batch, worker: usize| {
        batch.failed_service != Some(worker_services[worker])
            || batch
                .requeued_at
                .is_some_and(|at| at.elapsed() >= REROUTE_GRACE)
    };
    while remaining > 0 {
        // Dispatch every eligible ready worker before waiting for events.
        while let Some(position) = ready_workers
            .iter()
            .position(|worker| queue.iter().any(|batch| eligible(batch, *worker)))
        {
            let worker = ready_workers.remove(position).unwrap();
            let batch_position = queue
                .iter()
                .position(|batch| eligible(batch, worker))
                .unwrap();
            let batch = queue.remove(batch_position).unwrap();
            let batch_index = batch.index;
            if assignment_txs[worker].send(Assignment { batch }).is_err() {
                errors.push(format!(
                    "batch {}: AI service worker stopped",
                    batch_index + 1
                ));
                remaining -= 1;
                continue;
            }
            in_flight += 1;
        }
        if remaining == 0 {
            break;
        }
        // Nothing dispatchable: wait for a worker event. When every idle
        // worker belongs to the failing service and nothing is in flight, cap
        // the wait at the earliest requeue grace deadline to force the retry.
        let grace_deadline = if in_flight == 0 && !queue.is_empty() && !ready_workers.is_empty() {
            queue
                .iter()
                .filter_map(|batch| batch.requeued_at)
                .min()
                .map(|at| tokio::time::Instant::from_std(at + REROUTE_GRACE))
        } else {
            None
        };
        let event = match grace_deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, event_rx.recv())
                .await
                .unwrap_or(None),
            None => event_rx.recv().await,
        };
        match event {
            Some(WorkerEvent::Ready(worker)) => ready_workers.push_back(worker),
            Some(WorkerEvent::Outcome {
                service,
                index,
                content,
                attempts,
                result,
                ..
            }) => {
                in_flight -= 1;
                match result {
                    Ok(found) => {
                        proposals.extend(found);
                        remaining -= 1;
                    }
                    Err(message) => {
                        if attempts + 1 < MAX_ATTEMPTS {
                            queue.push_back(Batch {
                                index,
                                content,
                                attempts: attempts + 1,
                                failed_service: Some(service),
                                requeued_at: Some(std::time::Instant::now()),
                            });
                        } else {
                            errors.push(format!("batch {}: {message}", index + 1));
                            remaining -= 1;
                        }
                    }
                }
            }
            None => {
                if grace_deadline.is_some() {
                    // Grace expired: allow retries on the failing service.
                    for batch in queue.iter_mut() {
                        batch.failed_service = None;
                        batch.requeued_at = None;
                    }
                    continue;
                }
                // Channel closed with batches still pending: workers are gone.
                for batch in queue.drain(..) {
                    errors.push(format!(
                        "batch {}: AI service worker stopped",
                        batch.index + 1
                    ));
                }
                remaining = in_flight;
                while remaining > 0 {
                    match event_rx.recv().await {
                        Some(WorkerEvent::Outcome { index, result, .. }) => {
                            match result {
                                Ok(found) => proposals.extend(found),
                                Err(message) => {
                                    errors.push(format!("batch {}: {message}", index + 1))
                                }
                            }
                            remaining -= 1;
                        }
                        _ => break,
                    }
                }
            }
        }
    }
    drop(assignment_txs);
    while workers.join_next().await.is_some() {}
    let families: std::collections::HashMap<(i64, String), String> = catalog
        .iter()
        .map(|entry| {
            (
                (entry.provider_id, entry.model.clone()),
                entry.family.clone(),
            )
        })
        .collect();
    let routes = sanitize_proposals(proposals, &providers, &families, &mut existing_slugs);
    Json(json!({"routes":routes,"errors":errors,"batches":batch_count,"services":active.len()}))
        .into_response()
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/ai/pool", get(list_pool).post(create_pool_service))
        .route("/ai/pool/test", post(test_pool_draft))
        .route(
            "/ai/pool/{id}",
            axum::routing::put(update_pool_service).delete(delete_pool_service),
        )
        .route("/ai/pool/{id}/test", post(test_pool_service))
        .route("/ai/routes/generate", post(generate_routes))
}
