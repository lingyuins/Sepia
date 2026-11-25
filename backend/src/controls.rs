use crate::Store;

impl Store {
    pub async fn set_provider_enabled(&self, id: i64, enabled: bool) -> Result<(), String> {
        sqlx::query("UPDATE providers SET enabled = ? WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn set_route_enabled(&self, id: i64, enabled: bool) -> Result<(), String> {
        sqlx::query("UPDATE routes SET enabled = ? WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn delete_route(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM routes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn delete_all_routes(&self) -> Result<u64, String> {
        let result = sqlx::query("DELETE FROM routes")
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(result.rows_affected())
    }
    pub async fn set_client_key_enabled(&self, id: i64, enabled: bool) -> Result<(), String> {
        sqlx::query("UPDATE client_keys SET enabled = ? WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
