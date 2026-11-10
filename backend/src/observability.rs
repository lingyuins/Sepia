use crate::{ProviderProtocol, Store};
use chrono::Utc;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct RequestRecord {
    pub id: String,
    pub protocol: String,
    pub model: String,
    pub status: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_micros: i64,
    pub latency_ms: i64,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct AttemptRecord {
    pub id: i64,
    pub request_id: String,
    pub provider_id: Option<i64>,
    pub provider_key_id: Option<i64>,
    pub model: String,
    pub status: String,
    pub error: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub latency_ms: i64,
    pub created_at: String,
}
pub struct NewAttempt<'a> {
    pub request_id: &'a str,
    pub provider_id: Option<i64>,
    pub model: &'a str,
    pub status: &'a str,
    pub error: Option<&'a str>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: i64,
}
impl Store {
    pub async fn begin_request(
        &self,
        client_key_id: i64,
        protocol: ProviderProtocol,
        model: &str,
    ) -> Result<String, String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO requests (id, client_key_id, protocol, model, status, created_at) VALUES (?, ?, ?, ?, 'pending', ?)").bind(&id).bind(client_key_id).bind(protocol.as_str()).bind(model).bind(Utc::now().to_rfc3339()).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub async fn record_attempt(&self, attempt: NewAttempt<'_>) -> Result<(), String> {
        let NewAttempt {
            request_id,
            provider_id,
            model,
            status,
            error,
            input_tokens,
            output_tokens,
            latency_ms,
        } = attempt;
        sqlx::query("INSERT INTO attempts (request_id, provider_id, model, status, error, input_tokens, output_tokens, latency_ms, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)").bind(request_id).bind(provider_id).bind(model).bind(status).bind(error).bind(input_tokens as i64).bind(output_tokens as i64).bind(latency_ms).bind(Utc::now().to_rfc3339()).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn finish_request(
        &self,
        request_id: &str,
        status: &str,
        input_tokens: u64,
        output_tokens: u64,
        latency_ms: i64,
    ) -> Result<(), String> {
        sqlx::query("UPDATE requests SET status = ?, input_tokens = ?, output_tokens = ?, latency_ms = ? WHERE id = ?").bind(status).bind(input_tokens as i64).bind(output_tokens as i64).bind(latency_ms).bind(request_id).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn requests(&self) -> Result<Vec<RequestRecord>, String> {
        let rows = sqlx::query("SELECT id, protocol, model, status, input_tokens, output_tokens, cost_micros, latency_ms, created_at FROM requests ORDER BY created_at DESC LIMIT 200").fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| RequestRecord {
                id: row.get("id"),
                protocol: row.get("protocol"),
                model: row.get("model"),
                status: row.get("status"),
                input_tokens: row.get("input_tokens"),
                output_tokens: row.get("output_tokens"),
                cost_micros: row.get("cost_micros"),
                latency_ms: row.get("latency_ms"),
                created_at: row.get("created_at"),
            })
            .collect())
    }
}
