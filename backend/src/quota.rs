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
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|e| e.to_string())?;
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
        let policy =
            sqlx::query("SELECT model_quotas FROM client_key_policies WHERE client_key_id = ?")
                .bind(key_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        let quotas: std::collections::HashMap<String, crate::policy::ModelQuota> = policy
            .map(|row| serde_json::from_str(row.get("model_quotas")).map_err(|e| e.to_string()))
            .transpose()?
            .unwrap_or_default();
        let model_quota = quotas.get(model).copied();
        let rpm = model_quota
            .map(|quota| quota.rpm)
            .unwrap_or_else(|| row.get("rpm"));
        let tpm = model_quota
            .map(|quota| quota.tpm)
            .unwrap_or_else(|| row.get("tpm"));
        let used: i64 =
            sqlx::query("SELECT COALESCE(SUM(tokens), 0) FROM quota_usage WHERE client_key_id = ?")
                .bind(key_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .get(0);
        if rpm > 0 && requests >= rpm
            || tpm > 0 && usage.get::<i64, _>("tokens") + tokens > tpm
            || row.get::<i64, _>("token_limit") > 0
                && used + tokens > row.get::<i64, _>("token_limit")
        {
            return Err("quota exceeded".into());
        }
        let day = Utc::now().format("%Y%m%d").to_string();
        sqlx::query(
            "INSERT INTO quota_usage (client_key_id, period) VALUES (?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(key_id)
        .bind(&day)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        let mut reserved_cost = 0;
        if row.get::<i64, _>("max_amount_micros") > 0 {
            let spent: i64 = sqlx::query("SELECT COALESCE(SUM(amount_micros), 0) FROM quota_usage WHERE client_key_id = ? AND period = ?").bind(key_id).bind(&day).fetch_one(&mut *tx).await.map_err(|e| e.to_string())?.get(0);
            reserved_cost = sqlx::query("SELECT COALESCE(MAX((pm.input_price_micros + pm.output_price_micros) * ? / 2000000), 0) FROM provider_models pm WHERE pm.model = ?").bind(tokens).bind(model).fetch_one(&mut *tx).await.map_err(|e| e.to_string())?.get(0);
            if reserved_cost == 0 {
                return Err("amount quota requires model pricing".into());
            }
            if spent + reserved_cost > row.get::<i64, _>("max_amount_micros") {
                return Err("amount quota exceeded".into());
            }
        }
        sqlx::query("UPDATE quota_usage SET requests = requests + 1, tokens = tokens + ? WHERE client_key_id = ? AND period = ?").bind(tokens).bind(key_id).bind(&period).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("UPDATE quota_usage SET amount_micros = amount_micros + ? WHERE client_key_id = ? AND period = ?").bind(reserved_cost).bind(key_id).bind(&day).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO quota_reservations (id, client_key_id, model, token_estimate, amount_micros, created_at) VALUES (?, ?, ?, ?, ?, ?)").bind(&id).bind(key_id).bind(model).bind(tokens).bind(reserved_cost).bind(Utc::now().to_rfc3339()).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(id)
    }
    async fn settle_quota_inner(
        &self,
        reservation: &str,
        actual_tokens: i64,
        actual_cost_override: Option<i64>,
    ) -> Result<bool, String> {
        if actual_tokens < 0 {
            return Err("invalid actual usage".into());
        }
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|e| e.to_string())?;
        // A missing reservation returns Ok(false): hard-deleting a client
        // key clears its open reservations, and the guard verifies the key
        // is actually gone before accepting an unquota'd settlement.
        let Some(row) = sqlx::query(
            "SELECT client_key_id, model, token_estimate, amount_micros, created_at FROM quota_reservations WHERE id = ?",
        )
        .bind(reservation)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        else {
            return Ok(false);
        };
        let key_id: i64 = row.get("client_key_id");
        let reserved: i64 = row.get("token_estimate");
        let period = chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .map_err(|e| e.to_string())?
            .format("%Y%m%d%H%M")
            .to_string();
        sqlx::query("UPDATE quota_usage SET tokens = MAX(0, tokens + ?) WHERE client_key_id = ? AND period = ?").bind(actual_tokens - reserved).bind(key_id).bind(period).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        let reserved_cost: i64 = row.get("amount_micros");
        let reserved_tokens: i64 = row.get("token_estimate");
        let day = chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .map_err(|e| e.to_string())?
            .format("%Y%m%d")
            .to_string();
        let actual_cost = actual_cost_override.unwrap_or(if reserved_cost > 0 {
            ((reserved_cost as i128 * actual_tokens as i128) / reserved_tokens.max(1) as i128)
                as i64
        } else {
            0
        });
        sqlx::query("UPDATE quota_usage SET amount_micros = MAX(0, amount_micros + ?) WHERE client_key_id = ? AND period = ?").bind(actual_cost - reserved_cost).bind(key_id).bind(day).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM quota_reservations WHERE id = ?")
            .bind(reservation)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }
    pub async fn settle_quota(
        &self,
        reservation: &str,
        actual_tokens: i64,
    ) -> Result<bool, String> {
        self.settle_quota_inner(reservation, actual_tokens, None)
            .await
    }
    pub async fn settle_quota_amount(
        &self,
        reservation: &str,
        actual_tokens: i64,
        actual_cost: i64,
    ) -> Result<bool, String> {
        self.settle_quota_inner(reservation, actual_tokens, Some(actual_cost))
            .await
    }
    /// Cost in micros for one request. `cached_tokens` bills at the
    /// cache-read price when configured (>0), else at the normal input
    /// price. `cached_included` says whether `input_tokens` already counts
    /// the cached tokens (OpenAI/Gemini/Responses: yes; Anthropic reports
    /// `input_tokens` excluding cache reads: no).
    pub async fn model_cost(
        &self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cached_tokens: u64,
        cached_included: bool,
    ) -> Result<i64, String> {
        // Same "configured price" gate as `model_prices` (both columns > 0):
        // a channel holding only a sync-filled cache-read price must never
        // bill uncached/output tokens at zero while the catalog shows unset.
        let row = sqlx::query("SELECT input_price_micros, output_price_micros, cache_read_price_micros FROM provider_models WHERE model = ? AND input_price_micros > 0 AND output_price_micros > 0 ORDER BY (input_price_micros + output_price_micros) DESC LIMIT 1").bind(model).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(row
            .map(|row| {
                let input_price = row.get::<i64, _>("input_price_micros") as i128;
                let output_price = row.get::<i64, _>("output_price_micros") as i128;
                let cache_price = row.get::<i64, _>("cache_read_price_micros") as i128;
                let cache_price = if cache_price > 0 {
                    cache_price
                } else {
                    input_price
                };
                let cached = cached_tokens as i128;
                let uncached = if cached_included {
                    (input_tokens as i128 - cached).max(0)
                } else {
                    input_tokens as i128
                };
                ((uncached * input_price
                    + cached * cache_price
                    + output_tokens as i128 * output_price)
                    / 1_000_000) as i64
            })
            .unwrap_or(0))
    }
    pub async fn set_model_price(
        &self,
        provider_id: i64,
        model: &str,
        input_price_micros: i64,
        output_price_micros: i64,
        cache_read_price_micros: i64,
    ) -> Result<(), String> {
        if input_price_micros <= 0 || output_price_micros <= 0 {
            return Err("model pricing required".into());
        }
        if cache_read_price_micros < 0 {
            return Err("cache read price cannot be negative".into());
        }
        sqlx::query("UPDATE provider_models SET input_price_micros = ?, output_price_micros = ?, cache_read_price_micros = ? WHERE provider_id = ? AND model = ?").bind(input_price_micros).bind(output_price_micros).bind(cache_read_price_micros).bind(provider_id).bind(model).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn amount_used(&self, key_id: i64) -> Result<i64, String> {
        Ok(sqlx::query("SELECT COALESCE(SUM(amount_micros), 0) FROM quota_usage WHERE client_key_id = ? AND period = ?").bind(key_id).bind(Utc::now().format("%Y%m%d").to_string()).fetch_one(&self.pool).await.map_err(|e| e.to_string())?.get(0))
    }
}
