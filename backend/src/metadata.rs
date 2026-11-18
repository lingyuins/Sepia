use crate::Store;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

pub const DEFAULT_SOURCE: &str = "https://openrouter.ai/api/v1/models";

#[derive(Debug, Clone, Serialize)]
pub struct ModelMeta {
    pub source_id: String,
    pub base_model: String,
    pub display_name: String,
    pub description: String,
    pub context_length: i64,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub capabilities: Vec<String>,
    pub input_price_nanos: Option<i64>,
    pub output_price_nanos: Option<i64>,
}

/// Lowercase, drop a leading `~` alias marker and any `vendor/` prefix.
pub fn bare_id(value: &str) -> String {
    let value = value.trim().trim_start_matches('~').to_lowercase();
    match value.rsplit_once('/') {
        Some((_, tail)) => tail.to_string(),
        None => value,
    }
}

/// Strip snapshot/alias suffixes so dated or free variants share one base model.
pub fn base_model(value: &str) -> String {
    let mut base = bare_id(value);
    if let Some(stripped) = base.strip_suffix(":free") {
        base = stripped.to_string();
    }
    for suffix in ["-latest", "-preview", "-experimental", "-exp"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.to_string();
            break;
        }
    }
    if base.len() > 9 && base.as_bytes()[base.len() - 9] == b'-' {
        let tail = &base[base.len() - 8..];
        if tail.bytes().all(|byte| byte.is_ascii_digit()) {
            base.truncate(base.len() - 9);
        }
    }
    if base.len() > 11 {
        let tail = &base[base.len() - 11..];
        let bytes = tail.as_bytes();
        if bytes[0] == b'-'
            && bytes[5] == b'-'
            && bytes[8] == b'-'
            && tail.bytes().enumerate().all(|(index, byte)| {
                index == 0 || index == 5 || index == 8 || byte.is_ascii_digit()
            })
        {
            base.truncate(base.len() - 11);
        }
    }
    base
}

fn price_nanos(value: Option<&str>) -> Option<i64> {
    // Exact decimal scaling to nanos per token (3 decimals per million USD):
    // integer micros would round 0.0000025 USD/token to $3/M instead of $2.5/M.
    let text = value?.trim();
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (text, 0),
    };
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches(['+', '-']);
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{int_part}{frac_part}");
    let magnitude: i128 = digits.parse().ok()?;
    let scale = exponent - frac_part.len() as i32 + 9;
    let scaled = if scale >= 0 {
        magnitude.checked_mul(10i128.checked_pow(scale as u32)?)?
    } else {
        let divisor = 10i128.checked_pow((-scale) as u32)?;
        (magnitude + divisor / 2) / divisor
    };
    if scaled <= 0 {
        return None;
    }
    let nanos = i64::try_from(if negative { -scaled } else { scaled }).ok()?;
    if nanos <= 0 { None } else { Some(nanos) }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn capabilities(entry: &Value) -> Vec<String> {
    let parameters = string_list(entry.get("supported_parameters"));
    let input = string_list(entry.pointer("/architecture/input_modalities"));
    let output = string_list(entry.pointer("/architecture/output_modalities"));
    let mut tags = Vec::new();
    if parameters
        .iter()
        .any(|item| item == "tools" || item == "tool_choice")
    {
        tags.push("tools".to_string());
    }
    if parameters.iter().any(|item| {
        item == "reasoning" || item == "include_reasoning" || item == "reasoning_effort"
    }) {
        tags.push("reasoning".to_string());
    }
    if parameters.iter().any(|item| item == "structured_outputs") {
        tags.push("structured".to_string());
    }
    if input.iter().any(|item| item == "image") {
        tags.push("vision".to_string());
    }
    if input.iter().any(|item| item == "audio") {
        tags.push("audio".to_string());
    }
    if input.iter().any(|item| item == "video") {
        tags.push("video".to_string());
    }
    if output.iter().any(|item| item == "image") {
        tags.push("image-output".to_string());
    }
    tags
}

