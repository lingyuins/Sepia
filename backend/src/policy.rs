use crate::Store;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{net::IpAddr, str::FromStr};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientPolicy {
    pub ip_whitelist: Vec<String>,
    pub model_quotas: std::collections::HashMap<String, ModelQuota>,
    pub allowed_groups: Vec<String>,
    pub excluded_providers: Vec<i64>,
    /// Referenced model-group ids; their members widen the key's model scope.
    #[serde(default)]
    pub model_groups: Vec<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelGroup {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub members: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelQuota {
    pub rpm: i64,
    pub tpm: i64,
}
impl Store {
    pub async fn set_client_policy(
        &self,
        key_id: i64,
        policy: &ClientPolicy,
    ) -> Result<(), String> {
        for network in &policy.ip_whitelist {
            if network.parse::<IpAddr>().is_err() && ipnet::IpNet::from_str(network).is_err() {
                return Err("invalid IP whitelist".into());
            }
        }
        sqlx::query("INSERT INTO client_key_policies (client_key_id, ip_whitelist, model_quotas, allowed_groups, excluded_providers, model_groups) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(client_key_id) DO UPDATE SET ip_whitelist = excluded.ip_whitelist, model_quotas = excluded.model_quotas, allowed_groups = excluded.allowed_groups, excluded_providers = excluded.excluded_providers, model_groups = excluded.model_groups").bind(key_id).bind(serde_json::to_string(&policy.ip_whitelist).map_err(|e| e.to_string())?).bind(serde_json::to_string(&policy.model_quotas).map_err(|e| e.to_string())?).bind(serde_json::to_string(&policy.allowed_groups).map_err(|e| e.to_string())?).bind(serde_json::to_string(&policy.excluded_providers).map_err(|e| e.to_string())?).bind(serde_json::to_string(&policy.model_groups).map_err(|e| e.to_string())?).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn client_policy(&self, key_id: i64) -> Result<ClientPolicy, String> {
        let row = sqlx::query("SELECT ip_whitelist, model_quotas, allowed_groups, excluded_providers, model_groups FROM client_key_policies WHERE client_key_id = ?").bind(key_id).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;
        row.map(|row| {
            Ok(ClientPolicy {
                ip_whitelist: serde_json::from_str(row.get("ip_whitelist"))
                    .map_err(|e| e.to_string())?,
                model_quotas: serde_json::from_str(row.get("model_quotas"))
                    .map_err(|e| e.to_string())?,
                allowed_groups: serde_json::from_str(row.get("allowed_groups"))
                    .map_err(|e| e.to_string())?,
                excluded_providers: serde_json::from_str(row.get("excluded_providers"))
                    .map_err(|e| e.to_string())?,
                model_groups: serde_json::from_str(row.get("model_groups"))
                    .map_err(|e| e.to_string())?,
            })
        })
        .unwrap_or_else(|| Ok(ClientPolicy::default()))
    }
    pub fn ip_allowed(policy: &ClientPolicy, address: IpAddr) -> bool {
        policy.ip_whitelist.is_empty()
            || policy.ip_whitelist.iter().any(|network| {
                network.parse::<IpAddr>().ok() == Some(address)
                    || ipnet::IpNet::from_str(network)
                        .ok()
                        .is_some_and(|network| network.contains(&address))
            })
    }
    pub async fn provider_allowed(&self, key_id: i64, provider_id: i64) -> Result<bool, String> {
        let policy = self.client_policy(key_id).await?;
        if policy.excluded_providers.contains(&provider_id) {
            return Ok(false);
        }
        if policy.allowed_groups.is_empty() {
            return Ok(true);
        }
        let group: String = sqlx::query("SELECT group_id FROM providers WHERE id = ?")
            .bind(provider_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("provider not found")?
            .get("group_id");
        Ok(policy.allowed_groups.contains(&group))
    }
    pub async fn channel_groups(&self) -> Result<Vec<crate::store::ChannelGroup>, String> {
        sqlx::query("SELECT id, name, description FROM channel_groups ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|row| {
                Ok(crate::store::ChannelGroup {
                    id: row.get("id"),
                    name: row.get("name"),
                    description: row.get("description"),
                })
            })
            .collect()
    }
    pub async fn set_group(&self, id: &str, name: &str, description: &str) -> Result<(), String> {
        sqlx::query("INSERT INTO channel_groups (id, name, description) VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description").bind(id).bind(name).bind(description).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn set_provider_group(&self, provider_id: i64, group_id: &str) -> Result<(), String> {
        sqlx::query("UPDATE providers SET group_id = ? WHERE id = ?")
            .bind(group_id)
            .bind(provider_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn model_groups(&self) -> Result<Vec<ModelGroup>, String> {
        sqlx::query("SELECT id, name, description, members, created_at, updated_at FROM model_groups ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|row| {
                Ok(ModelGroup {
                    id: row.get("id"),
                    name: row.get("name"),
                    description: row.get("description"),
                    members: serde_json::from_str(row.get("members")).map_err(|e| e.to_string())?,
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect()
    }
    fn validate_model_group(name: &str, members: &[String]) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("model group name required".into());
        }
        if members.iter().any(|member| member.trim().is_empty()) {
            return Err("model group members must be non-empty".into());
        }
        Ok(())
    }
    pub async fn create_model_group(
        &self,
        name: &str,
        description: &str,
        members: &[String],
    ) -> Result<ModelGroup, String> {
        Self::validate_model_group(name, members)?;
        let id =
            sqlx::query("INSERT INTO model_groups (name, description, members) VALUES (?, ?, ?)")
                .bind(name.trim())
                .bind(description.trim())
                .bind(serde_json::to_string(members).map_err(|e| e.to_string())?)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?
                .last_insert_rowid();
        self.model_groups()
            .await?
            .into_iter()
            .find(|group| group.id == id)
            .ok_or("model group not saved".into())
    }
    pub async fn update_model_group(
        &self,
        id: i64,
        name: &str,
        description: &str,
        members: &[String],
    ) -> Result<ModelGroup, String> {
        Self::validate_model_group(name, members)?;
        let result = sqlx::query("UPDATE model_groups SET name = ?, description = ?, members = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(name.trim())
            .bind(description.trim())
            .bind(serde_json::to_string(members).map_err(|e| e.to_string())?)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("model group not found".into());
        }
        self.model_groups()
            .await?
            .into_iter()
            .find(|group| group.id == id)
            .ok_or("model group not found".into())
    }
    /// Deleting a group still referenced by a client-key policy is rejected;
    /// references are stored by id so renames never orphan them.
    pub async fn delete_model_group(&self, id: i64) -> Result<(), String> {
        let rows = sqlx::query("SELECT client_key_id, model_groups FROM client_key_policies")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for row in rows {
            let referenced: Vec<i64> =
                serde_json::from_str(row.get("model_groups")).unwrap_or_default();
            if referenced.contains(&id) {
                return Err("model group is referenced by a client key".into());
            }
        }
        let result = sqlx::query("DELETE FROM model_groups WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("model group not found".into());
        }
        Ok(())
    }
    /// Members of every model group referenced by a key's policy, deduplicated
    /// in reference order. Unknown group ids contribute nothing.
    pub async fn model_group_members(&self, key_id: i64) -> Result<Vec<String>, String> {
        let policy = self.client_policy(key_id).await?;
        if policy.model_groups.is_empty() {
            return Ok(Vec::new());
        }
        let groups = self.model_groups().await?;
        let mut members: Vec<String> = Vec::new();
        for id in &policy.model_groups {
            if let Some(group) = groups.iter().find(|group| group.id == *id) {
                for member in &group.members {
                    if !members.contains(member) {
                        members.push(member.clone());
                    }
                }
            }
        }
        Ok(members)
    }
}
