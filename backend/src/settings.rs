use crate::Store;
use serde_json::Value;
use sqlx::Row;

/// (key, inclusive maximum); minimum is always 1.
const NUMERIC_SETTINGS: &[(&str, u64)] = &[
    ("connect_timeout_seconds", 60),
    ("first_byte_timeout_seconds", 600),
    ("stream_idle_timeout_seconds", 600),
    ("max_attempts", 16),
    ("retry_after_cap_seconds", 60),
];
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
        // Range contract shared with snapshot::bounded_seconds so a persisted
        // value can never disagree with what the runtime would accept.
        if let Some((_, max)) = NUMERIC_SETTINGS.iter().find(|(name, _)| *name == key) {
            match value.as_u64() {
                Some(number) if (1..=*max).contains(&number) => {}
                _ => return Err(format!("{key} must be an integer between 1 and {max}")),
            }
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
        // `None` means no time window at all: "all" aggregates every recorded request
        // through the same day buckets, so the series/by_model/by_provider/by_route
        // shapes stay identical to the windowed periods.
        let (window, bucket) = match period {
            "24 hours" => (Some("-24 hours"), "%Y-%m-%dT%H:00:00Z"),
            "7 days" => (Some("-7 days"), "%Y-%m-%dT00:00:00Z"),
            "30 days" => (Some("-30 days"), "%Y-%m-%dT00:00:00Z"),
            "90 days" => (Some("-90 days"), "%Y-%m-%dT00:00:00Z"),
            "all" => (None, "%Y-%m-%dT00:00:00Z"),
            _ => return Err("unsupported analytics period".into()),
        };
        let time_filter = |column: &str, connector: &str| match window {
            Some(_) => format!(" {connector} {column} >= datetime('now', ?)"),
            None => String::new(),
        };
        let totals_sql = format!(
            "SELECT COUNT(*) requests, COALESCE(SUM(input_tokens), 0) input_tokens, COALESCE(SUM(output_tokens), 0) output_tokens, COALESCE(SUM(cost_micros), 0) cost_micros, COALESCE(SUM(status = 'success'), 0) successful, COALESCE(AVG(latency_ms), 0.0) latency_ms FROM requests{}",
            time_filter("created_at", "WHERE")
        );
        let series_sql = format!(
            "SELECT strftime(?, created_at) time, COUNT(*) requests, COALESCE(SUM(status = 'success'), 0) successful, COALESCE(SUM(cost_micros), 0) cost_micros, COALESCE(AVG(latency_ms), 0.0) latency_ms FROM requests{} GROUP BY time ORDER BY time",
            time_filter("created_at", "WHERE")
        );
        let by_model_sql = format!(
            "SELECT model, COUNT(*) requests FROM requests{} GROUP BY model ORDER BY requests DESC",
            time_filter("created_at", "WHERE")
        );
        let by_provider_sql = format!(
            "SELECT a.provider_id, COUNT(*) requests, COALESCE(SUM(r.status = 'success'), 0) successful, COALESCE(AVG(a.latency_ms), 0) latency_ms FROM attempts a JOIN requests r ON r.id = a.request_id WHERE a.provider_id IS NOT NULL{} GROUP BY a.provider_id",
            time_filter("r.created_at", "AND")
        );
        // Route performance must be aggregated over the whole window instead of the
        // newest request page, so routes whose recent traffic fell out of that page
        // still report their real totals. Direct (unrouted) requests are excluded.
        let by_route_sql = format!(
            "SELECT route, COUNT(*) requests, COALESCE(SUM(status = 'success'), 0) successful, COALESCE(AVG(latency_ms), 0.0) latency_ms FROM requests WHERE route IS NOT NULL AND route != ''{} GROUP BY route ORDER BY requests DESC",
            time_filter("created_at", "AND")
        );
        let mut totals = sqlx::query(&totals_sql);
        let mut series = sqlx::query(&series_sql).bind(bucket);
        let mut by_model = sqlx::query(&by_model_sql);
        let mut by_provider = sqlx::query(&by_provider_sql);
        let mut by_route = sqlx::query(&by_route_sql);
        if let Some(window) = window {
            totals = totals.bind(window);
            series = series.bind(window);
            by_model = by_model.bind(window);
            by_provider = by_provider.bind(window);
            by_route = by_route.bind(window);
        }
        let row = totals
            .fetch_one(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        let series = series.fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|row| serde_json::json!({"time":row.get::<String, _>("time"),"requests":row.get::<i64, _>("requests"),"successful":row.get::<i64, _>("successful"),"cost_micros":row.get::<i64, _>("cost_micros"),"latency_ms":row.get::<f64, _>("latency_ms")})).collect::<Vec<_>>();
        let by_model = by_model.fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|row| serde_json::json!({"model":row.get::<String, _>("model"),"requests":row.get::<i64, _>("requests")})).collect::<Vec<_>>();
        let by_provider = by_provider.fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|row| serde_json::json!({"provider_id":row.get::<i64, _>("provider_id"),"requests":row.get::<i64, _>("requests"),"successful":row.get::<i64, _>("successful"),"latency_ms":row.get::<f64, _>("latency_ms")})).collect::<Vec<_>>();
        let by_route = by_route.fetch_all(&self.pool).await.map_err(|e| e.to_string())?.into_iter().map(|row| serde_json::json!({"route":row.get::<String, _>("route"),"requests":row.get::<i64, _>("requests"),"successful":row.get::<i64, _>("successful"),"latency_ms":row.get::<f64, _>("latency_ms")})).collect::<Vec<_>>();
        Ok(
            serde_json::json!({"requests":row.get::<i64, _>("requests"),"input_tokens":row.get::<i64, _>("input_tokens"),"output_tokens":row.get::<i64, _>("output_tokens"),"cost_micros":row.get::<i64, _>("cost_micros"),"successful":row.get::<i64, _>("successful"),"latency_ms":row.get::<f64, _>("latency_ms"),"series":series,"by_model":by_model,"by_provider":by_provider,"by_route":by_route}),
        )
    }
}
