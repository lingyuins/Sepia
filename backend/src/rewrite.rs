//! Built-in provider/route request rewriting: static outbound headers,
//! top-level parameter regex replacement, system-only prompt regex replacement,
//! and only-if-absent structured parameter defaults. Rules are stored as JSON
//! arrays on providers and routes; route rules override provider rules by key
//! (header name / parameter field / prompt pattern) at relay time. The `model`
//! rewrite only affects the outbound body field and the Gemini URL path — key
//! scopes, request logs, strategy stats and quotas keep the original model.
use crate::protocol::{ChatRequest, ContentBlock, Role};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One static outbound header. Names are matched case-insensitively; sensitive
/// headers (auth/cookie/host/protocol-version/transport) are rejected at save.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderRule {
    pub name: String,
    pub value: String,
}

/// Regex replacement applied to one top-level outbound body field (mainly
/// `model`). Only string-valued fields are rewritten; `$1`/`${name}` captures
/// use the Rust `regex` crate syntax (no lookaround/backreferences).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamReplaceRule {
    pub field: String,
    pub pattern: String,
    pub replacement: String,
}

/// Regex replacement applied only to system/developer prompt text. User,
/// assistant and tool content is never touched.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptReplaceRule {
    pub pattern: String,
    pub replacement: String,
}

/// Structured only-if-absent default for one top-level outbound body field
/// (Octopus `param_override` semantics). The value is any JSON value and is
/// inserted only when the field is missing; an explicit client value wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamDefaultRule {
    pub field: String,
    pub value: Value,
}

/// The four rewrite rule lists owned by a provider or a route.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RewriteConfig {
    #[serde(default)]
    pub headers: Vec<HeaderRule>,
    #[serde(default)]
    pub param_replace: Vec<ParamReplaceRule>,
    #[serde(default)]
    pub prompt_replace: Vec<PromptReplaceRule>,
    #[serde(default)]
    pub param_defaults: Vec<ParamDefaultRule>,
}

impl RewriteConfig {
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
            && self.param_replace.is_empty()
            && self.prompt_replace.is_empty()
            && self.param_defaults.is_empty()
    }

    /// Parse a stored JSON config column; an empty/blank column is an empty config.
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.trim().is_empty() {
            return Ok(Self::default());
        }
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| format!("invalid rewrite config: {error}"))?;
        if !value.is_object() {
            return Err("invalid rewrite config: expected a JSON object".into());
        }
        serde_json::from_value(value).map_err(|error| format!("invalid rewrite config: {error}"))
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|error| error.to_string())
    }
}

/// Headers that configuration (like plugins) may never set: they carry
/// credentials, routing identity or transport framing owned by the gateway.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "host",
    "x-api-key",
    "x-goog-api-key",
    "anthropic-version",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
];

pub fn is_sensitive_header(name: &str) -> bool {
    SENSITIVE_HEADERS
        .iter()
        .any(|item| item.eq_ignore_ascii_case(name))
}

/// Validate a rewrite config before persistence: non-empty names/fields,
/// no sensitive headers, and every regex pattern compiles under the `regex`
/// crate (so lookaround/backreferences are rejected up front, not at relay).
pub fn validate(config: &RewriteConfig) -> Result<(), String> {
    for header in &config.headers {
        let name = header.name.trim();
        if name.is_empty() {
            return Err("header name is required".into());
        }
        if name != header.name {
            return Err("header name must not have leading or trailing spaces".into());
        }
        if http::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
            return Err(format!("header '{name}' is not a valid HTTP header name"));
        }
        if is_sensitive_header(name) {
            return Err(format!("header '{name}' is managed by the gateway"));
        }
        if header.value.contains(['\r', '\n']) {
            return Err(format!("header '{name}' value must be a single line"));
        }
    }
    for rule in &config.param_replace {
        if rule.field.trim().is_empty() {
            return Err("parameter field is required".into());
        }
        compile(&rule.pattern).map_err(|error| format!("parameter '{}': {error}", rule.field))?;
    }
    for rule in &config.prompt_replace {
        compile(&rule.pattern).map_err(|error| format!("prompt pattern: {error}"))?;
    }
    for rule in &config.param_defaults {
        if rule.field.trim().is_empty() {
            return Err("parameter default field is required".into());
        }
    }
    Ok(())
}

