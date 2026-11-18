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
        } = input;
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query(
            "UPDATE providers SET name = ?, protocol = ?, endpoint = ?, group_id = ?, enabled = ?, plugin_id = ? WHERE id = ?",
        )
        .bind(name)
        .bind(protocol.as_str())
        .bind(endpoint)
        .bind(group)
        .bind(enabled)
        .bind(plugin_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        for model in models {
            sqlx::query("INSERT INTO provider_models (provider_id, model) VALUES (?, ?) ON CONFLICT(provider_id, model) DO NOTHING").bind(id).bind(model).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        }
        sqlx::query("DELETE FROM provider_models WHERE provider_id = ? AND model NOT IN (SELECT value FROM json_each(?))").bind(id).bind(serde_json::to_string(models).map_err(|e| e.to_string())?).execute(&mut *tx).await.map_err(|e| e.to_string())?;
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
        } = input;
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("UPDATE routes SET slug = ?, name = ?, description = ?, patterns = ?, is_default = ?, list_members = ?, enabled = ? WHERE id = ?").bind(slug).bind(name).bind(description).bind(serde_json::to_string(patterns).map_err(|e| e.to_string())?).bind(is_default).bind(list_members).bind(enabled).bind(id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM route_candidates WHERE route_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        for (position, (provider, model)) in candidates.iter().enumerate() {
            sqlx::query("INSERT INTO route_candidates (route_id, provider_id, model, position) VALUES (?, ?, ?, ?)").bind(id).bind(provider).bind(model).bind(position as i64).execute(&mut *tx).await.map_err(|e| e.to_string())?;
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