/// Map one OpenRouter-style catalog entry to stored metadata.
pub fn parse_entry(entry: &Value) -> Option<(String, ModelMeta)> {
    let id = entry.get("id")?.as_str()?;
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_string();
    let display_name = name
        .split_once(": ")
        .map(|(_, tail)| tail.to_string())
        .unwrap_or(name);
    Some((
        bare_id(id),
        ModelMeta {
            source_id: id.to_string(),
            base_model: base_model(id),
            display_name,
            description: entry
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            context_length: entry
                .get("context_length")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            input_modalities: string_list(entry.pointer("/architecture/input_modalities")),
            output_modalities: string_list(entry.pointer("/architecture/output_modalities")),
            capabilities: capabilities(entry),
            input_price_nanos: price_nanos(
                entry.pointer("/pricing/prompt").and_then(Value::as_str),
            ),
            output_price_nanos: price_nanos(
                entry.pointer("/pricing/completion").and_then(Value::as_str),
            ),
        },
    ))
}

/// Pick the metadata for a local model id: exact bare match first, then a
/// variant whose base model equals the local id or its own base form.
pub fn match_meta<'a>(entries: &'a [(String, ModelMeta)], local: &str) -> Option<&'a ModelMeta> {
    let bare = bare_id(local);
    if let Some((_, meta)) = entries.iter().find(|(id, _)| *id == bare) {
        return Some(meta);
    }
    let base = base_model(local);
    entries
        .iter()
        .find(|(_, meta)| meta.base_model == bare || meta.base_model == base)
        .map(|(_, meta)| meta)
}

