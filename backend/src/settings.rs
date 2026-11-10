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
    pub async fn analytics(&self) -> Result<serde_json::Value, String> {
        let row = sqlx::query("SELECT COUNT(*) requests, COALESCE(SUM(input_tokens), 0) input_tokens, COALESCE(SUM(output_tokens), 0) output_tokens, COALESCE(SUM(cost_micros), 0) cost_micros, COALESCE(SUM(status = 'success'), 0) successful FROM requests").fetch_one(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(
            serde_json::json!({"requests":row.get::<i64, _>("requests"),"input_tokens":row.get::<i64, _>("input_tokens"),"output_tokens":row.get::<i64, _>("output_tokens"),"cost_micros":row.get::<i64, _>("cost_micros"),"successful":row.get::<i64, _>("successful")}),
        )
    }
}
