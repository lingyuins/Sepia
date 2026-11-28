use crate::Store;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

pub const DEFAULT_SOURCE: &str = "https://models.dev/api.json";
pub const DEFAULT_LITELLM_SOURCE: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
pub const DEFAULT_OPENROUTER_SOURCE: &str = "https://openrouter.ai/api/v1/models";

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
    pub cache_read_price_nanos: Option<i64>,
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

/// Parse a JSON numeric price (USD per token or per million tokens) into
/// nanos per token without float rounding drift.
fn price_nanos_number(value: Option<&Value>, per_million: bool) -> Option<i64> {
    let number = value?.as_number()?;
    let text = number.to_string();
    if !per_million {
        return price_nanos(Some(&text));
    }
    // Per-million prices divide by 1e6: shift the decimal exponent by -6
    // instead of multiplying floats. A mantissa already in exponent form
    // adds the exponents.
    let shifted = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => {
            let exponent: i32 = exponent.parse().ok()?;
            format!("{mantissa}e{}", exponent - 6)
        }
        None => format!("{text}e-6"),
    };
    price_nanos(Some(&shifted))
}

fn bool_flag(entry: &Value, key: &str) -> bool {
    entry.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Map one models.dev catalog entry (`{provider: {models: {id: entry}}}`).
/// `cost` is USD per million tokens; deprecated entries are skipped.
pub fn parse_models_dev_entry(provider_id: &str, entry: &Value) -> Option<(String, ModelMeta)> {
    if entry.get("status").and_then(Value::as_str) == Some("deprecated") {
        return None;
    }
    let id = entry.get("id")?.as_str()?;
    let input = string_list(entry.pointer("/modalities/input"));
    let output = string_list(entry.pointer("/modalities/output"));
    let mut capabilities = Vec::new();
    if bool_flag(entry, "tool_call") {
        capabilities.push("tools".to_string());
    }
    if bool_flag(entry, "reasoning") {
        capabilities.push("reasoning".to_string());
    }
    if bool_flag(entry, "structured_output") {
        capabilities.push("structured".to_string());
    }
    if input.iter().any(|item| item == "image") {
        capabilities.push("vision".to_string());
    }
    if input.iter().any(|item| item == "audio") {
        capabilities.push("audio".to_string());
    }
    if input.iter().any(|item| item == "video") {
        capabilities.push("video".to_string());
    }
    if output.iter().any(|item| item == "image") {
        capabilities.push("image-output".to_string());
    }
    Some((
        bare_id(id),
        ModelMeta {
            source_id: format!("{provider_id}/{id}"),
            base_model: base_model(id),
            display_name: entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string(),
            description: entry
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            context_length: entry
                .pointer("/limit/context")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            input_modalities: input,
            output_modalities: output,
            capabilities,
            input_price_nanos: price_nanos_number(entry.pointer("/cost/input"), true),
            output_price_nanos: price_nanos_number(entry.pointer("/cost/output"), true),
            cache_read_price_nanos: price_nanos_number(entry.pointer("/cost/cache_read"), true),
        },
    ))
}

/// Parse the whole models.dev catalog object into deduplicated entries keyed
/// by bare model id. Direct providers win over router-style entries whose id
/// carries a `vendor/` path; among equals the newest `last_updated` wins.
pub fn parse_models_dev(body: &Value) -> Vec<(String, ModelMeta)> {
    let Some(providers) = body.as_object() else {
        return Vec::new();
    };
    let mut best: std::collections::HashMap<String, (bool, String, ModelMeta)> =
        std::collections::HashMap::new();
    for (provider_id, provider) in providers {
        let Some(models) = provider.get("models").and_then(Value::as_object) else {
            continue;
        };
        for entry in models.values() {
            let Some((bare, meta)) = parse_models_dev_entry(provider_id, entry) else {
                continue;
            };
            // A model id that itself contains a slash is a router-style
            // passthrough (e.g. openrouter listing anthropic/claude); direct
            // provider listings win over those.
            let id = &meta.source_id[provider_id.len() + 1..];
            let direct = !id.contains('/');
            let updated = entry
                .get("last_updated")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            match best.get(&bare) {
                Some((was_direct, was_updated, _))
                    if (*was_direct && !direct)
                        || (*was_direct == direct && was_updated.as_str() >= updated.as_str()) => {}
                _ => {
                    best.insert(bare, (direct, updated, meta));
                }
            }
        }
    }
    let mut entries: Vec<(String, ModelMeta)> = best
        .into_iter()
        .map(|(bare, (_, _, meta))| (bare, meta))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// Map one LiteLLM price-table entry. Costs are USD per token floats;
/// `max_input_tokens` (fallback `max_tokens`) is the context window. Entries
/// with neither a price nor a context are skipped; a single-sided price
/// (embedding/rerank rows often carry only input cost) is kept.
pub fn parse_litellm_entry(key: &str, entry: &Value) -> Option<(String, ModelMeta)> {
    if key == "sample_spec" || !entry.is_object() {
        return None;
    }
    let input = price_nanos_number(entry.get("input_cost_per_token"), false);
    let output = price_nanos_number(entry.get("output_cost_per_token"), false);
    let context_length = entry
        .get("max_input_tokens")
        .or_else(|| entry.get("max_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if input.is_none() && output.is_none() && context_length == 0 {
        return None;
    }
    let mut capabilities = Vec::new();
    if bool_flag(entry, "supports_function_calling") || bool_flag(entry, "supports_tool_choice") {
        capabilities.push("tools".to_string());
    }
    if bool_flag(entry, "supports_reasoning") {
        capabilities.push("reasoning".to_string());
    }
    if bool_flag(entry, "supports_response_schema") {
        capabilities.push("structured".to_string());
    }
    if bool_flag(entry, "supports_vision") {
        capabilities.push("vision".to_string());
    }
    if bool_flag(entry, "supports_audio_input") {
        capabilities.push("audio".to_string());
    }
    let mut input_modalities = vec!["text".to_string()];
    if bool_flag(entry, "supports_vision") {
        input_modalities.push("image".to_string());
    }
    if bool_flag(entry, "supports_pdf_input") {
        input_modalities.push("pdf".to_string());
    }
    if bool_flag(entry, "supports_audio_input") {
        input_modalities.push("audio".to_string());
    }
    Some((
        bare_id(key),
        ModelMeta {
            source_id: key.to_string(),
            base_model: base_model(key),
            display_name: String::new(),
            description: String::new(),
            context_length,
            input_modalities,
            output_modalities: vec!["text".to_string()],
            capabilities,
            input_price_nanos: input,
            output_price_nanos: output,
            cache_read_price_nanos: price_nanos_number(
                entry.get("cache_read_input_token_cost"),
                false,
            ),
        },
    ))
}

/// Parse the LiteLLM price table into entries keyed by bare model id.
/// Provider-prefixed duplicates collapse: unprefixed keys carry the vendor's
/// official pricing, so they win over `azure/`-style prefixed entries; among
/// equals the lexicographically first key wins, keeping syncs deterministic.
pub fn parse_litellm(body: &Value) -> Vec<(String, ModelMeta)> {
    let Some(table) = body.as_object() else {
        return Vec::new();
    };
    let mut best: std::collections::BTreeMap<String, (bool, String, ModelMeta)> =
        std::collections::BTreeMap::new();
    for (key, entry) in table {
        let Some((bare, meta)) = parse_litellm_entry(key, entry) else {
            continue;
        };
        let direct = !key.contains('/');
        match best.get(&bare) {
            Some((was_direct, first_key, _))
                if (*was_direct && !direct)
                    || (*was_direct == direct && first_key.as_str() <= key.as_str()) => {}
            _ => {
                best.insert(bare, (direct, key.clone(), meta));
            }
        }
    }
    best.into_iter()
        .map(|(bare, (_, _, meta))| (bare, meta))
        .collect()
}

/// Merge OpenRouter-style entries over a base catalog (models.dev). The base
/// keeps every field it already carries; the overlay only fills gaps, except
/// prices and context where the overlay's live values win when present.
/// Overlay-only models are appended. A wholesale replace would erase the
/// base's richer descriptions/modalities/capabilities whenever the overlay
/// entry is sparse (OpenRouter free variants often carry `"0"` prices).
pub fn merge_entries(
    base: Vec<(String, ModelMeta)>,
    overlay: Vec<(String, ModelMeta)>,
) -> Vec<(String, ModelMeta)> {
    let mut merged: std::collections::BTreeMap<String, ModelMeta> = base.into_iter().collect();
    for (bare, over) in overlay {
        match merged.get_mut(&bare) {
            Some(meta) => fill_gaps(meta, &over),
            None => {
                merged.insert(bare, over);
            }
        }
    }
    merged.into_iter().collect()
}

/// Copy every field the overlay supplies onto a base entry whose field is
/// empty/missing. Prices and context are the exception: a present overlay
/// value wins because it is fresher live data.
fn fill_gaps(meta: &mut ModelMeta, over: &ModelMeta) {
    if meta.display_name.is_empty() && !over.display_name.is_empty() {
        meta.display_name = over.display_name.clone();
    }
    if meta.description.is_empty() && !over.description.is_empty() {
        meta.description = over.description.clone();
    }
    if meta.input_modalities.is_empty() && !over.input_modalities.is_empty() {
        meta.input_modalities = over.input_modalities.clone();
    }
    if meta.output_modalities.is_empty() && !over.output_modalities.is_empty() {
        meta.output_modalities = over.output_modalities.clone();
    }
    if meta.capabilities.is_empty() && !over.capabilities.is_empty() {
        meta.capabilities = over.capabilities.clone();
    }
    if over.input_price_nanos.is_some() {
        meta.input_price_nanos = over.input_price_nanos;
    }
    if over.output_price_nanos.is_some() {
        meta.output_price_nanos = over.output_price_nanos;
    }
    if over.cache_read_price_nanos.is_some() {
        meta.cache_read_price_nanos = over.cache_read_price_nanos;
    }
    if over.context_length > 0 {
        meta.context_length = over.context_length;
    }
}

/// Fill missing prices/contexts on merged entries from the LiteLLM price
/// table. LiteLLM never overwrites a price another source already supplied;
/// it only calibrates gaps (and zero contexts). `:free` variants keep the
/// context calibration but never take prices: `base_model` strips the
/// suffix, so the lookup would pull the paid base model's prices onto a
/// genuinely free variant.
pub fn calibrate_prices(entries: &mut [(String, ModelMeta)], litellm: &[(String, ModelMeta)]) {
    for (_, meta) in entries.iter_mut() {
        let Some(reference) = match_meta(litellm, &meta.source_id) else {
            continue;
        };
        if meta.context_length == 0 {
            meta.context_length = reference.context_length;
        }
        if bare_id(&meta.source_id).ends_with(":free") {
            continue;
        }
        if meta.input_price_nanos.is_none() {
            meta.input_price_nanos = reference.input_price_nanos;
        }
        if meta.output_price_nanos.is_none() {
            meta.output_price_nanos = reference.output_price_nanos;
        }
        if meta.cache_read_price_nanos.is_none() {
            meta.cache_read_price_nanos = reference.cache_read_price_nanos;
        }
    }
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
            cache_read_price_nanos: price_nanos(
                entry
                    .pointer("/pricing/input_cache_read")
                    .and_then(Value::as_str),
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

/// Price lookup for channel auto-fill: `:free` variants only match an exact
/// bare id — the base-model fallback would bill a free variant at the paid
/// base model's prices. Display metadata still uses `match_meta`.
fn match_price_meta<'a>(entries: &'a [(String, ModelMeta)], local: &str) -> Option<&'a ModelMeta> {
    let bare = bare_id(local);
    if bare.ends_with(":free") {
        return entries
            .iter()
            .find(|(id, _)| *id == bare)
            .map(|(_, meta)| meta);
    }
    match_meta(entries, local)
}

impl Store {
    /// Replaces the reference catalog and fills unset channel prices from
    /// matched reference prices in the same transaction. Configured prices
    /// (both columns > 0) are never overwritten. Returns the filled row count.
    pub async fn replace_model_metadata(
        &self,
        source: &str,
        entries: &[(String, ModelMeta)],
    ) -> Result<usize, String> {
        let mut transaction = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM model_metadata")
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
        for (model, meta) in entries {
            sqlx::query(
                "INSERT INTO model_metadata (model, source, source_id, base_model, display_name, description, context_length, input_modalities, output_modalities, capabilities, input_price_nanos, output_price_nanos, cache_read_price_nanos, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
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
            .bind(meta.cache_read_price_nanos)
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
        let channels: Vec<(i64, String)> =
            sqlx::query_as("SELECT provider_id, model FROM provider_models")
                .fetch_all(&mut *transaction)
                .await
                .map_err(|e| e.to_string())?;
        let mut filled = 0usize;
        for (provider_id, model) in channels {
            let Some(meta) = match_price_meta(entries, &model) else {
                continue;
            };
            // nanos/token → micros/token is ×1000 (1 micro = 1000 nanos).
            // Input/output fill only fully-unset channels (both 0) and need
            // both reference prices; configured values are never overwritten.
            if let (Some(input_nanos), Some(output_nanos)) =
                (meta.input_price_nanos, meta.output_price_nanos)
            {
                let input_micros = input_nanos * 1000;
                let output_micros = output_nanos * 1000;
                if input_micros > 0 && output_micros > 0 {
                    let updated = sqlx::query(
                        "UPDATE provider_models SET input_price_micros = ?, output_price_micros = ? WHERE provider_id = ? AND model = ? AND input_price_micros = 0 AND output_price_micros = 0",
                    )
                    .bind(input_micros)
                    .bind(output_micros)
                    .bind(provider_id)
                    .bind(&model)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|e| e.to_string())?;
                    filled += updated.rows_affected() as usize;
                }
            }
            // Cache-read reference price fills only unset channels (0),
            // independent of the input/output gate above.
            if let Some(cache_nanos) = meta.cache_read_price_nanos {
                let cache_micros = cache_nanos * 1000;
                if cache_micros > 0 {
                    sqlx::query(
                        "UPDATE provider_models SET cache_read_price_micros = ? WHERE provider_id = ? AND model = ? AND cache_read_price_micros = 0",
                    )
                    .bind(cache_micros)
                    .bind(provider_id)
                    .bind(&model)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        transaction.commit().await.map_err(|e| e.to_string())?;
        Ok(filled)
    }
    pub async fn model_metadata(&self) -> Result<Vec<(String, ModelMeta)>, String> {
        let rows = sqlx::query("SELECT model, source_id, base_model, display_name, description, context_length, input_modalities, output_modalities, capabilities, input_price_nanos, output_price_nanos, cache_read_price_nanos FROM model_metadata ORDER BY model")
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
                        cache_read_price_nanos: row.get("cache_read_price_nanos"),
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

    #[test]
    fn models_dev_maps_costs_capabilities_and_skips_deprecated() {
        let entry = json!({
            "id": "gpt-x",
            "name": "GPT-X",
            "description": "flagship",
            "reasoning": true,
            "tool_call": true,
            "structured_output": true,
            "last_updated": "2026-01-01",
            "modalities": {"input": ["text", "image"], "output": ["text"]},
            "limit": {"context": 128000, "output": 16384},
            "cost": {"input": 2.5, "output": 10, "cache_read": 0.25}
        });
        let (bare, meta) = parse_models_dev_entry("openai", &entry).unwrap();
        assert_eq!(bare, "gpt-x");
        assert_eq!(meta.source_id, "openai/gpt-x");
        assert_eq!(meta.display_name, "GPT-X");
        assert_eq!(meta.context_length, 128000);
        assert_eq!(meta.input_price_nanos, Some(2_500));
        assert_eq!(meta.output_price_nanos, Some(10_000));
        assert_eq!(meta.cache_read_price_nanos, Some(250));
        assert_eq!(
            meta.capabilities,
            ["tools", "reasoning", "structured", "vision"]
        );
        let deprecated = json!({"id": "old", "status": "deprecated"});
        assert!(parse_models_dev_entry("openai", &deprecated).is_none());
        // Sub-nano per-million prices drop instead of rounding up.
        let tiny = json!({"id": "tiny", "cost": {"input": 0.0000001, "output": 1}});
        let (_, meta) = parse_models_dev_entry("p", &tiny).unwrap();
        assert_eq!(meta.input_price_nanos, None);
        assert_eq!(meta.output_price_nanos, Some(1_000));
    }

    #[test]
    fn models_dev_dedupes_preferring_direct_and_newest() {
        let body = json!({
            "openrouter": {"models": {
                "vendor/gpt-x": {"id": "vendor/gpt-x", "last_updated": "2026-05-01",
                    "cost": {"input": 9, "output": 9}}
            }},
            "openai": {"models": {
                "gpt-x": {"id": "gpt-x", "name": "GPT-X direct", "last_updated": "2026-01-01",
                    "cost": {"input": 2.5, "output": 10}}
            }},
            "azure": {"models": {
                "gpt-x": {"id": "gpt-x", "name": "GPT-X azure", "last_updated": "2026-03-01",
                    "cost": {"input": 2.5, "output": 10}}
            }}
        });
        let entries = parse_models_dev(&body);
        assert_eq!(entries.len(), 1);
        // Direct listing beats the router passthrough despite being older;
        // among direct listings the newest last_updated wins.
        assert_eq!(entries[0].1.source_id, "azure/gpt-x");
    }

    #[test]
    fn litellm_parses_costs_and_prefers_unprefixed_keys() {
        let body = json!({
            "sample_spec": {"input_cost_per_token": 1},
            "gpt-x": {"input_cost_per_token": 2.5e-6, "output_cost_per_token": 1e-5,
                "cache_read_input_token_cost": 2.5e-7,
                "max_input_tokens": 128000, "supports_function_calling": true,
                "supports_vision": true, "mode": "chat"},
            "azure/gpt-x": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-5},
            "bedrock/gpt-x": {"input_cost_per_token": 8e-6, "output_cost_per_token": 8e-5},
            "no-prices": {"mode": "chat"}
        });
        let entries = parse_litellm(&body);
        assert_eq!(entries.len(), 1);
        let (bare, meta) = &entries[0];
        assert_eq!(bare, "gpt-x");
        // The unprefixed key is the vendor's official price; prefixed
        // duplicates only serve as fallback when no direct key exists.
        assert_eq!(meta.source_id, "gpt-x");
        assert_eq!(meta.input_price_nanos, Some(2_500));
        assert_eq!(meta.output_price_nanos, Some(10_000));
        assert_eq!(meta.cache_read_price_nanos, Some(250));
        assert_eq!(meta.context_length, 128000);
        assert_eq!(meta.capabilities, ["tools", "vision"]);
        assert_eq!(meta.input_modalities, ["text", "image"]);
        let prefixed_only = parse_litellm(&json!({
            "azure/gpt-x": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-5},
            "bedrock/gpt-x": {"input_cost_per_token": 8e-6, "output_cost_per_token": 8e-5}
        }));
        // Among prefixed keys the lexicographically first wins deterministically.
        assert_eq!(prefixed_only[0].1.source_id, "azure/gpt-x");
    }

    #[test]
    fn litellm_keeps_single_sided_prices_and_context_only_rows() {
        let body = json!({
            "text-embed-x": {"input_cost_per_token": 1e-7, "mode": "embedding"},
            "context-only": {"max_input_tokens": 32000, "mode": "chat"},
            "empty-row": {"mode": "chat"}
        });
        let entries = parse_litellm(&body);
        assert_eq!(entries.len(), 2);
        let embed = entries.iter().find(|(id, _)| id == "text-embed-x").unwrap();
        assert_eq!(embed.1.input_price_nanos, Some(100));
        assert_eq!(embed.1.output_price_nanos, None);
        let ctx = entries.iter().find(|(id, _)| id == "context-only").unwrap();
        assert_eq!(ctx.1.context_length, 32000);
        assert_eq!(ctx.1.input_price_nanos, None);
    }

    #[test]
    fn merge_and_calibrate_fill_gaps_without_erasing_base_fields() {
        let base = parse_models_dev(&json!({
            "openai": {"models": {
                "gpt-x": {"id": "gpt-x", "name": "GPT-X", "description": "flagship",
                    "reasoning": true, "limit": {"context": 128000},
                    "cost": {"input": 2.5, "output": 10}},
                "doubao-x": {"id": "doubao-x", "name": "Doubao X"}
            }}
        }));
        // Sparse overlay: live prices/context but no description/capabilities.
        let overlay = vec![
            parse_entry(&json!({"id": "openai/gpt-x", "name": "OpenAI: GPT-X live",
                "context_length": 200000,
                "pricing": {"prompt": "0.000003", "completion": "0.000012", "input_cache_read": "0.0000003"}}))
            .unwrap(),
        ];
        let mut merged = merge_entries(base, overlay);
        assert_eq!(merged.len(), 2);
        let litellm = parse_litellm(&json!({
            "doubao-x": {"input_cost_per_token": 4e-7, "output_cost_per_token": 8e-7,
                "max_input_tokens": 256000},
            "gpt-x": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-5}
        }));
        calibrate_prices(&mut merged, &litellm);
        let doubao = merged.iter().find(|(id, _)| id == "doubao-x").unwrap();
        assert_eq!(doubao.1.input_price_nanos, Some(400));
        assert_eq!(doubao.1.output_price_nanos, Some(800));
        // LiteLLM calibrates a missing cache-read price too.
        let litellm2 = parse_litellm(&json!({
            "doubao-x": {"input_cost_per_token": 4e-7, "output_cost_per_token": 8e-7,
                "cache_read_input_token_cost": 4e-8}
        }));
        let mut gap = parse_models_dev(&json!({
            "volc": {"models": {"doubao-x": {"id": "doubao-x"}}}
        }));
        calibrate_prices(&mut gap, &litellm2);
        assert_eq!(gap[0].1.cache_read_price_nanos, Some(40));
        assert_eq!(doubao.1.context_length, 256000);
        let gpt = merged.iter().find(|(id, _)| id == "gpt-x").unwrap();
        // Overlay prices/context win, but the base's description and
        // capabilities survive a sparse overlay; LiteLLM never overwrites.
        assert_eq!(gpt.1.display_name, "GPT-X");
        assert_eq!(gpt.1.description, "flagship");
        assert_eq!(gpt.1.capabilities, ["reasoning"]);
        assert_eq!(gpt.1.input_price_nanos, Some(3_000));
        assert_eq!(gpt.1.context_length, 200000);
        assert_eq!(gpt.1.cache_read_price_nanos, Some(300));
        // A zero-priced overlay on the same bare id must not erase base prices.
        let base2 = parse_models_dev(&json!({
            "openai": {"models": {
                "gpt-y": {"id": "gpt-y", "cost": {"input": 2.5, "output": 10}}
            }}
        }));
        let zero_overlay = vec![
            parse_entry(&json!({"id": "openai/gpt-y",
                "pricing": {"prompt": "0", "completion": "0"}}))
            .unwrap(),
        ];
        let merged2 = merge_entries(base2, zero_overlay);
        assert_eq!(merged2.len(), 1);
        assert_eq!(merged2[0].1.input_price_nanos, Some(2_500));
        assert_eq!(merged2[0].1.output_price_nanos, Some(10_000));
    }

    #[test]
    fn litellm_calibration_never_prices_free_variants() {
        let mut entries = vec![
            parse_entry(&json!({"id": "vendor/gpt-x:free",
                "pricing": {"prompt": "0", "completion": "0"}}))
            .unwrap(),
            parse_entry(&json!({"id": "vendor/gpt-x",
                "pricing": {"prompt": "0", "completion": "0"}}))
            .unwrap(),
        ];
        let litellm = parse_litellm(&json!({
            "gpt-x": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-5,
                "max_input_tokens": 128000}
        }));
        calibrate_prices(&mut entries, &litellm);
        let free = entries.iter().find(|(id, _)| id == "gpt-x:free").unwrap();
        assert_eq!(free.1.input_price_nanos, None);
        assert_eq!(free.1.output_price_nanos, None);
        // Context still calibrates: free variants share the base context.
        assert_eq!(free.1.context_length, 128_000);
        // The paid base variant still calibrates its gaps.
        let paid = entries.iter().find(|(id, _)| id == "gpt-x").unwrap();
        assert_eq!(paid.1.input_price_nanos, Some(9_000));
        assert_eq!(paid.1.context_length, 128_000);
    }

    #[test]
    fn price_match_for_free_variants_requires_exact_id() {
        let entries = vec![
            parse_entry(&json!({"id": "vendor/gpt-x",
                "pricing": {"prompt": "0.000001", "completion": "0.000002"}}))
            .unwrap(),
        ];
        // No exact `gpt-x:free` entry: the paid base must not supply prices.
        assert!(match_price_meta(&entries, "gpt-x:free").is_none());
        assert!(match_price_meta(&entries, "vendor/gpt-x:free").is_none());
        // The paid id keeps the base fallback; an exact free entry matches.
        assert!(match_price_meta(&entries, "gpt-x").is_some());
        let mut entries = entries;
        entries.push(
            parse_entry(&json!({"id": "vendor/gpt-x:free",
                "pricing": {"prompt": "0.000003", "completion": "0.000004"}}))
            .unwrap(),
        );
        assert_eq!(
            match_price_meta(&entries, "gpt-x:free")
                .unwrap()
                .input_price_nanos,
            Some(3_000)
        );
    }
}
