use serde_json::{Value, json};
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct StoredResponse {
    pub id: String,
    pub model: String,
    pub status: String,
    pub background: bool,
    pub conversation_id: Option<String>,
    pub previous_response_id: Option<String>,
    pub request_json: Value,
    pub response_json: Option<Value>,
    pub input_items: Vec<Value>,
    pub output_items: Vec<Value>,
    pub stream_events: Vec<Value>,
    pub cancel_requested: bool,
}

impl super::Store {
    #[allow(clippy::too_many_arguments)]
    pub async fn create_response(
        &self,
        client_key_id: i64,
        id: &str,
        model: &str,
        background: bool,
        conversation_id: Option<&str>,
        previous_response_id: Option<&str>,
        request_json: &Value,
        input_items: &[Value],
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO api_responses (id,client_key_id,model,status,background,conversation_id,previous_response_id,request_json,input_items_json,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?)")
            .bind(id).bind(client_key_id).bind(model).bind("queued").bind(background).bind(conversation_id).bind(previous_response_id)
            .bind(request_json.to_string()).bind(serde_json::to_string(input_items).map_err(|e| e.to_string())?).bind(&now).bind(&now)
            .execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn update_response(
        &self,
        client_key_id: i64,
        id: &str,
        status: &str,
        response_json: &Value,
        output_items: &[Value],
    ) -> Result<(), String> {
        sqlx::query("UPDATE api_responses SET status=?,response_json=?,output_items_json=?,updated_at=? WHERE id=? AND client_key_id=?")
            .bind(status).bind(response_json.to_string()).bind(serde_json::to_string(output_items).map_err(|e| e.to_string())?).bind(chrono::Utc::now().to_rfc3339()).bind(id).bind(client_key_id)
            .execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn response(
        &self,
        client_key_id: i64,
        id: &str,
    ) -> Result<Option<StoredResponse>, String> {
        let row = sqlx::query("SELECT * FROM api_responses WHERE id=? AND client_key_id=?")
            .bind(id)
            .bind(client_key_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        row.map(|row| {
            let parse = |key: &str| {
                serde_json::from_str::<Value>(row.get::<&str, _>(key)).map_err(|e| e.to_string())
            };
            let parse_array = |key: &str| {
                serde_json::from_str::<Vec<Value>>(row.get::<&str, _>(key))
                    .map_err(|e| e.to_string())
            };
            Ok(StoredResponse {
                id: row.get("id"),
                model: row.get("model"),
                status: row.get("status"),
                background: row.get::<i64, _>("background") != 0,
                conversation_id: row.get("conversation_id"),
                previous_response_id: row.get("previous_response_id"),
                request_json: parse("request_json")?,
                response_json: row
                    .get::<Option<&str>, _>("response_json")
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|e| e.to_string())?,
                input_items: parse_array("input_items_json")?,
                output_items: parse_array("output_items_json")?,
                stream_events: parse_array("stream_events_json")?,
                cancel_requested: row.get::<i64, _>("cancel_requested") != 0,
            })
        })
        .transpose()
    }
    pub async fn update_response_json(
        &self,
        client_key_id: i64,
        id: &str,
        status: &str,
        response_json: &Value,
    ) -> Result<(), String> {
        sqlx::query("UPDATE api_responses SET status=?,response_json=?,updated_at=? WHERE id=? AND client_key_id=?").bind(status).bind(response_json.to_string()).bind(chrono::Utc::now().to_rfc3339()).bind(id).bind(client_key_id).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn response_status(
        &self,
        client_key_id: i64,
        id: &str,
        status: &str,
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE api_responses SET status=?,response_json=CASE WHEN response_json IS NULL THEN NULL ELSE json_set(response_json,'$.status',?) END,updated_at=? WHERE id=? AND client_key_id=?",
        )
        .bind(status)
        .bind(status)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id)
        .bind(client_key_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn request_response_cancel(
        &self,
        client_key_id: i64,
        id: &str,
    ) -> Result<bool, String> {
        let result = sqlx::query("UPDATE api_responses SET cancel_requested=1,status='cancelled',response_json=CASE WHEN response_json IS NULL THEN NULL ELSE json_set(response_json,'$.status','cancelled') END,updated_at=? WHERE id=? AND client_key_id=? AND background=1 AND status IN ('queued','in_progress')").bind(chrono::Utc::now().to_rfc3339()).bind(id).bind(client_key_id).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(result.rows_affected() > 0)
    }
    pub async fn append_response_events(
        &self,
        client_key_id: i64,
        id: &str,
        events: &[Value],
    ) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }
        let Some(existing) = self.response(client_key_id, id).await? else {
            return Ok(());
        };
        let mut all = existing.stream_events;
        all.extend_from_slice(events);
        sqlx::query("UPDATE api_responses SET stream_events_json=?,updated_at=? WHERE id=? AND client_key_id=?").bind(serde_json::to_string(&all).map_err(|e| e.to_string())?).bind(chrono::Utc::now().to_rfc3339()).bind(id).bind(client_key_id).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn create_conversation(
        &self,
        client_key_id: i64,
        metadata: &Value,
        items: &[Value],
    ) -> Result<Value, String> {
        let id = format!("conv_{}", uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now().to_rfc3339();
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO conversations (id,client_key_id,metadata_json,created_at,updated_at) VALUES (?,?,?,?,?)").bind(&id).bind(client_key_id).bind(metadata.to_string()).bind(&now).bind(&now).execute(&mut *transaction).await.map_err(|e| e.to_string())?;
        for (position, item) in items.iter().enumerate() {
            sqlx::query("INSERT INTO conversation_items (conversation_id,position,item_id,item_json) VALUES (?,?,?,?)").bind(&id).bind(position as i64).bind(item.get("id").and_then(Value::as_str)).bind(item.to_string()).execute(&mut *transaction).await.map_err(|e| e.to_string())?;
        }
        transaction.commit().await.map_err(|e| e.to_string())?;
        Ok(json!({"id":id,"object":"conversation","created_at":0,"metadata":metadata}))
    }
    pub async fn conversation(
        &self,
        client_key_id: i64,
        id: &str,
    ) -> Result<Option<Value>, String> {
        let row =
            sqlx::query("SELECT metadata_json FROM conversations WHERE id=? AND client_key_id=?")
                .bind(id)
                .bind(client_key_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        row.map(|row| Ok(json!({"id":id,"object":"conversation","created_at":0,"metadata":serde_json::from_str::<Value>(row.get("metadata_json")).map_err(|e| e.to_string())?}))).transpose()
    }
    pub async fn update_conversation(
        &self,
        client_key_id: i64,
        id: &str,
        metadata: &Value,
    ) -> Result<bool, String> {
        let result = sqlx::query(
            "UPDATE conversations SET metadata_json=?,updated_at=? WHERE id=? AND client_key_id=?",
        )
        .bind(metadata.to_string())
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id)
        .bind(client_key_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.rows_affected() > 0)
    }
    pub async fn delete_conversation(&self, client_key_id: i64, id: &str) -> Result<bool, String> {
        let result = sqlx::query("DELETE FROM conversations WHERE id=? AND client_key_id=?")
            .bind(id)
            .bind(client_key_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(result.rows_affected() > 0)
    }
    pub async fn append_conversation_items(
        &self,
        client_key_id: i64,
        id: &str,
        items: &[Value],
    ) -> Result<(), String> {
        if self.conversation(client_key_id, id).await?.is_none() {
            return Err("conversation not found".into());
        }
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        let next: i64 = sqlx::query(
            "SELECT COALESCE(MAX(position)+1,0) FROM conversation_items WHERE conversation_id=?",
        )
        .bind(id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?
        .get(0);
        for (offset, item) in items.iter().enumerate() {
            sqlx::query("INSERT INTO conversation_items (conversation_id,position,item_id,item_json) VALUES (?,?,?,?)").bind(id).bind(next + offset as i64).bind(item.get("id").and_then(Value::as_str)).bind(item.to_string()).execute(&mut *transaction).await.map_err(|e| e.to_string())?;
        }
        sqlx::query("UPDATE conversations SET updated_at=? WHERE id=?")
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        transaction.commit().await.map_err(|e| e.to_string())
    }
    pub async fn conversation_items(
        &self,
        client_key_id: i64,
        id: &str,
        limit: i64,
        descending: bool,
        after: Option<&str>,
    ) -> Result<Option<Vec<Value>>, String> {
        if self.conversation(client_key_id, id).await?.is_none() {
            return Ok(None);
        }
        let after_position = if let Some(after) = after {
            sqlx::query(
                "SELECT position FROM conversation_items WHERE conversation_id=? AND item_id=?",
            )
            .bind(id)
            .bind(after)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?
            .map(|row| row.get::<i64, _>(0))
        } else {
            None
        };
        let order = if descending { "DESC" } else { "ASC" };
        let comparison = if descending { "<" } else { ">" };
        let query = format!(
            "SELECT item_json FROM conversation_items WHERE conversation_id=? AND (? IS NULL OR position {comparison} ?) ORDER BY position {order} LIMIT ?"
        );
        let rows = sqlx::query(&query)
            .bind(id)
            .bind(after_position)
            .bind(after_position)
            .bind(limit.clamp(1, 100))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                serde_json::from_str(row.get::<&str, _>("item_json")).map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }
    pub async fn delete_conversation_item(
        &self,
        client_key_id: i64,
        id: &str,
        item_id: &str,
    ) -> Result<Option<Value>, String> {
        if self.conversation(client_key_id, id).await?.is_none() {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT item_json FROM conversation_items WHERE conversation_id=? AND item_id=?",
        )
        .bind(id)
        .bind(item_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            return Ok(None);
        };
        let value =
            serde_json::from_str(row.get::<&str, _>("item_json")).map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM conversation_items WHERE conversation_id=? AND item_id=?")
            .bind(id)
            .bind(item_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(value))
    }
}