fn compile(pattern: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|error| format!("invalid regex: {error}"))
}

/// Merge provider rules under route rules. Later (route) entries win by key:
/// header name (case-insensitive), parameter field, prompt pattern. The result
/// preserves provider order for keys the route does not override, then appends
/// route-only keys, so ordering stays stable and predictable.
pub fn merge(provider: &RewriteConfig, route: &RewriteConfig) -> RewriteConfig {
    let mut headers = provider.headers.clone();
    for rule in &route.headers {
        headers.retain(|existing| !existing.name.eq_ignore_ascii_case(&rule.name));
        headers.push(rule.clone());
    }
    let mut param_replace = provider.param_replace.clone();
    for rule in &route.param_replace {
        param_replace.retain(|existing| existing.field != rule.field);
        param_replace.push(rule.clone());
    }
    let mut prompt_replace = provider.prompt_replace.clone();
    for rule in &route.prompt_replace {
        prompt_replace.retain(|existing| existing.pattern != rule.pattern);
        prompt_replace.push(rule.clone());
    }
    let mut param_defaults = provider.param_defaults.clone();
    for rule in &route.param_defaults {
        param_defaults.retain(|existing| existing.field != rule.field);
        param_defaults.push(rule.clone());
    }
    RewriteConfig {
        headers,
        param_replace,
        prompt_replace,
        param_defaults,
    }
}

/// Apply system-only prompt rewriting to the shared request model. The
/// `system` field and any `Role::System` messages are rewritten; all other
/// roles pass through untouched. Both `Message.text` and its `Text` content
/// blocks stay consistent so every protocol encoder sees the rewritten prompt.
pub fn apply_prompt(request: &mut ChatRequest, rules: &[PromptReplaceRule]) {
    if rules.is_empty() {
        return;
    }
    let compiled: Vec<(Regex, &str)> = rules
        .iter()
        .filter_map(|rule| {
            compile(&rule.pattern)
                .ok()
                .map(|re| (re, rule.replacement.as_str()))
        })
        .collect();
    if compiled.is_empty() {
        return;
    }
    let rewrite = |text: &str| -> String {
        let mut current = text.to_string();
        for (re, replacement) in &compiled {
            current = re.replace_all(&current, *replacement).into_owned();
        }
        current
    };
    if let Some(system) = &request.system {
        request.system = Some(rewrite(system));
    }
    for message in &mut request.messages {
        if message.role != Role::System {
            continue;
        }
        message.text = rewrite(&message.text);
        for block in &mut message.content {
            if let ContentBlock::Text(text) = block {
                *text = rewrite(text);
            }
        }
    }
}

/// Apply top-level parameter regex replacement and only-if-absent defaults to
/// an encoded outbound body. Replacement only touches existing string fields;
/// defaults only fill missing fields. Gemini keeps generation parameters inside
/// `generationConfig`, so those are resolved there too (and lifted to the top
/// level only to detect an explicit client value).
pub fn apply_params(body: &mut Value, config: &RewriteConfig, protocol: crate::protocol::Protocol) {
    let Some(map) = body.as_object_mut() else {
        return;
    };
    for rule in &config.param_replace {
        let Ok(re) = compile(&rule.pattern) else {
            continue;
        };
        if let Some(Value::String(current)) = map.get(&rule.field) {
            let replaced = re
                .replace_all(current, rule.replacement.as_str())
                .into_owned();
            map.insert(rule.field.clone(), Value::String(replaced));
        }
    }
    if protocol == crate::protocol::Protocol::Gemini {
        apply_gemini_defaults(map, &config.param_defaults);
    } else {
        for rule in &config.param_defaults {
            map.entry(rule.field.clone())
                .or_insert_with(|| rule.value.clone());
        }
    }
}