impl Store {
    pub async fn replace_model_metadata(
        &self,
        source: &str,
        entries: &[(String, ModelMeta)],
    ) -> Result<(), String> {
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM model_metadata")
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        for (model, meta) in entries {
            sqlx::query(
                "INSERT INTO model_metadata (model, source, source_id, base_model, display_name, description, context_length, input_modalities, output_modalities, capabilities, input_price_nanos, output_price_nanos, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
            )
            .bind(model)
            .bind(source)
            .bind(&meta.source_id)
            .bind(&meta.base_model)
            .bind(&meta.display_name)
            .bind(&meta.description)
            .bind(meta.context_length)
            .bind(serde_json::to_string(&meta.input_modalities).map_err(|e| e.to_string())?)
            .bind(serde_json::to_string(&meta.output_modalities).map_err(|e| e.to_string())?)
            .bind(serde_json::to_string(&meta.capabilities).map_err(|e| e.to_string())?)
            .bind(meta.input_price_nanos)
            .bind(meta.output_price_nanos)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        }
        sqlx::query(
            "INSERT INTO catalog_meta (key, value) VALUES ('source', ?), ('fetched_at', datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(source)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
        transaction.commit().await.map_err(|e| e.to_string())
    }
    pub async fn model_metadata(&self) -> Result<Vec<(String, ModelMeta)>, String> {
        let rows = sqlx::query("SELECT model, source_id, base_model, display_name, description, context_length, input_modalities, output_modalities, capabilities, input_price_nanos, output_price_nanos FROM model_metadata ORDER BY model")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                let parse = |column: &str| -> Result<Vec<String>, String> {
                    serde_json::from_str(&row.get::<String, _>(column)).map_err(|e| e.to_string())
                };
                Ok((
                    row.get("model"),
                    ModelMeta {
                        source_id: row.get("source_id"),
                        base_model: row.get("base_model"),
                        display_name: row.get("display_name"),
                        description: row.get("description"),
                        context_length: row.get("context_length"),
                        input_modalities: parse("input_modalities")?,
                        output_modalities: parse("output_modalities")?,
                        capabilities: parse("capabilities")?,
                        input_price_nanos: row.get("input_price_nanos"),
                        output_price_nanos: row.get("output_price_nanos"),
                    },
                ))
            })
            .collect()
    }
    pub async fn catalog_meta(&self) -> Result<(Option<String>, Option<String>), String> {
        let rows = sqlx::query("SELECT key, value FROM catalog_meta")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        let mut source = None;
        let mut fetched_at = None;
        for row in rows {
            match row.get::<String, _>("key").as_str() {
                "source" => source = Some(row.get("value")),
                "fetched_at" => fetched_at = Some(row.get("value")),
                _ => {}
            }
        }
        Ok((source, fetched_at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base_model_strips_variant_suffixes() {
        assert_eq!(base_model("openai/gpt-4o-2024-08-06"), "gpt-4o");
        assert_eq!(base_model("openai/gpt-4o-20241120"), "gpt-4o");
        assert_eq!(base_model("moonshotai/kimi-k3:free"), "kimi-k3");
        assert_eq!(
            base_model("~deepseek/deepseek-v4-flash-latest"),
            "deepseek-v4-flash"
        );
        assert_eq!(
            base_model("google/gemini-2.5-pro-preview"),
            "gemini-2.5-pro"
        );
        assert_eq!(base_model("deepseek/deepseek-v3.2-exp"), "deepseek-v3.2");
        assert_eq!(base_model("Kimi-K3"), "kimi-k3");
        assert_eq!(base_model("gpt-4o"), "gpt-4o");
        assert_eq!(base_model("o3"), "o3");
    }

    #[test]
    fn price_nanos_scales_decimal_strings_exactly() {
        assert_eq!(price_nanos(Some("0.0000025")), Some(2_500));
        assert_eq!(price_nanos(Some("0.00001")), Some(10_000));
        assert_eq!(price_nanos(Some("2.5e-6")), Some(2_500));
        assert_eq!(price_nanos(Some("0.0000000001")), None);
        assert_eq!(price_nanos(Some("0")), None);
        assert_eq!(price_nanos(Some("-1")), None);
        assert_eq!(price_nanos(Some("abc")), None);
        assert_eq!(price_nanos(None), None);
    }

    #[test]
    fn parse_entry_maps_fields_and_prices() {
        let entry = json!({
            "id": "openai/gpt-4o-2024-08-06",
            "name": "OpenAI: GPT-4o (2024-08-06)",
            "description": "omni model",
            "context_length": 128000,
            "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]},
            "pricing": {"prompt": "0.0000025", "completion": "0.00001"},
            "supported_parameters": ["tools", "reasoning", "structured_outputs", "temperature"]
        });
        let (id, meta) = parse_entry(&entry).unwrap();
        assert_eq!(id, "gpt-4o-2024-08-06");
        assert_eq!(meta.base_model, "gpt-4o");
        assert_eq!(meta.display_name, "GPT-4o (2024-08-06)");
        assert_eq!(meta.context_length, 128000);
        assert_eq!(meta.input_price_nanos, Some(2_500));
        assert_eq!(meta.output_price_nanos, Some(10_000));
        assert_eq!(
            meta.capabilities,
            ["tools", "reasoning", "structured", "vision"]
        );
        assert!(parse_entry(&json!({"name": "no id"})).is_none());
        let free =
            parse_entry(&json!({"id": "x/y:free", "pricing": {"prompt": "0", "completion": "0"}}))
                .unwrap();
        assert!(free.1.input_price_nanos.is_none());
    }

    #[test]
    fn match_meta_prefers_exact_then_base() {
        let entries = vec![
            (
                "gpt-4o-2024-08-06".to_string(),
                parse_entry(&json!({"id": "openai/gpt-4o-2024-08-06"}))
                    .unwrap()
                    .1,
            ),
            (
                "gpt-4o".to_string(),
                parse_entry(&json!({"id": "openai/gpt-4o", "name": "OpenAI: GPT-4o"}))
                    .unwrap()
                    .1,
            ),
        ];
        assert_eq!(
            match_meta(&entries, "gpt-4o").unwrap().source_id,
            "openai/gpt-4o"
        );
        assert_eq!(
            match_meta(&entries, "vendor/gpt-4o-2024-11-20")
                .unwrap()
                .base_model,
            "gpt-4o"
        );
        assert!(match_meta(&entries, "unknown-model").is_none());
    }
}
