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
    pub requested_model: String,
    pub status: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_micros: i64,
    pub latency_ms: i64,
    pub created_at: String,
    pub route: Option<String>,
    pub provider: Option<String>,
    pub key: Option<String>,
    pub client_ip: Option<String>,
    pub first_byte_ms: Option<i64>,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub has_body: bool,
    pub error: Option<String>,
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
    pub provider_key_id: Option<i64>,
    pub model: &'a str,
    pub status: &'a str,
    pub error: Option<&'a str>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: i64,
}
impl Store {
    pub async fn begin_request_with_id(
        &self,
        id: &str,
        client_key_id: i64,
        protocol: ProviderProtocol,
        model: &str,
    ) -> Result<String, String> {
        sqlx::query("INSERT INTO requests (id, client_key_id, protocol, model, status, created_at) VALUES (?, ?, ?, ?, 'pending', ?)").bind(id).bind(client_key_id).bind(protocol.as_str()).bind(model).bind(Utc::now().to_rfc3339()).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(id.to_string())
    }
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
    pub async fn set_request_route(&self, request_id: &str, route: &str) -> Result<(), String> {
        sqlx::query("UPDATE requests SET route = ? WHERE id = ?")
            .bind(route)
            .bind(request_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Non-Gemini inbound bodies only carry the model in the decoded request;
    /// persist it once decoding succeeds so failed/cancelled rows keep the
    /// requested name even without a successful attempt to resolve.
    pub async fn set_request_model(&self, request_id: &str, model: &str) -> Result<(), String> {
        sqlx::query("UPDATE requests SET model = ? WHERE id = ?")
            .bind(model)
            .bind(request_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn record_attempt(&self, attempt: NewAttempt<'_>) -> Result<(), String> {
        let NewAttempt {
            request_id,
            provider_id,
            provider_key_id,
            model,
            status,
            error,
            input_tokens,
            output_tokens,
            latency_ms,
        } = attempt;
        sqlx::query("INSERT INTO attempts (request_id, provider_id, provider_key_id, model, status, error, input_tokens, output_tokens, latency_ms, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)").bind(request_id).bind(provider_id).bind(provider_key_id).bind(model).bind(status).bind(error).bind(input_tokens as i64).bind(output_tokens as i64).bind(latency_ms).bind(Utc::now().to_rfc3339()).execute(&self.pool).await.map_err(|e| e.to_string())?;
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
    pub async fn finish_request_with_cost(
        &self,
        request_id: &str,
        status: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost_micros: i64,
        latency_ms: i64,
    ) -> Result<(), String> {
        sqlx::query("UPDATE requests SET status = ?, input_tokens = ?, output_tokens = ?, cost_micros = ?, latency_ms = ? WHERE id = ?").bind(status).bind(input_tokens as i64).bind(output_tokens as i64).bind(cost_micros).bind(latency_ms).bind(request_id).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    /// True when the request exists with a NULL client_key_id — the state a
    /// hard key delete leaves behind. Guards settlement after the key's open
    /// quota reservations were cleared.
    pub async fn request_key_deleted(&self, request_id: &str) -> Result<bool, String> {
        let row = sqlx::query("SELECT client_key_id FROM requests WHERE id = ?")
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.is_some_and(|row| row.get::<Option<i64>, _>("client_key_id").is_none()))
    }
    pub async fn set_request_client_ip(&self, request_id: &str, ip: &str) -> Result<(), String> {
        sqlx::query("UPDATE requests SET client_ip = ? WHERE id = ?")
            .bind(ip)
            .bind(request_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Records upstream timing and token detail for a finished (or failed)
    /// upstream leg; a later attempt overwrites earlier partial values.
    pub async fn set_request_details(
        &self,
        request_id: &str,
        first_byte_ms: i64,
        cached_tokens: u64,
        reasoning_tokens: u64,
    ) -> Result<(), String> {
        sqlx::query("UPDATE requests SET first_byte_ms = ?, cached_tokens = ?, reasoning_tokens = ? WHERE id = ?")
            .bind(first_byte_ms)
            .bind(cached_tokens as i64)
            .bind(reasoning_tokens as i64)
            .bind(request_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn save_request_body(&self, request_id: &str, body: &str) -> Result<(), String> {
        sqlx::query("INSERT INTO request_bodies (request_id, request_body, created_at) VALUES (?, ?, ?) ON CONFLICT(request_id) DO UPDATE SET request_body = excluded.request_body")
            .bind(request_id)
            .bind(body)
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn save_response_body(&self, request_id: &str, body: &str) -> Result<(), String> {
        sqlx::query("INSERT INTO request_bodies (request_id, request_body, response_body, created_at) VALUES (?, '', ?, ?) ON CONFLICT(request_id) DO UPDATE SET response_body = excluded.response_body")
            .bind(request_id)
            .bind(body)
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Deletes request history older than `days` (attempts and bodies cascade).
    /// Batches keep each write short so console reads and inference writes are
    /// never starved by one giant DELETE; `days == 0` keeps history forever.
    pub async fn prune_requests(&self, days: u64) -> Result<u64, String> {
        if days == 0 {
            return Ok(0);
        }
        let cutoff = (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        let mut deleted = 0;
        loop {
            let affected = sqlx::query(
                "DELETE FROM requests WHERE id IN (SELECT id FROM requests WHERE created_at < ? LIMIT 500)",
            )
            .bind(&cutoff)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .rows_affected();
            deleted += affected;
            if affected < 500 {
                return Ok(deleted);
            }
        }
    }
    /// Background retention sweep: reads `body_retention_days` (0/unset keeps
    /// forever), prunes in batches and notifies open consoles when rows went.
    pub async fn enforce_retention(&self, events: &crate::events::Events) -> Result<u64, String> {
        let days = self
            .settings()
            .await?
            .get("body_retention_days")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let deleted = self.prune_requests(days).await?;
        if deleted > 0 {
            events.publish(crate::events::REQUESTS_CHANGED);
        }
        Ok(deleted)
    }
    pub async fn request_bodies(
        &self,
        request_id: &str,
    ) -> Result<Option<(String, Option<String>)>, String> {
        let row = sqlx::query(
            "SELECT request_body, response_body FROM request_bodies WHERE request_id = ?",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(|row| (row.get("request_body"), row.get("response_body"))))
    }
    pub async fn requests(&self) -> Result<Vec<RequestRecord>, String> {
        let rows = sqlx::query("SELECT r.id, r.protocol, COALESCE((SELECT a.model FROM attempts a WHERE a.request_id = r.id AND a.status IN ('success', 'streaming') ORDER BY a.id DESC LIMIT 1), r.model) model, r.model requested_model, r.route, r.status, r.input_tokens, r.output_tokens, r.cost_micros, r.latency_ms, r.created_at, r.client_ip, r.first_byte_ms, r.cached_tokens, r.reasoning_tokens, EXISTS(SELECT 1 FROM request_bodies b WHERE b.request_id = r.id) has_body, (SELECT p.name FROM attempts a JOIN providers p ON p.id = a.provider_id WHERE a.request_id = r.id AND a.status IN ('success', 'streaming') ORDER BY a.id DESC LIMIT 1) provider, k.name key, COALESCE((SELECT a.error FROM attempts a WHERE a.request_id = r.id AND a.error IS NOT NULL AND a.status != 'retry' ORDER BY a.id DESC LIMIT 1), (SELECT a.error FROM attempts a WHERE a.request_id = r.id AND a.error IS NOT NULL ORDER BY a.id DESC LIMIT 1)) error FROM requests r LEFT JOIN client_keys k ON k.id = r.client_key_id ORDER BY r.created_at DESC LIMIT 200").fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| RequestRecord {
                id: row.get("id"),
                protocol: row.get("protocol"),
                model: row.get("model"),
                requested_model: row.get("requested_model"),
                status: row.get("status"),
                input_tokens: row.get("input_tokens"),
                output_tokens: row.get("output_tokens"),
                cost_micros: row.get("cost_micros"),
                latency_ms: row.get("latency_ms"),
                created_at: row.get("created_at"),
                route: row.get("route"),
                provider: row.get("provider"),
                key: row.get("key"),
                client_ip: row.get("client_ip"),
                first_byte_ms: row.get("first_byte_ms"),
                cached_tokens: row.get("cached_tokens"),
                reasoning_tokens: row.get("reasoning_tokens"),
                has_body: row.get::<i64, _>("has_body") != 0,
                error: row.get("error"),
            })
            .collect())
    }
}
