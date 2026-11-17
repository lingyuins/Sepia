use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    OpenAI,
    Anthropic,
    Gemini,
    /// Outbound-only: relays to an upstream Responses API. Inbound Responses
    /// requests are normalized by `crate::responses` before entering the relay.
    #[serde(rename = "openai-responses")]
    OpenAIResponses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(String),
    Image {
        url: Option<String>,
        data: Option<String>,
        media_type: String,
        file_id: Option<String>,
    },
    Audio {
        data: String,
        media_type: String,
    },
    File {
        url: Option<String>,
        data: Option<String>,
        media_type: Option<String>,
        filename: Option<String>,
        file_id: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        call_id: String,
        name: Option<String>,
        content: String,
        is_error: bool,
    },
}

impl ContentBlock {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }
    pub fn as_text(&self) -> Option<&str> {
        if let Self::Text(text) = self {
            Some(text)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            role,
            content: vec![ContentBlock::Text(text.clone())],
            text,
        }
    }
    pub fn with_content(role: Role, content: Vec<ContentBlock>) -> Self {
        let text = content
            .iter()
            .filter_map(ContentBlock::as_text)
            .collect::<String>();
        Self {
            role,
            text,
            content,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolChoice {
    #[default]
    Auto,
    None,
    Required,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub stop: Vec<String>,
    pub stream: bool,
    pub tools: Vec<Tool>,
    pub tool_choice: ToolChoice,
    pub allowed_tools: Vec<String>,
    pub passthrough: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    ToolUse,
    ContentFilter,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub text: String,
    pub content: Vec<ContentBlock>,
    pub finish_reason: FinishReason,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub reasoning_tokens: u64,
}

impl ChatResponse {
    pub fn from_content(
        id: String,
        model: String,
        content: Vec<ContentBlock>,
        finish_reason: FinishReason,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Self {
        let text = content.iter().filter_map(ContentBlock::as_text).collect();
        Self {
            id,
            model,
            text,
            content,
            finish_reason,
            input_tokens,
            output_tokens,
            cached_tokens: 0,
            reasoning_tokens: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Start {
        id: String,
        model: String,
    },
    Delta(String),
    Content(ContentBlock),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        arguments: String,
    },
    ToolCallEnd {
        index: usize,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        cached_tokens: u64,
        reasoning_tokens: u64,
    },
    Stop(FinishReason),
}

pub fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "request must be a JSON object".into())
}
pub fn text(value: Option<&Value>) -> Result<String, String> {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "text content is required".into())
}
pub fn unsupported(condition: bool, name: &str) -> Result<(), String> {
    if condition {
        Err(format!("unsupported_feature: {name}"))
    } else {
        Ok(())
    }
}
pub fn unsupported_present(value: &Map<String, Value>, name: &str) -> Result<(), String> {
    unsupported(value.get(name).is_some_and(|field| !field.is_null()), name)
}
pub fn optional_bool(value: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a boolean")),
    }
}
pub fn optional_u64(value: &Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a positive integer")),
    }
}
pub fn optional_number(value: &Map<String, Value>, key: &str) -> Result<Option<f64>, String> {
    value
        .get(key)
        .filter(|v| !v.is_null())
        .map(|v| v.as_f64().ok_or_else(|| format!("{key} must be a number")))
        .transpose()
}
pub fn max_tokens(value: Option<&Value>) -> Result<Option<u32>, String> {
    value
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| "max_tokens must be a positive integer".into())
        })
        .transpose()
}
pub fn stops(value: Option<&Value>) -> Result<Vec<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(value)) => Ok(vec![value.clone()]),
        Some(Value::Array(values)) => values
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "stop must contain strings".into())
            })
            .collect(),
        _ => Err("stop must be a string or string array".into()),
    }
}

pub fn data_url(value: &str) -> Option<(String, String)> {
    let (metadata, data) = value.strip_prefix("data:")?.split_once(',')?;
    Some((metadata.split(';').next()?.to_string(), data.to_string()))
}
pub fn media_url(url: Option<String>, data: Option<String>, media_type: &str) -> String {
    url.or_else(|| data.map(|data| format!("data:{media_type};base64,{data}")))
        .unwrap_or_default()
}
pub fn base64_source(data: String, media_type: String) -> Value {
    json!({"type":"base64","data":data,"media_type":media_type})
}
pub fn usage(value: &Map<String, Value>, input: &str, output: &str) -> (u64, u64) {
    let usage = value.get("usage").or_else(|| value.get("usageMetadata"));
    (
        usage
            .and_then(|v| v.get(input))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        usage
            .and_then(|v| v.get(output))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
}
/// Cached-input / reasoning-output token detail. Vendors nest these under
/// different keys; missing detail means zero, never an invented value.
pub fn usage_details(value: &Value) -> (u64, u64) {
    let cached = [
        "/prompt_tokens_details/cached_tokens",
        "/input_tokens_details/cached_tokens",
        "/cache_read_input_tokens",
        "/cachedContentTokenCount",
    ]
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))
    .unwrap_or(0);
    let reasoning = [
        "/completion_tokens_details/reasoning_tokens",
        "/output_tokens_details/reasoning_tokens",
        "/thoughtsTokenCount",
    ]
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))
    .unwrap_or(0);
    (cached, reasoning)
}
pub fn finish(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop" | "end_turn" | "STOP") | None => FinishReason::Stop,
        Some("length" | "max_tokens" | "MAX_TOKENS") => FinishReason::Length,
        Some("tool_calls" | "tool_use" | "function_call") => FinishReason::ToolUse,
        Some("content_filter" | "SAFETY" | "PROHIBITED_CONTENT" | "RECITATION") => {
            FinishReason::ContentFilter
        }
        _ => FinishReason::Other,
    }
}
pub fn openai_finish(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolUse => "tool_calls",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Other => "error",
    }
}
pub fn anthropic_finish(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "end_turn",
        FinishReason::Length => "max_tokens",
        FinishReason::ToolUse => "tool_use",
        FinishReason::ContentFilter => "refusal",
        FinishReason::Other => "refusal",
    }
}
pub fn gemini_finish(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop | FinishReason::ToolUse => "STOP",
        FinishReason::Length => "MAX_TOKENS",
        FinishReason::ContentFilter => "SAFETY",
        FinishReason::Other => "OTHER",
    }
}
pub fn openai_role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}
pub fn anthropic_role(role: Role) -> &'static str {
    if role == Role::Assistant {
        "assistant"
    } else {
        "user"
    }
}
pub fn gemini_role(role: Role) -> &'static str {
    if role == Role::Assistant {
        "model"
    } else {
        "user"
    }
}
