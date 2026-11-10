use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    OpenAI,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
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
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    Other,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub text: String,
    pub finish_reason: FinishReason,
    pub input_tokens: u64,
    pub output_tokens: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Start {
        id: String,
        model: String,
    },
    Delta(String),
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Stop(FinishReason),
}

fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "request must be a JSON object".into())
}
fn text(value: Option<&Value>) -> Result<String, String> {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "text content is required".into())
}
fn unsupported(condition: bool, name: &str) -> Result<(), String> {
    if condition {
        Err(format!("unsupported_feature: {name}"))
    } else {
        Ok(())
    }
}
fn unsupported_present(value: &Map<String, Value>, name: &str) -> Result<(), String> {
    unsupported(value.get(name).is_some_and(|field| !field.is_null()), name)
}
fn empty_tools(value: Option<&Value>) -> Result<(), String> {
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(items)) if items.is_empty() => Ok(()),
        _ => Err("unsupported_feature: tools".into()),
    }
}
fn optional_bool(value: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a boolean")),
    }
}
fn optional_u64(value: &Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a positive integer")),
    }
}
fn optional_number(value: &Map<String, Value>, key: &str) -> Result<Option<f64>, String> {
    value
        .get(key)
        .filter(|v| !v.is_null())
        .map(|v| v.as_f64().ok_or_else(|| format!("{key} must be a number")))
        .transpose()
}
fn max_tokens(value: Option<&Value>) -> Result<Option<u32>, String> {
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
fn stops(value: Option<&Value>) -> Result<Vec<String>, String> {
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
fn reject_openai_fields(value: &Map<String, Value>) -> Result<(), String> {
    for name in [
        "response_format",
        "logprobs",
        "top_logprobs",
        "reasoning_effort",
        "tool_choice",
        "functions",
    ] {
        unsupported_present(value, name)?;
    }
    match optional_u64(value, "n")? {
        None | Some(1) => {}
        Some(_) => return Err("unsupported_feature: multiple candidates".into()),
    }
    empty_tools(value.get("tools"))
}

pub fn decode_request(
    protocol: Protocol,
    value: Value,
    path_model: Option<&str>,
) -> Result<ChatRequest, String> {
    let root = object(&value)?;
    match protocol {
        Protocol::OpenAI => {
            reject_openai_fields(root)?;
            let mut messages = Vec::new();
            let mut system = None;
            for message in root
                .get("messages")
                .and_then(Value::as_array)
                .ok_or("messages must be an array")?
            {
                let message = object(message)?;
                let role = match message.get("role").and_then(Value::as_str) {
                    Some("system" | "developer") => Role::System,
                    Some("user") => Role::User,
                    Some("assistant") => Role::Assistant,
                    _ => return Err("unsupported_feature: message role".into()),
                };
                let content = match message.get("content") {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .map(|part| {
                            let part = object(part)?;
                            if part.get("type").and_then(Value::as_str) != Some("text") {
                                return Err("unsupported_feature: non-text content".into());
                            }
                            text(part.get("text"))
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(""),
                    _ => return Err("unsupported_feature: non-text content".into()),
                };
                if role == Role::System {
                    system = Some(match system {
                        Some(previous) => format!("{previous}\n{content}"),
                        None => content,
                    });
                } else {
                    messages.push(Message {
                        role,
                        text: content,
                    });
                }
            }
            if messages.is_empty() {
                return Err("messages must include user or assistant input".into());
            }
            Ok(ChatRequest {
                model: path_model
                    .map(str::to_owned)
                    .or_else(|| root.get("model").and_then(Value::as_str).map(str::to_owned))
                    .filter(|s| !s.is_empty())
                    .ok_or("model is required")?,
                system,
                messages,
                max_tokens: max_tokens(
                    root.get("max_tokens")
                        .filter(|value| !value.is_null())
                        .or_else(|| root.get("max_completion_tokens")),
                )?,
                temperature: optional_number(root, "temperature")?,
                top_p: optional_number(root, "top_p")?,
                stop: stops(root.get("stop"))?,
                stream: optional_bool(root, "stream")?.unwrap_or(false),
            })
        }
        Protocol::Anthropic => {
            unsupported_present(root, "thinking")?;
            empty_tools(root.get("tools"))?;
            let _ = optional_bool(root, "stream")?;
            let system = root
                .get("system")
                .filter(|value| !value.is_null())
                .map(|v| match v {
                    Value::String(s) => Ok(s.clone()),
                    Value::Array(parts) => anthropic_blocks_text(parts, "\n"),
                    _ => Err("system must contain text".into()),
                })
                .transpose()?;
            let messages = root
                .get("messages")
                .and_then(Value::as_array)
                .ok_or("messages must be an array")?
                .iter()
                .map(|message| {
                    let message = object(message)?;
                    let role = match message.get("role").and_then(Value::as_str) {
                        Some("user") => Role::User,
                        Some("assistant") => Role::Assistant,
                        _ => return Err("unsupported_feature: message role".into()),
                    };
                    let content = match message.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(parts)) => anthropic_blocks_text(parts, "")?,
                        _ => return Err("content is required".into()),
                    };
                    Ok(Message {
                        role,
                        text: content,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if messages.is_empty() {
                return Err("messages must not be empty".into());
            }
            Ok(ChatRequest {
                model: path_model
                    .map(str::to_owned)
                    .or_else(|| root.get("model").and_then(Value::as_str).map(str::to_owned))
                    .filter(|s| !s.is_empty())
                    .ok_or("model is required")?,
                system,
                messages,
                max_tokens: Some(
                    max_tokens(root.get("max_tokens"))?.ok_or("max_tokens is required")?,
                ),
                temperature: optional_number(root, "temperature")?,
                top_p: optional_number(root, "top_p")?,
                stop: stops(root.get("stop_sequences"))?,
                stream: optional_bool(root, "stream")?.unwrap_or(false),
            })
        }
        Protocol::Gemini => {
            unsupported_present(root, "safetySettings")?;
            empty_tools(root.get("tools"))?;
            let _ = optional_bool(root, "stream")?;
            let generation = root
                .get("generationConfig")
                .filter(|value| !value.is_null())
                .map(object)
                .transpose()?
                .cloned()
                .unwrap_or_default();
            unsupported(
                optional_u64(&generation, "candidateCount")?.is_some_and(|count| count != 1),
                "multiple candidates",
            )?;
            unsupported_present(&generation, "thinkingConfig")?;
            let parts_text = |value: &Map<String, Value>| -> Result<String, String> {
                value
                    .get("parts")
                    .and_then(Value::as_array)
                    .ok_or("parts must be an array")?
                    .iter()
                    .map(|part| {
                        if object(part)?.len() != 1 {
                            return Err("unsupported_feature: non-text content".into());
                        }
                        text(object(part)?.get("text"))
                    })
                    .collect()
            };
            let system = root
                .get("systemInstruction")
                .filter(|value| !value.is_null())
                .map(object)
                .transpose()?
                .map(parts_text)
                .transpose()?;
            let messages = root
                .get("contents")
                .and_then(Value::as_array)
                .ok_or("contents must be an array")?
                .iter()
                .map(|message| {
                    let message = object(message)?;
                    Ok(Message {
                        role: match message
                            .get("role")
                            .and_then(Value::as_str)
                            .unwrap_or("user")
                        {
                            "user" => Role::User,
                            "model" => Role::Assistant,
                            _ => return Err("unsupported_feature: content role".into()),
                        },
                        text: parts_text(message)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if messages.is_empty() {
                return Err("contents must not be empty".into());
            }
            Ok(ChatRequest {
                model: path_model
                    .map(str::to_owned)
                    .filter(|s| !s.is_empty())
                    .ok_or("model path is required")?,
                system,
                messages,
                max_tokens: max_tokens(generation.get("maxOutputTokens"))?,
                temperature: optional_number(&generation, "temperature")?,
                top_p: optional_number(&generation, "topP")?,
                stop: stops(generation.get("stopSequences"))?,
                stream: false,
            })
        }
    }
}

fn openai_messages(request: &ChatRequest) -> Vec<Value> {
    request.system.iter().map(|text| json!({"role":"system","content":text})).chain(request.messages.iter().map(|m| json!({"role":match m.role { Role::System => "system", Role::User => "user", Role::Assistant => "assistant" },"content":m.text}))).collect()
}
pub fn encode_request(
    protocol: Protocol,
    request: &ChatRequest,
    model: &str,
) -> Result<Value, String> {
    if model.is_empty() {
        return Err("upstream model is required".into());
    }
    Ok(match protocol {
        Protocol::OpenAI => {
            let mut value =
                json!({"model":model,"messages":openai_messages(request),"stream":request.stream});
            let map = value.as_object_mut().unwrap();
            if let Some(n) = request.max_tokens {
                map.insert("max_tokens".into(), n.into());
            }
            if let Some(n) = request.temperature {
                map.insert("temperature".into(), n.into());
            }
            if let Some(n) = request.top_p {
                map.insert("top_p".into(), n.into());
            }
            if !request.stop.is_empty() {
                map.insert("stop".into(), json!(request.stop));
            }
            if request.stream {
                map.insert("stream_options".into(), json!({"include_usage":true}));
            }
            value
        }
        Protocol::Anthropic => {
            let mut value = json!({"model":model,"messages":request.messages.iter().map(|m| json!({"role":if m.role == Role::User {"user"} else {"assistant"},"content":m.text})).collect::<Vec<_>>(),"max_tokens":request.max_tokens.unwrap_or(1024),"stream":request.stream});
            let map = value.as_object_mut().unwrap();
            if let Some(text) = &request.system {
                map.insert("system".into(), text.clone().into());
            }
            if let Some(n) = request.temperature {
                map.insert("temperature".into(), n.into());
            }
            if let Some(n) = request.top_p {
                map.insert("top_p".into(), n.into());
            }
            if !request.stop.is_empty() {
                map.insert("stop_sequences".into(), json!(request.stop));
            }
            value
        }
        Protocol::Gemini => {
            let mut config = Map::new();
            if let Some(n) = request.max_tokens {
                config.insert("maxOutputTokens".into(), n.into());
            }
            if let Some(n) = request.temperature {
                config.insert("temperature".into(), n.into());
            }
            if let Some(n) = request.top_p {
                config.insert("topP".into(), n.into());
            }
            if !request.stop.is_empty() {
                config.insert("stopSequences".into(), json!(request.stop));
            }
            let mut value = json!({"contents":request.messages.iter().map(|m| json!({"role":if m.role == Role::User {"user"} else {"model"},"parts":[{"text":m.text}]})).collect::<Vec<_>>(),"generationConfig":config});
            if let Some(text) = &request.system {
                value
                    .as_object_mut()
                    .unwrap()
                    .insert("systemInstruction".into(), json!({"parts":[{"text":text}]}));
            }
            value
        }
    })
}

fn usage(value: &Map<String, Value>, input: &str, output: &str) -> (u64, u64) {
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
fn finish(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop" | "end_turn" | "STOP") | None => FinishReason::Stop,
        Some("length" | "max_tokens" | "MAX_TOKENS") => FinishReason::Length,
        _ => FinishReason::Other,
    }
}
fn openai_text_part(part: &Value) -> Result<String, String> {
    let part = object(part)?;
    if part.get("type").and_then(Value::as_str) != Some("text") {
        return Err("unsupported_feature: non-text content".into());
    }
    text(part.get("text"))
}
fn openai_content(value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Array(parts) => parts
            .iter()
            .map(openai_text_part)
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("")),
        _ => Err("unsupported_feature: non-text content".into()),
    }
}
fn openai_delta_content(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => openai_content(value).map(Some),
    }
}
fn has_tool_payload(value: &Map<String, Value>) -> bool {
    value.get("tool_calls").is_some_and(|calls| {
        !calls.is_null() && !matches!(calls, Value::Array(items) if items.is_empty())
    }) || value
        .get("function_call")
        .is_some_and(|call| !call.is_null())
        || value.get("audio").is_some_and(|audio| !audio.is_null())
}
fn anthropic_blocks_text(parts: &[Value], separator: &str) -> Result<String, String> {
    parts
        .iter()
        .map(|part| {
            let part = object(part)?;
            if part.get("type").and_then(Value::as_str) != Some("text") {
                return Err("unsupported_feature: non-text content".into());
            }
            text(part.get("text"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join(separator))
}
fn gemini_part_text(part: &Value) -> Result<String, String> {
    let part = object(part)?;
    for name in [
        "functionCall",
        "functionResponse",
        "inlineData",
        "fileData",
        "executableCode",
        "codeExecutionResult",
    ] {
        if part.get(name).is_some_and(|value| !value.is_null()) {
            return Err("unsupported_feature: non-text content".into());
        }
    }
    if part.get("thought").and_then(Value::as_bool) == Some(true) {
        return Err("unsupported_feature: thinking".into());
    }
    part.get("text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "unsupported_feature: non-text content".into())
}
fn gemini_parts_text(value: Option<&Value>) -> Result<String, String> {
    match value {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(gemini_part_text)
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("")),
        Some(_) => Err("content parts must be an array".into()),
    }
}
pub fn decode_response(protocol: Protocol, value: Value) -> Result<ChatResponse, String> {
    let root = object(&value)?;
    Ok(match protocol {
        Protocol::OpenAI => {
            let choices = root
                .get("choices")
                .and_then(Value::as_array)
                .ok_or("missing completion choice")?;
            if choices.is_empty() {
                return Err("missing completion choice".into());
            }
            if choices.len() > 1 {
                return Err("unsupported_feature: multiple choices".into());
            }
            let message = choices[0]
                .get("message")
                .map(object)
                .transpose()?
                .ok_or("missing completion message")?;
            if has_tool_payload(message) {
                return Err("unsupported_feature: tools".into());
            }
            let (input_tokens, output_tokens) = usage(root, "prompt_tokens", "completion_tokens");
            ChatResponse {
                id: root.get("id").and_then(Value::as_str).unwrap_or("").into(),
                model: root
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                text: openai_content(message.get("content").ok_or("missing message content")?)?,
                finish_reason: finish(choices[0].get("finish_reason").and_then(Value::as_str)),
                input_tokens,
                output_tokens,
            }
        }
        Protocol::Anthropic => {
            let content = root
                .get("content")
                .and_then(Value::as_array)
                .ok_or("missing content")?;
            let (input_tokens, output_tokens) = usage(root, "input_tokens", "output_tokens");
            ChatResponse {
                id: root.get("id").and_then(Value::as_str).unwrap_or("").into(),
                model: root
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                text: anthropic_blocks_text(content, "")?,
                finish_reason: finish(root.get("stop_reason").and_then(Value::as_str)),
                input_tokens,
                output_tokens,
            }
        }
        Protocol::Gemini => {
            let candidates = match root.get("candidates") {
                None | Some(Value::Null) => None,
                Some(Value::Array(items)) => Some(items),
                Some(_) => return Err("candidates must be an array".into()),
            };
            if candidates.is_some_and(|items| items.len() > 1) {
                return Err("unsupported_feature: multiple candidates".into());
            }
            let prompt_feedback = match root.get("promptFeedback") {
                None | Some(Value::Null) => None,
                Some(value) => Some(object(value)?),
            };
            let has_usage_metadata = root
                .get("usageMetadata")
                .is_some_and(|value| value.is_object());
            let has_candidates_key = root.contains_key("candidates");
            if !has_candidates_key && prompt_feedback.is_none() && !has_usage_metadata {
                return Err("missing completion candidate".into());
            }
            let choice = candidates
                .and_then(|items| items.first())
                .map(object)
                .transpose()?;
            let text = match choice {
                Some(choice) => match choice.get("content") {
                    None | Some(Value::Null) => String::new(),
                    Some(content) => gemini_parts_text(object(content)?.get("parts"))?,
                },
                None => String::new(),
            };
            let finish_reason = match choice
                .and_then(|choice| choice.get("finishReason"))
                .and_then(Value::as_str)
            {
                Some(reason) => finish(Some(reason)),
                None if prompt_feedback.is_some_and(|feedback| {
                    feedback
                        .get("blockReason")
                        .is_some_and(|reason| !reason.is_null())
                }) =>
                {
                    FinishReason::Other
                }
                None => FinishReason::Stop,
            };
            let (input_tokens, output_tokens) =
                usage(root, "promptTokenCount", "candidatesTokenCount");
            ChatResponse {
                id: String::new(),
                model: String::new(),
                text,
                finish_reason,
                input_tokens,
                output_tokens,
            }
        }
    })
}

pub fn encode_response(protocol: Protocol, response: &ChatResponse) -> Value {
    let reason = match response.finish_reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::Other => "error",
    };
    match protocol {
        Protocol::OpenAI => {
            json!({"id":response.id,"object":"chat.completion","created":0,"model":response.model,"choices":[{"index":0,"message":{"role":"assistant","content":response.text},"finish_reason":reason}],"usage":{"prompt_tokens":response.input_tokens,"completion_tokens":response.output_tokens,"total_tokens":response.input_tokens + response.output_tokens}})
        }
        Protocol::Anthropic => {
            json!({"id":response.id,"type":"message","role":"assistant","model":response.model,"content":[{"type":"text","text":response.text}],"stop_reason":match response.finish_reason { FinishReason::Stop => "end_turn", FinishReason::Length => "max_tokens", FinishReason::Other => "refusal" },"usage":{"input_tokens":response.input_tokens,"output_tokens":response.output_tokens}})
        }
        Protocol::Gemini => {
            json!({"candidates":[{"content":{"role":"model","parts":[{"text":response.text}]},"finishReason":match response.finish_reason { FinishReason::Stop => "STOP", FinishReason::Length => "MAX_TOKENS", FinishReason::Other => "OTHER" }}],"usageMetadata":{"promptTokenCount":response.input_tokens,"candidatesTokenCount":response.output_tokens,"totalTokenCount":response.input_tokens + response.output_tokens}})
        }
    }
}

fn openai_sse_data(event: &StreamEvent) -> Vec<(Option<&'static str>, Value)> {
    match event {
        StreamEvent::Start { id, model } => vec![(
            None,
            json!({"id":id,"object":"chat.completion.chunk","created":0,"model":model,"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}),
        )],
        StreamEvent::Delta(text) => vec![(
            None,
            json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
        )],
        StreamEvent::Usage {
            input_tokens,
            output_tokens,
        } => vec![(
            None,
            json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[],"usage":{"prompt_tokens":input_tokens,"completion_tokens":output_tokens}}),
        )],
        StreamEvent::Stop(reason) => vec![(
            None,
            json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[{"index":0,"delta":{},"finish_reason":match reason { FinishReason::Stop => "stop", FinishReason::Length => "length", FinishReason::Other => "error" }}]}),
        )],
    }
}
fn anthropic_stop_reason(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "end_turn",
        FinishReason::Length => "max_tokens",
        FinishReason::Other => "refusal",
    }
}
fn anthropic_sse_events(events: &[StreamEvent]) -> Vec<(Option<&'static str>, Value)> {
    let usage = events.iter().rev().find_map(|event| match event {
        StreamEvent::Usage {
            input_tokens,
            output_tokens,
        } => Some((*input_tokens, *output_tokens)),
        _ => None,
    });
    let stop = events.iter().rev().find_map(|event| match event {
        StreamEvent::Stop(reason) => Some(*reason),
        _ => None,
    });
    let mut output = Vec::new();
    let mut started = false;
    let mut stopped = false;
    let mut usage_emitted = false;
    for event in events {
        match event {
            StreamEvent::Start { id, model } => {
                if started {
                    continue;
                }
                started = true;
                let (input_tokens, output_tokens) = usage.unwrap_or((0, 0));
                output.push((
                    Some("message_start"),
                    json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens}}}),
                ));
                output.push((
                    Some("content_block_start"),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                ));
            }
            StreamEvent::Delta(text) => output.push((
                Some("content_block_delta"),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            )),
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => {
                if stopped {
                    continue;
                }
                let (final_input, final_output) = usage.unwrap_or((*input_tokens, *output_tokens));
                if let Some(reason) = stop {
                    output.push((
                        Some("content_block_stop"),
                        json!({"type":"content_block_stop","index":0}),
                    ));
                    output.push((
                        Some("message_delta"),
                        json!({"type":"message_delta","delta":{"stop_reason":anthropic_stop_reason(reason)},"usage":{"input_tokens":final_input,"output_tokens":final_output}}),
                    ));
                    output.push((Some("message_stop"), json!({"type":"message_stop"})));
                    stopped = true;
                } else {
                    if usage_emitted {
                        continue;
                    }
                    output.push((
                        Some("message_delta"),
                        json!({"type":"message_delta","delta":{},"usage":{"input_tokens":final_input,"output_tokens":final_output}}),
                    ));
                    usage_emitted = true;
                }
            }
            StreamEvent::Stop(reason) => {
                if stopped || usage.is_some() {
                    continue;
                }
                output.push((
                    Some("content_block_stop"),
                    json!({"type":"content_block_stop","index":0}),
                ));
                output.push((
                    Some("message_delta"),
                    json!({"type":"message_delta","delta":{"stop_reason":anthropic_stop_reason(*reason)}}),
                ));
                output.push((Some("message_stop"), json!({"type":"message_stop"})));
                stopped = true;
            }
        }
    }
    output
}
fn gemini_sse_events(events: &[StreamEvent]) -> Vec<(Option<&'static str>, Value)> {
    let usage = events.iter().rev().find_map(|event| match event {
        StreamEvent::Usage {
            input_tokens,
            output_tokens,
        } => Some((*input_tokens, *output_tokens)),
        _ => None,
    });
    let stop = events.iter().rev().find_map(|event| match event {
        StreamEvent::Stop(reason) => Some(*reason),
        _ => None,
    });
    let mut output = Vec::new();
    for event in events {
        if let StreamEvent::Delta(text) = event {
            output.push((
                None,
                json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]}}]}),
            ));
        }
    }
    let usage_metadata = |input_tokens: u64, output_tokens: u64| json!({"promptTokenCount":input_tokens,"candidatesTokenCount":output_tokens,"totalTokenCount":input_tokens + output_tokens});
    match stop {
        Some(reason) => {
            let mut response = json!({"candidates":[{"content":{"role":"model","parts":[]},"finishReason":match reason { FinishReason::Stop => "STOP", FinishReason::Length => "MAX_TOKENS", FinishReason::Other => "OTHER" }}]});
            if let Some((input_tokens, output_tokens)) = usage {
                let metadata = usage_metadata(input_tokens, output_tokens);
                response
                    .as_object_mut()
                    .expect("Gemini SSE response is an object")
                    .insert("usageMetadata".into(), metadata);
            }
            output.push((None, response));
        }
        None => {
            if let Some((input_tokens, output_tokens)) = usage {
                let metadata = usage_metadata(input_tokens, output_tokens);
                output.push((None, json!({"usageMetadata": metadata})));
            }
        }
    }
    output
}
pub fn encode_sse(protocol: Protocol, events: &[StreamEvent]) -> String {
    let mapped: Vec<(Option<&'static str>, Value)> = match protocol {
        Protocol::OpenAI => events.iter().flat_map(openai_sse_data).collect(),
        Protocol::Anthropic => anthropic_sse_events(events),
        Protocol::Gemini => gemini_sse_events(events),
    };
    let mut output = mapped
        .into_iter()
        .map(|(name, value)| {
            format!(
                "{}data: {}\n\n",
                name.map(|name| format!("event: {name}\n"))
                    .unwrap_or_default(),
                value
            )
        })
        .collect::<String>();
    if protocol == Protocol::OpenAI
        && events
            .iter()
            .any(|event| matches!(event, StreamEvent::Stop(_)))
    {
        output.push_str("data: [DONE]\n\n");
    }
    output
}
pub fn decode_sse(protocol: Protocol, input: &str) -> Result<Vec<StreamEvent>, String> {
    let mut output = Vec::new();
    let mut started = false;
    let mut stopped = false;
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut usage_seen = false;
    let mut usage_emitted = false;
    for block in input.replace("\r\n", "\n").split("\n\n") {
        let data = block
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let value: Value =
            serde_json::from_str(&data).map_err(|_| "invalid upstream SSE JSON".to_string())?;
        let root = object(&value)?;
        match protocol {
            Protocol::OpenAI => {
                if let Some(usage) = root.get("usage").filter(|usage| usage.is_object()) {
                    let usage = object(usage)?;
                    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_u64);
                    let completion_tokens = usage.get("completion_tokens").and_then(Value::as_u64);
                    if prompt_tokens.is_some() || completion_tokens.is_some() {
                        output.push(StreamEvent::Usage {
                            input_tokens: prompt_tokens.unwrap_or(0),
                            output_tokens: completion_tokens.unwrap_or(0),
                        });
                    }
                }
                let choices = match root.get("choices") {
                    None | Some(Value::Null) => None,
                    Some(Value::Array(choices)) => Some(choices),
                    Some(_) => return Err("choices must be an array".into()),
                };
                if choices.is_some_and(|choices| choices.len() > 1) {
                    return Err("unsupported_feature: multiple choices".into());
                }
                if let Some(choice) = choices.and_then(|choices| choices.first()) {
                    let choice = object(choice)?;
                    let delta = match choice.get("delta") {
                        None | Some(Value::Null) => None,
                        Some(delta) => Some(object(delta)?),
                    };
                    if let Some(delta) = delta {
                        if has_tool_payload(delta) {
                            return Err("unsupported_feature: tools".into());
                        }
                        match delta.get("role") {
                            None | Some(Value::Null) => {}
                            Some(role) if role.as_str() == Some("assistant") => {
                                if !started {
                                    output.push(StreamEvent::Start {
                                        id: root
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .into(),
                                        model: root
                                            .get("model")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .into(),
                                    });
                                    started = true;
                                }
                            }
                            Some(_) => {
                                return Err("unsupported_feature: message role".into());
                            }
                        }
                        if let Some(content) = openai_delta_content(delta.get("content"))?
                            .filter(|content| !content.is_empty())
                        {
                            output.push(StreamEvent::Delta(content));
                        }
                    }
                    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str)
                        && !stopped
                    {
                        output.push(StreamEvent::Stop(finish(Some(reason))));
                        stopped = true;
                    }
                }
            }
            Protocol::Anthropic => match root.get("type").and_then(Value::as_str) {
                Some("message_start") => {
                    let message = root.get("message").and_then(Value::as_object);
                    if let Some(message) = message
                        && let Some(usage) = message.get("usage").filter(|usage| usage.is_object())
                    {
                        let usage = object(usage)?;
                        input_tokens = usage
                            .get("input_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(input_tokens);
                        output_tokens = usage
                            .get("output_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(output_tokens);
                        usage_seen = true;
                    }
                    if !started {
                        output.push(StreamEvent::Start {
                            id: message
                                .and_then(|message| message.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                            model: message
                                .and_then(|message| message.get("model"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                        });
                        started = true;
                    }
                }
                Some("content_block_start") => {
                    let block = root
                        .get("content_block")
                        .map(object)
                        .transpose()?
                        .ok_or("missing content block")?;
                    if block.get("type").and_then(Value::as_str) != Some("text") {
                        return Err("unsupported_feature: non-text content".into());
                    }
                }
                Some("content_block_delta") => {
                    let delta = object(root.get("delta").ok_or("missing content delta")?)?;
                    if let Some(kind) = delta.get("type").and_then(Value::as_str)
                        && kind != "text_delta"
                    {
                        return Err("unsupported_feature: non-text content".into());
                    }
                    let content = text(delta.get("text"))?;
                    if !content.is_empty() {
                        output.push(StreamEvent::Delta(content));
                    }
                }
                Some("content_block_stop") => {}
                Some("message_delta") => {
                    if let Some(usage) = root.get("usage").filter(|usage| usage.is_object()) {
                        let usage = object(usage)?;
                        let explicit_input = usage.get("input_tokens").and_then(Value::as_u64);
                        let explicit_output = usage.get("output_tokens").and_then(Value::as_u64);
                        input_tokens = explicit_input.unwrap_or(input_tokens);
                        output_tokens = explicit_output.unwrap_or(output_tokens);
                        usage_seen = true;
                        if explicit_input.is_some() || explicit_output.is_some() {
                            output.push(StreamEvent::Usage {
                                input_tokens,
                                output_tokens,
                            });
                            usage_emitted = true;
                        }
                    }
                    if let Some(reason) =
                        value.pointer("/delta/stop_reason").and_then(Value::as_str)
                        && !stopped
                    {
                        output.push(StreamEvent::Stop(finish(Some(reason))));
                        stopped = true;
                    }
                }
                Some("message_stop") => {
                    if !usage_emitted && usage_seen && (input_tokens != 0 || output_tokens != 0) {
                        output.push(StreamEvent::Usage {
                            input_tokens,
                            output_tokens,
                        });
                        usage_emitted = true;
                    }
                    if !stopped {
                        output.push(StreamEvent::Stop(FinishReason::Stop));
                        stopped = true;
                    }
                }
                Some("ping") => {}
                Some("error") => return Err("upstream stream error".into()),
                _ => return Err("unsupported upstream SSE event".into()),
            },
            Protocol::Gemini => {
                let candidates = match root.get("candidates") {
                    None | Some(Value::Null) => None,
                    Some(Value::Array(items)) => Some(items),
                    Some(_) => return Err("candidates must be an array".into()),
                };
                if candidates.is_some_and(|items| items.len() > 1) {
                    return Err("unsupported_feature: multiple candidates".into());
                }
                let prompt_feedback = match root.get("promptFeedback") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(object(value)?),
                };
                let ChatResponse {
                    text,
                    finish_reason,
                    input_tokens: response_input,
                    output_tokens: response_output,
                    ..
                } = decode_response(protocol, value.clone())?;
                if !started {
                    output.push(StreamEvent::Start {
                        id: String::new(),
                        model: String::new(),
                    });
                    started = true;
                }
                if !text.is_empty() {
                    output.push(StreamEvent::Delta(text));
                }
                if response_input > 0 || response_output > 0 {
                    output.push(StreamEvent::Usage {
                        input_tokens: response_input,
                        output_tokens: response_output,
                    });
                }
                let has_finish = candidates
                    .and_then(|items| items.first())
                    .and_then(|candidate| candidate.get("finishReason"))
                    .and_then(Value::as_str)
                    .is_some();
                if has_finish && !stopped {
                    output.push(StreamEvent::Stop(finish_reason));
                    stopped = true;
                } else if !has_finish
                    && !stopped
                    && prompt_feedback.is_some_and(|feedback| {
                        feedback
                            .get("blockReason")
                            .is_some_and(|reason| !reason.is_null())
                    })
                {
                    output.push(StreamEvent::Stop(FinishReason::Other));
                    stopped = true;
                }
            }
        }
    }
    Ok(output)
}
