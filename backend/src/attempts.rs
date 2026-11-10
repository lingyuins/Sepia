use crate::{ProviderProtocol, Store};
use sqlx::Row;

impl Store {
    pub async fn attempts(
        &self,
        request_id: &str,
    ) -> Result<Vec<crate::observability::AttemptRecord>, String> {
        let rows = sqlx::query("SELECT id, request_id, provider_id, provider_key_id, model, status, error, input_tokens, output_tokens, latency_ms, created_at FROM attempts WHERE request_id = ? ORDER BY id").bind(request_id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| crate::observability::AttemptRecord {
                id: row.get("id"),
                request_id: row.get("request_id"),
                provider_id: row.get("provider_id"),
                provider_key_id: row.get("provider_key_id"),
                model: row.get("model"),
                status: row.get("status"),
                error: row.get("error"),
                input_tokens: row.get("input_tokens"),
                output_tokens: row.get("output_tokens"),
                latency_ms: row.get("latency_ms"),
                created_at: row.get("created_at"),
            })
            .collect())
    }
    pub fn provider_protocol(value: &str) -> Result<ProviderProtocol, String> {
        match value {
            "openai" => Ok(ProviderProtocol::OpenAI),
            "anthropic" => Ok(ProviderProtocol::Anthropic),
            "gemini" => Ok(ProviderProtocol::Gemini),
            _ => Err("unknown protocol".into()),
        }
    }
}