/// Map a shared-model parameter field to its Gemini `generationConfig` key.
fn gemini_param_field(field: &str) -> Option<&'static str> {
    match field {
        "max_tokens" => Some("maxOutputTokens"),
        "temperature" => Some("temperature"),
        "top_p" => Some("topP"),
        "stop" => Some("stopSequences"),
        _ => None,
    }
}

fn apply_gemini_defaults(map: &mut serde_json::Map<String, Value>, defaults: &[ParamDefaultRule]) {
    let mut config = map.remove("generationConfig").unwrap_or_else(|| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    let generation = config.as_object_mut().unwrap();
    for rule in defaults {
        if let Some(key) = gemini_param_field(&rule.field) {
            generation
                .entry(key.to_string())
                .or_insert_with(|| rule.value.clone());
        } else {
            map.entry(rule.field.clone())
                .or_insert_with(|| rule.value.clone());
        }
    }
    map.insert("generationConfig".into(), config);
}

/// The effective upstream model after parameter rewriting: the rewritten
/// `model` body field when present, otherwise the original. The caller uses it
/// for the Gemini URL path while key scopes, logs and strategy stats keep the
/// original model.
pub fn effective_model(body: &Value, original_model: &str) -> String {
    body.get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .unwrap_or(original_model)
        .to_string()
}

/// The effective upstream model for a protocol that carries the model in the
/// request path (Gemini) instead of the body: apply the `model` parameter rules
/// to the original model string. Rules that fail to compile are skipped, which
/// mirrors `apply_params` (validation rejects them at save time anyway).
pub fn effective_model_for(original: &str, config: &RewriteConfig) -> String {
    let mut model = original.to_string();
    for rule in &config.param_replace {
        if rule.field != "model" {
            continue;
        }
        let Ok(re) = compile(&rule.pattern) else {
            continue;
        };
        model = re
            .replace_all(&model, rule.replacement.as_str())
            .into_owned();
    }
    model
}

/// Rewrite a verbatim (non-shared-model) JSON body, used by auxiliary
/// forwarding: parameter regex replacement plus only-if-absent defaults at the
/// top level. Auxiliary endpoints are OpenAI-shaped, so Gemini generationConfig
/// mapping does not apply. Non-object bodies are returned unchanged.
pub fn apply_raw_params(value: Value, config: &RewriteConfig) -> Value {
    let mut body = value;
    if !config.param_replace.is_empty() || !config.param_defaults.is_empty() {
        apply_params(&mut body, config, crate::protocol::Protocol::OpenAI);
    }
    body
}

/// Apply parameter rewriting to the shared request model itself, used for
/// plugin-backed providers: the host cannot inject rules into a plugin's own
/// outbound HTTP call, so it rewrites the shared request BEFORE handing it to
/// `provider.fetch` and every plugin forwards rewritten values unchanged.
/// Typed string fields (currently only `model`) take regex replacement;
/// passthrough string fields likewise; defaults fill typed fields and
/// passthrough only when absent (an explicit client value always wins).
/// Gemini generation parameters live in typed fields here — the encoded
/// `generationConfig` mapping does not apply to the shared model.
pub fn apply_shared_params(request: &mut ChatRequest, config: &RewriteConfig) {
    for rule in &config.param_replace {
        let Ok(re) = compile(&rule.pattern) else {
            continue;
        };
        let target = match rule.field.as_str() {
            "model" => Some(&mut request.model),
            _ => None,
        };
        if let Some(current) = target {
            *current = re
                .replace_all(current, rule.replacement.as_str())
                .into_owned();
        } else if let Some(Value::String(current)) = request.passthrough.get_mut(&rule.field) {
            *current = re
                .replace_all(current, rule.replacement.as_str())
                .into_owned();
        }
    }
    for rule in &config.param_defaults {
        match rule.field.as_str() {
            "model" => {
                if request.model.is_empty()
                    && let Some(value) = rule.value.as_str()
                {
                    request.model = value.to_string();
                }
            }
            "max_tokens" => {
                if request.max_tokens.is_none() {
                    request.max_tokens = rule.value.as_u64().map(|n| n as u32);
                }
            }
            "temperature" => {
                if request.temperature.is_none() {
                    request.temperature = rule.value.as_f64();
                }
            }
            "top_p" => {
                if request.top_p.is_none() {
                    request.top_p = rule.value.as_f64();
                }
            }
            "stop" => {
                if request.stop.is_empty()
                    && let Some(items) = rule.value.as_array()
                {
                    request.stop = items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect();
                }
            }
            _ => {
                request
                    .passthrough
                    .entry(rule.field.clone())
                    .or_insert_with(|| rule.value.clone());
            }
        }
    }
}

/// Outbound static headers as ordered name/value pairs, skipping sensitive
/// names as a defense-in-depth measure (validation already rejects them).
pub fn header_pairs(config: &RewriteConfig) -> Vec<(String, String)> {
    config
        .headers
        .iter()
        .filter(|rule| !is_sensitive_header(&rule.name))
        .map(|rule| (rule.name.clone(), rule.value.clone()))
        .collect()
}

/// Merge two header lists by name (case-insensitive); `overlay` wins.
pub fn merge_headers(
    base: Vec<(String, String)>,
    overlay: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let mut merged = base;
    for (name, value) in overlay {
        merged.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        merged.push((name, value));
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Message;

    fn header(name: &str, value: &str) -> HeaderRule {
        HeaderRule {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn merge_route_overrides_provider_by_key() {
        let provider = RewriteConfig {
            headers: vec![header("User-Agent", "provider"), header("X-Keep", "yes")],
            param_replace: vec![ParamReplaceRule {
                field: "model".into(),
                pattern: "^gpt".into(),
                replacement: "azure".into(),
            }],
            prompt_replace: vec![PromptReplaceRule {
                pattern: "foo".into(),
                replacement: "bar".into(),
            }],
            param_defaults: vec![ParamDefaultRule {
                field: "max_tokens".into(),
                value: json!(1024),
            }],
        };
        let route = RewriteConfig {
            headers: vec![header("user-agent", "route")],
            param_replace: vec![ParamReplaceRule {
                field: "model".into(),
                pattern: "^claude".into(),
                replacement: "anthropic".into(),
            }],
            prompt_replace: vec![PromptReplaceRule {
                pattern: "foo".into(),
                replacement: "baz".into(),
            }],
            param_defaults: vec![ParamDefaultRule {
                field: "max_tokens".into(),
                value: json!(2048),
            }],
        };
        let merged = merge(&provider, &route);
        assert_eq!(merged.headers.len(), 2);
        assert_eq!(merged.headers[0].name, "X-Keep");
        assert_eq!(merged.headers[1].name, "user-agent");
        assert_eq!(merged.headers[1].value, "route");
        assert_eq!(merged.param_replace.len(), 1);
        assert_eq!(merged.param_replace[0].pattern, "^claude");
        assert_eq!(merged.prompt_replace[0].replacement, "baz");
        assert_eq!(merged.param_defaults[0].value, json!(2048));
    }

    #[test]
    fn validate_rejects_sensitive_headers_and_bad_regex() {
        let mut config = RewriteConfig {
            headers: vec![header("Authorization", "Bearer x")],
            ..Default::default()
        };
        assert!(validate(&config).is_err());
        config.headers = vec![header("User-Agent", "ok")];
        assert!(validate(&config).is_ok());
        config.param_replace = vec![ParamReplaceRule {
            field: "model".into(),
            pattern: "(?<=a)b".into(),
            replacement: "x".into(),
        }];
        assert!(validate(&config).is_err());
        config.param_replace = vec![ParamReplaceRule {
            field: "model".into(),
            pattern: "^gpt-(.*)$".into(),
            replacement: "azure-$1".into(),
        }];
        assert!(validate(&config).is_ok());
    }

    #[test]
    fn params_replace_model_and_fill_defaults_only_if_absent() {
        let config = RewriteConfig {
            param_replace: vec![ParamReplaceRule {
                field: "model".into(),
                pattern: "^gpt-(.*)$".into(),
                replacement: "azure-$1".into(),
            }],
            param_defaults: vec![
                ParamDefaultRule {
                    field: "max_tokens".into(),
                    value: json!(8192),
                },
                ParamDefaultRule {
                    field: "temperature".into(),
                    value: json!(0.5),
                },
            ],
            ..Default::default()
        };
        let mut body = json!({"model":"gpt-4o","temperature":0.9});
        apply_params(&mut body, &config, crate::protocol::Protocol::OpenAI);
        assert_eq!(effective_model(&body, "gpt-4o"), "azure-4o");
        assert_eq!(body["model"], "azure-4o");
        assert_eq!(body["max_tokens"], 8192);
        assert_eq!(body["temperature"], 0.9);
        let mut gemini = json!({"generationConfig":{"temperature":0.9}});
        apply_params(&mut gemini, &config, crate::protocol::Protocol::Gemini);
        assert_eq!(gemini["generationConfig"]["maxOutputTokens"], 8192);
        assert_eq!(gemini["generationConfig"]["temperature"], 0.9);
    }

    #[test]
    fn prompt_rewrite_touches_only_system() {
        let rules = vec![PromptReplaceRule {
            pattern: "secret".into(),
            replacement: "public".into(),
        }];
        let mut request = ChatRequest {
            system: Some("secret system".into()),
            messages: vec![
                Message::new(Role::System, "secret dev"),
                Message::new(Role::User, "secret user"),
                Message::new(Role::Assistant, "secret assistant"),
            ],
            ..Default::default()
        };
        apply_prompt(&mut request, &rules);
        assert_eq!(request.system.as_deref(), Some("public system"));
        assert_eq!(request.messages[0].text, "public dev");
        assert_eq!(request.messages[0].content[0].as_text(), Some("public dev"));
        assert_eq!(request.messages[1].text, "secret user");
        assert_eq!(request.messages[2].text, "secret assistant");
    }

    #[test]
    fn parse_round_trips_and_blank_is_empty() {
        assert!(RewriteConfig::parse("").unwrap().is_empty());
        assert!(RewriteConfig::parse("[]").is_err());
        let config = RewriteConfig {
            headers: vec![header("X-Test", "1")],
            ..Default::default()
        };
        let json = config.to_json().unwrap();
        assert_eq!(RewriteConfig::parse(&json).unwrap(), config);
    }

    #[test]
    fn shared_params_rewrite_typed_fields_and_passthrough() {
        let config = RewriteConfig {
            param_replace: vec![
                ParamReplaceRule {
                    field: "model".into(),
                    pattern: "^alias-".into(),
                    replacement: "real-".into(),
                },
                ParamReplaceRule {
                    field: "user".into(),
                    pattern: "SECRET".into(),
                    replacement: "PUBLIC".into(),
                },
            ],
            param_defaults: vec![
                ParamDefaultRule {
                    field: "temperature".into(),
                    value: json!(0.3),
                },
                ParamDefaultRule {
                    field: "top_p".into(),
                    value: json!(0.9),
                },
                ParamDefaultRule {
                    field: "logprobs".into(),
                    value: json!(true),
                },
            ],
            ..Default::default()
        };
        let mut request = ChatRequest {
            model: "alias-model".into(),
            top_p: Some(0.5),
            ..Default::default()
        };
        request.passthrough.insert("user".into(), json!("SECRET"));
        apply_shared_params(&mut request, &config);
        assert_eq!(request.model, "real-model");
        assert_eq!(request.temperature, Some(0.3));
        // An explicit client value wins over the default.
        assert_eq!(request.top_p, Some(0.5));
        assert_eq!(request.passthrough["user"], json!("PUBLIC"));
        assert_eq!(request.passthrough["logprobs"], json!(true));
    }
}
