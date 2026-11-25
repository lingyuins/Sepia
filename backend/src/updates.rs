use crate::{ProviderProtocol, Store};

pub struct RouteUpdate<'a> {
    pub slug: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub patterns: &'a [String],
    pub is_default: bool,
    pub list_members: bool,
    pub enabled: bool,
    pub candidates: &'a [(i64, String)],
    /// `None` keeps the stored strategy.
    pub strategy: Option<&'a str>,
    /// Parallel to `candidates`; `None` keeps stored weights by candidate
    /// identity, missing entries default to weight 1.
    pub weights: Option<&'a [i64]>,
    /// Built-in outbound request rewriting (replaces the stored config).
    pub rewrite: &'a crate::rewrite::RewriteConfig,
}
pub struct ProviderUpdate<'a> {
    pub id: i64,
    pub name: &'a str,
    pub protocol: ProviderProtocol,
    pub endpoint: &'a str,
    pub models: &'a [String],
    pub group: &'a str,
    pub enabled: bool,
    pub plugin_id: Option<&'a str>,
    pub mode: &'a str,
    /// Built-in outbound request rewriting (replaces the stored config).
    pub rewrite: &'a crate::rewrite::RewriteConfig,
}

impl Store {
    pub async fn update_provider(&self, input: ProviderUpdate<'_>) -> Result<(), String> {
        let ProviderUpdate {
            id,
            name,
            protocol,
            endpoint,
            models,
            group,
            enabled,
            plugin_id,
            mode,
            rewrite,
        } = input;
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query(
            "UPDATE providers SET name = ?, protocol = ?, endpoint = ?, group_id = ?, enabled = ?, plugin_id = ?, mode = ?, headers = ?, param_replace = ?, prompt_replace = ?, param_defaults = ? WHERE id = ?",
        )
        .bind(name)
        .bind(protocol.as_str())
        .bind(endpoint)
        .bind(group)
        .bind(enabled)
        .bind(plugin_id)
        .bind(mode)
        .bind(serde_json::to_string(&rewrite.headers).map_err(|e| e.to_string())?)
        .bind(serde_json::to_string(&rewrite.param_replace).map_err(|e| e.to_string())?)
        .bind(serde_json::to_string(&rewrite.prompt_replace).map_err(|e| e.to_string())?)
        .bind(serde_json::to_string(&rewrite.param_defaults).map_err(|e| e.to_string())?)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        for model in models {
            sqlx::query("INSERT INTO provider_models (provider_id, model) VALUES (?, ?) ON CONFLICT(provider_id, model) DO NOTHING").bind(id).bind(model).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        }
        sqlx::query("DELETE FROM provider_models WHERE provider_id = ? AND model NOT IN (SELECT value FROM json_each(?))").bind(id).bind(serde_json::to_string(models).map_err(|e| e.to_string())?).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        // Models removed from the catalog can no longer be served: drop their
        // route candidates, then remove only the affected routes left without
        // any candidate. Pre-existing empty routes (update_route accepts an
        // empty candidate list) are unrelated to this save and stay untouched.
        let models_json = serde_json::to_string(models).map_err(|e| e.to_string())?;
        let affected: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT route_id FROM route_candidates WHERE provider_id = ? AND model NOT IN (SELECT value FROM json_each(?))",
        )
        .bind(id)
        .bind(&models_json)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        sqlx::query(
            "DELETE FROM route_candidates WHERE provider_id = ? AND model NOT IN (SELECT value FROM json_each(?))",
        )
        .bind(id)
        .bind(&models_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        for route_id in affected {
            sqlx::query(
                "DELETE FROM routes WHERE id = ? AND NOT EXISTS (SELECT 1 FROM route_candidates WHERE route_id = ?)",
            )
            .bind(route_id)
            .bind(route_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn update_route(&self, id: i64, input: RouteUpdate<'_>) -> Result<(), String> {
        let RouteUpdate {
            slug,
            name,
            description,
            patterns,
            is_default,
            list_members,
            enabled,
            candidates,
            strategy,
            weights,
            rewrite,
        } = input;
        if let Some(strategy) = strategy
            && crate::strategy::Strategy::parse(strategy).is_none()
        {
            return Err(format!("unknown route strategy: {strategy}"));
        }
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        type StoredRoute = (String, Vec<(i64, String, i64)>);
        let stored: Option<StoredRoute> = if strategy.is_none() || weights.is_none() {
            let strategy: String = sqlx::query_scalar("SELECT strategy FROM routes WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("unknown route")?;
            let rows: Vec<(i64, String, i64)> = sqlx::query_as(
                "SELECT provider_id, model, weight FROM route_candidates WHERE route_id = ?",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            Some((strategy, rows))
        } else {
            None
        };
        let strategy = strategy.unwrap_or_else(|| {
            stored
                .as_ref()
                .map(|(strategy, _)| strategy.as_str())
                .unwrap_or("priority")
        });
        sqlx::query("UPDATE routes SET slug = ?, name = ?, description = ?, patterns = ?, is_default = ?, list_members = ?, enabled = ?, strategy = ?, headers = ?, param_replace = ?, prompt_replace = ?, param_defaults = ? WHERE id = ?").bind(slug).bind(name).bind(description).bind(serde_json::to_string(patterns).map_err(|e| e.to_string())?).bind(is_default).bind(list_members).bind(enabled).bind(strategy).bind(serde_json::to_string(&rewrite.headers).map_err(|e| e.to_string())?).bind(serde_json::to_string(&rewrite.param_replace).map_err(|e| e.to_string())?).bind(serde_json::to_string(&rewrite.prompt_replace).map_err(|e| e.to_string())?).bind(serde_json::to_string(&rewrite.param_defaults).map_err(|e| e.to_string())?).bind(id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM route_candidates WHERE route_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        for (position, (provider, model)) in candidates.iter().enumerate() {
            let weight = match weights {
                Some(weights) => weights.get(position).copied().unwrap_or(1),
                None => stored
                    .as_ref()
                    .and_then(|(_, rows)| {
                        rows.iter()
                            .find(|(saved_provider, saved_model, _)| {
                                saved_provider == provider && saved_model == model
                            })
                            .map(|(_, _, weight)| *weight)
                    })
                    .unwrap_or(1),
            }
            .clamp(1, 100);
            sqlx::query("INSERT INTO route_candidates (route_id, provider_id, model, position, weight) VALUES (?, ?, ?, ?, ?)").bind(id).bind(provider).bind(model).bind(position as i64).bind(weight).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn clear_requests(&self) -> Result<(), String> {
        sqlx::query("DELETE FROM requests")
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
