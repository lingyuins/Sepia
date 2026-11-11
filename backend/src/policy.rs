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
        sqlx::query("INSERT INTO client_key_policies (client_key_id, ip_whitelist, model_quotas, allowed_groups, excluded_providers) VALUES (?, ?, ?, ?, ?) ON CONFLICT(client_key_id) DO UPDATE SET ip_whitelist = excluded.ip_whitelist, model_quotas = excluded.model_quotas, allowed_groups = excluded.allowed_groups, excluded_providers = excluded.excluded_providers").bind(key_id).bind(serde_json::to_string(&policy.ip_whitelist).map_err(|e| e.to_string())?).bind(serde_json::to_string(&policy.model_quotas).map_err(|e| e.to_string())?).bind(serde_json::to_string(&policy.allowed_groups).map_err(|e| e.to_string())?).bind(serde_json::to_string(&policy.excluded_providers).map_err(|e| e.to_string())?).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn client_policy(&self, key_id: i64) -> Result<ClientPolicy, String> {
        let row = sqlx::query("SELECT ip_whitelist, model_quotas, allowed_groups, excluded_providers FROM client_key_policies WHERE client_key_id = ?").bind(key_id).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;
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
}
