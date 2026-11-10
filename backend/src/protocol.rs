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
        unsupported(value.contains_key(name), name)?;
    }
    if value.get("n").and_then(Value::as_u64).unwrap_or(1) != 1 {
        return Err("unsupported_feature: multiple candidates".into());
    }
    if value
        .get("tools")
        .is_some_and(|v| !matches!(v, Value::Array(items) if items.is_empty()))
    {
        return Err("unsupported_feature: tools".into());
    }
    Ok(())
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
                        .or_else(|| root.get("max_completion_tokens")),
                )?,
                temperature: optional_number(root, "temperature")?,
                top_p: optional_number(root, "top_p")?,
                stop: stops(root.get("stop"))?,
                stream: root.get("stream").and_then(Value::as_bool).unwrap_or(false),
            })
        }
        Protocol::Anthropic => {
            unsupported(root.contains_key("thinking"), "thinking")?;
            unsupported(
                root.get("tools")
                    .is_some_and(|v| !matches!(v, Value::Array(items) if items.is_empty())),
                "tools",
            )?;
            let system = root
                .get("system")
                .map(|v| match v {
                    Value::String(s) => Ok(s.clone()),
                    Value::Array(parts) => parts
                        .iter()
                        .map(|part| text(object(part)?.get("text")))
                        .collect::<Result<Vec<_>, _>>()
                        .map(|v| v.join("\n")),
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
                stream: root.get("stream").and_then(Value::as_bool).unwrap_or(false),
            })
        }
        Protocol::Gemini => {
            unsupported(root.contains_key("safetySettings"), "safetySettings")?;
            unsupported(
                root.get("tools")
                    .is_some_and(|v| !matches!(v, Value::Array(items) if items.is_empty())),
                "tools",
            )?;
            let generation = root
                .get("generationConfig")
                .map(object)
                .transpose()?
                .cloned()
                .unwrap_or_default();
            unsupported(
                generation
                    .get("candidateCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    != 1,
                "multiple candidates",
            )?;
            unsupported(generation.contains_key("thinkingConfig"), "thinking")?;
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
pub fn decode_response(protocol: Protocol, value: Value) -> Result<ChatResponse, String> {
    let root = object(&value)?;
    Ok(match protocol {
        Protocol::OpenAI => {
            let choice = root
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|v| v.first())
                .ok_or("missing completion choice")?;
            let (input_tokens, output_tokens) = usage(root, "prompt_tokens", "completion_tokens");
            ChatResponse {
                id: root.get("id").and_then(Value::as_str).unwrap_or("").into(),
                model: root
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                text: choice
                    .pointer("/message/content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                finish_reason: finish(choice.get("finish_reason").and_then(Value::as_str)),
                input_tokens,
                output_tokens,
            }
        }
        Protocol::Anthropic => {
            let (input_tokens, output_tokens) = usage(root, "input_tokens", "output_tokens");
            ChatResponse {
                id: root.get("id").and_then(Value::as_str).unwrap_or("").into(),
                model: root
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                text: root
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or("missing content")?
                    .iter()
                    .map(|v| v.get("text").and_then(Value::as_str).unwrap_or(""))
                    .collect(),
                finish_reason: finish(root.get("stop_reason").and_then(Value::as_str)),
                input_tokens,
                output_tokens,
            }
        }
        Protocol::Gemini => {
            let choice = root
                .get("candidates")
                .and_then(Value::as_array)
                .and_then(|v| v.first())
                .ok_or("missing completion candidate")?;
            let (input_tokens, output_tokens) =
                usage(root, "promptTokenCount", "candidatesTokenCount");
            ChatResponse {
                id: String::new(),
                model: String::new(),
                text: choice
                    .pointer("/content/parts")
                    .and_then(Value::as_array)
                    .ok_or("missing content parts")?
                    .iter()
                    .map(|v| v.get("text").and_then(Value::as_str).unwrap_or(""))
                    .collect(),
                finish_reason: finish(choice.get("finishReason").and_then(Value::as_str)),
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

fn sse_data(
    protocol: Protocol,
    event: &StreamEvent,
    events: &[StreamEvent],
) -> Vec<(Option<&'static str>, Value)> {
    match (protocol, event) {
        (Protocol::OpenAI, StreamEvent::Start { id, model }) => vec![(
            None,
            json!({"id":id,"object":"chat.completion.chunk","created":0,"model":model,"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}),
        )],
        (Protocol::OpenAI, StreamEvent::Delta(text)) => vec![(
            None,
            json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
        )],
        (
            Protocol::OpenAI,
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
            },
        ) => vec![(
            None,
            json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[],"usage":{"prompt_tokens":input_tokens,"completion_tokens":output_tokens}}),
        )],
        (Protocol::OpenAI, StreamEvent::Stop(reason)) => vec![(
            None,
            json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[{"index":0,"delta":{},"finish_reason":match reason { FinishReason::Stop => "stop", FinishReason::Length => "length", FinishReason::Other => "error" }}]}),
        )],
        (Protocol::Anthropic, StreamEvent::Start { id, model }) => vec![(
            Some("message_start"),
            json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[]}}),
        )],
        (Protocol::Anthropic, StreamEvent::Delta(text)) => vec![
            (
                Some("content_block_start"),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            ),
            (
                Some("content_block_delta"),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            ),
        ],
        (
            Protocol::Anthropic,
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
            },
        ) => {
            if let Some(index) = events
                .iter()
                .position(|event| matches!(event, StreamEvent::Stop(_)))
            {
                let StreamEvent::Stop(reason) = events[index] else {
                    unreachable!()
                };
                return vec![
                    (
                        Some("message_delta"),
                        json!({"type":"message_delta","delta":{"stop_reason":match reason { FinishReason::Stop => "end_turn", FinishReason::Length => "max_tokens", FinishReason::Other => "refusal" }},"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens}}),
                    ),
                    (Some("message_stop"), json!({"type":"message_stop"})),
                ];
            }
            vec![(
                Some("message_delta"),
                json!({"type":"message_delta","delta":{},"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens}}),
            )]
        }
        (Protocol::Anthropic, StreamEvent::Stop(_))
            if events
                .iter()
                .any(|event| matches!(event, StreamEvent::Usage { .. })) =>
        {
            Vec::new()
        }
        (Protocol::Anthropic, StreamEvent::Stop(reason)) => vec![
            (
                Some("message_delta"),
                json!({"type":"message_delta","delta":{"stop_reason":match reason { FinishReason::Stop => "end_turn", FinishReason::Length => "max_tokens", FinishReason::Other => "refusal" }}}),
            ),
            (Some("message_stop"), json!({"type":"message_stop"})),
        ],
        (Protocol::Gemini, StreamEvent::Start { .. }) => Vec::new(),
        (Protocol::Gemini, event) => {
            let response = match event {
                StreamEvent::Delta(text) => ChatResponse {
                    text: text.clone(),
                    ..ChatResponse::default()
                },
                StreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                } => ChatResponse {
                    input_tokens: *input_tokens,
                    output_tokens: *output_tokens,
                    ..ChatResponse::default()
                },
                StreamEvent::Stop(reason) => ChatResponse {
                    finish_reason: *reason,
                    ..ChatResponse::default()
                },
                StreamEvent::Start { .. } => unreachable!(),
            };
            vec![(None, encode_response(Protocol::Gemini, &response))]
        }
    }
}
pub fn encode_sse(protocol: Protocol, events: &[StreamEvent]) -> String {
    let mapped = if protocol == Protocol::Gemini && events.len() != 1 {
        let mut final_response = ChatResponse::default();
        let mut has_stop = false;
        let mut output = Vec::new();
        for event in events {
            match event {
                StreamEvent::Start { .. } => {}
                StreamEvent::Delta(text) => output.push((
                    None,
                    json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]}}]}),
                )),
                StreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                } => {
                    final_response.input_tokens = *input_tokens;
                    final_response.output_tokens = *output_tokens;
                }
                StreamEvent::Stop(reason) => {
                    final_response.finish_reason = *reason;
                    has_stop = true;
                }
            }
        }
        if has_stop {
            output.push((None, encode_response(Protocol::Gemini, &final_response)));
        }
        output
    } else {
        events
            .iter()
            .flat_map(|event| sse_data(protocol, event, events))
            .collect()
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
        match protocol {
            Protocol::OpenAI => {
                if let Some(usage) = value.get("usage") {
                    output.push(StreamEvent::Usage {
                        input_tokens: usage
                            .get("prompt_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        output_tokens: usage
                            .get("completion_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    });
                }
                if let Some(choice) = value
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|v| v.first())
                {
                    if value.pointer("/choices/0/delta/role").is_some() {
                        output.push(StreamEvent::Start {
                            id: value.get("id").and_then(Value::as_str).unwrap_or("").into(),
                            model: value
                                .get("model")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                        });
                    }
                    if let Some(text) = choice.pointer("/delta/content").and_then(Value::as_str) {
                        output.push(StreamEvent::Delta(text.into()));
                    }
                    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                        output.push(StreamEvent::Stop(finish(Some(reason))));
                    }
                }
            }
            Protocol::Anthropic => match value.get("type").and_then(Value::as_str) {
                Some("message_start") => output.push(StreamEvent::Start {
                    id: value
                        .pointer("/message/id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    model: value
                        .pointer("/message/model")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                }),
                Some("content_block_delta") => {
                    output.push(StreamEvent::Delta(text(value.pointer("/delta/text"))?))
                }
                Some("message_delta") => {
                    if let Some(usage) = value.get("usage") {
                        output.push(StreamEvent::Usage {
                            input_tokens: usage
                                .get("input_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                            output_tokens: usage
                                .get("output_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        });
                    }
                    if let Some(reason) =
                        value.pointer("/delta/stop_reason").and_then(Value::as_str)
                    {
                        output.push(StreamEvent::Stop(finish(Some(reason))));
                    }
                }
                Some("message_stop" | "content_block_start" | "content_block_stop" | "ping") => {}
                _ => return Err("unsupported upstream SSE event".into()),
            },
            Protocol::Gemini => {
                let has_finish = object(&value)?
                    .get("candidates")
                    .and_then(Value::as_array)
                    .and_then(|v| v.first())
                    .is_some_and(|v| v.get("finishReason").is_some());
                let response = decode_response(protocol, value)?;
                if output.is_empty() {
                    output.push(StreamEvent::Start {
                        id: String::new(),
                        model: String::new(),
                    });
                }
                if !response.text.is_empty() {
                    output.push(StreamEvent::Delta(response.text));
                }
                if has_finish && (response.input_tokens > 0 || response.output_tokens > 0) {
                    output.push(StreamEvent::Usage {
                        input_tokens: response.input_tokens,
                        output_tokens: response.output_tokens,
                    });
                }
                if has_finish {
                    output.push(StreamEvent::Stop(response.finish_reason));
                }
            }
        }
    }
    Ok(output)
}
