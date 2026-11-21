use crate::Store;
use serde_json::Value;
use sqlx::Row;

impl Store {
    pub async fn set_setting(&self, key: &str, value: &Value) -> Result<(), String> {
        if ![
            "connect_timeout_seconds",
            "first_byte_timeout_seconds",
            "stream_idle_timeout_seconds",
            "max_attempts",
            "retry_after_cap_seconds",
            "log_bodies",
            "body_retention_days",
            "model_normalize_router_prefixes",
            "model_normalize_functional_suffixes",
            "model_normalize_explicit_mappings",
            "ai_pool_enabled",
            "workspace_name",
            "gateway_deployment",
            "gateway_protocol",
            "gateway_host",
            "gateway_port",
        ]
        .contains(&key)
        {
            return Err("unsupported workspace setting".into());
        }
        sqlx::query("INSERT INTO workspace_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value").bind(key).bind(value.to_string()).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn settings(&self) -> Result<std::collections::HashMap<String, Value>, String> {
        let rows = sqlx::query("SELECT key, value FROM workspace_settings")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.get::<String, _>("key"),
                    serde_json::from_str(&row.get::<String, _>("value"))
                        .map_err(|e| e.to_string())?,
                ))
            })
            .collect()
    }
    pub async fn analytics_for(&self, period: &str) -> Result<serde_json::Value, String> {
        let (window, bucket) = match period {
            "24 hours" => ("-24 hours", "%Y-%m-%dT%H:00:00Z"),
            "7 days" => ("-7 days", "%Y-%m-%dT00:00:00Z"),
            "30 days" => ("-30 days", "%Y-%m-%dT00:00:00Z"),
            _ => return Err("unsupported analytics period".into()),
        };
        let row = sqlx::query("SELECT COUNT(*) requests, COALESCE(SUM(input_tokens), 0) input_tokens, COALESCE(SUM(output_tokens), 0) output_tokens, COALESCE(SUM(cost_micros), 0) cost_micros, COALESCE(SUM(status = 'success'), 0) successful, COALESCE(AVG(latency_ms), 0.0) latency_ms FROM requests WHERE created_at >= datetime('now', ?)").bind(window).fetch_one(&self.pool).await.map_err(|e| e.to_string())?;
        let series = sqlx::query("SELECT strftime(?, created_at) time, COUNT(*) requests, COALESCE(SUM(status = 'success'), 0) successful, COALESCE(SUM(cost_micros), 0) cost_micros, COALESCE(AVG(latency_ms), 0.0) latency_ms FROM requests WHERE created_at >= datetime('now', ?) GROUP BY time ORDER BY time").bind(bucket).bind(window).fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|row| serde_json::json!({"time":row.get::<String, _>("time"),"requests":row.get::<i64, _>("requests"),"successful":row.get::<i64, _>("successful"),"cost_micros":row.get::<i64, _>("cost_micros"),"latency_ms":row.get::<f64, _>("latency_ms")})).collect::<Vec<_>>();
        let by_model = sqlx::query("SELECT model, COUNT(*) requests FROM requests WHERE created_at >= datetime('now', ?) GROUP BY model ORDER BY requests DESC").bind(window).fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|row| serde_json::json!({"model":row.get::<String, _>("model"),"requests":row.get::<i64, _>("requests")})).collect::<Vec<_>>();
        let by_provider = sqlx::query("SELECT a.provider_id, COUNT(*) requests, COALESCE(SUM(r.status = 'success'), 0) successful, COALESCE(AVG(a.latency_ms), 0) latency_ms FROM attempts a JOIN requests r ON r.id = a.request_id WHERE a.provider_id IS NOT NULL AND r.created_at >= datetime('now', ?) GROUP BY a.provider_id").bind(window).fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|row| serde_json::json!({"provider_id":row.get::<i64, _>("provider_id"),"requests":row.get::<i64, _>("requests"),"successful":row.get::<i64, _>("successful"),"latency_ms":row.get::<f64, _>("latency_ms")})).collect::<Vec<_>>();
        Ok(
            serde_json::json!({"requests":row.get::<i64, _>("requests"),"input_tokens":row.get::<i64, _>("input_tokens"),"output_tokens":row.get::<i64, _>("output_tokens"),"cost_micros":row.get::<i64, _>("cost_micros"),"successful":row.get::<i64, _>("successful"),"latency_ms":row.get::<f64, _>("latency_ms"),"series":series,"by_model":by_model,"by_provider":by_provider}),
        )
    }
}
