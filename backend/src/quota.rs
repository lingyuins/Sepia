use crate::Store;
use chrono::Utc;
use sqlx::Row;
use uuid::Uuid;

impl Store {
    pub async fn authorize_model(&self, key_id: i64, model: &str) -> Result<bool, String> {
        let models: String =
            sqlx::query("SELECT models FROM client_keys WHERE id = ? AND enabled = 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("unknown client key")?
                .get("models");
        let models: Vec<String> = serde_json::from_str(&models).map_err(|e| e.to_string())?;
        Ok(models
            .iter()
            .any(|pattern| crate::server::glob(pattern, model)))
    }
    pub async fn reserve_quota(
        &self,
        key_id: i64,
        model: &str,
        tokens: i64,
    ) -> Result<String, String> {
        if tokens < 0 {
            return Err("invalid token reservation".into());
        }
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        let row = sqlx::query("SELECT token_limit, max_amount_micros, rpm, tpm FROM client_keys WHERE id = ? AND enabled = 1").bind(key_id).fetch_one(&mut *tx).await.map_err(|e| e.to_string())?;
        let period = Utc::now().format("%Y%m%d%H%M").to_string();
        sqlx::query(
            "INSERT INTO quota_usage (client_key_id, period) VALUES (?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(key_id)
        .bind(&period)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        let usage = sqlx::query(
            "SELECT requests, tokens FROM quota_usage WHERE client_key_id = ? AND period = ?",
        )
        .bind(key_id)
        .bind(&period)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        let requests: i64 = usage.get("requests");
        let used: i64 =
            sqlx::query("SELECT COALESCE(SUM(tokens), 0) FROM quota_usage WHERE client_key_id = ?")
                .bind(key_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .get(0);
        if row.get::<i64, _>("rpm") > 0 && requests >= row.get::<i64, _>("rpm")
            || row.get::<i64, _>("tpm") > 0
                && usage.get::<i64, _>("tokens") + tokens > row.get::<i64, _>("tpm")
            || row.get::<i64, _>("token_limit") > 0
                && used + tokens > row.get::<i64, _>("token_limit")
        {
            return Err("quota exceeded".into());
        }
        if row.get::<i64, _>("max_amount_micros") > 0 {
            let spent: i64 = sqlx::query(
                "SELECT COALESCE(SUM(amount_micros), 0) FROM quota_usage WHERE client_key_id = ?",
            )
            .bind(key_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?
            .get(0);
            let cost: i64 = sqlx::query("SELECT COALESCE(MAX((pm.input_price_micros + pm.output_price_micros) * ? / 2000000), 0) FROM provider_models pm WHERE pm.model = ?").bind(tokens).bind(model).fetch_one(&mut *tx).await.map_err(|e| e.to_string())?.get(0);
            if spent + cost > row.get::<i64, _>("max_amount_micros") {
                return Err("amount quota exceeded".into());
            }
        }
        sqlx::query("UPDATE quota_usage SET requests = requests + 1, tokens = tokens + ? WHERE client_key_id = ? AND period = ?").bind(tokens).bind(key_id).bind(&period).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO quota_reservations (id, client_key_id, model, token_estimate, amount_micros, created_at) VALUES (?, ?, ?, ?, 0, ?)").bind(&id).bind(key_id).bind(model).bind(tokens).bind(Utc::now().to_rfc3339()).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub async fn settle_quota(&self, reservation: &str, actual_tokens: i64) -> Result<(), String> {
        if actual_tokens < 0 {
            return Err("invalid actual usage".into());
        }
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        let row = sqlx::query(
            "SELECT client_key_id, token_estimate, created_at FROM quota_reservations WHERE id = ?",
        )
        .bind(reservation)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("reservation not found")?;
        let key_id: i64 = row.get("client_key_id");
        let reserved: i64 = row.get("token_estimate");
        let period = chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .map_err(|e| e.to_string())?
            .format("%Y%m%d%H%M")
            .to_string();
        sqlx::query("UPDATE quota_usage SET tokens = MAX(0, tokens + ?) WHERE client_key_id = ? AND period = ?").bind(actual_tokens - reserved).bind(key_id).bind(period).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM quota_reservations WHERE id = ?")
            .bind(reservation)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }
}
