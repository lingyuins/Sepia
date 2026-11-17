mod anthropic;
mod gemini;
mod model;
mod openai;
mod openai_responses;

use serde_json::Value;

pub use model::*;

#[derive(Default)]
pub struct AnthropicState {
    indices: std::collections::HashMap<usize, Option<(String, String)>>,
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    finished: bool,
}

#[derive(Default)]
pub struct ResponsesState {
    calls: std::collections::BTreeMap<usize, usize>,
    next_call: usize,
    pending_arguments: std::collections::BTreeMap<usize, String>,
    text_delta_seen: std::collections::BTreeSet<usize>,
    finished: bool,
}

pub fn openai_tool_choice_for_anthropic(value: &str) -> Result<(ToolChoice, Vec<String>), String> {
    match value {
        "auto" => Ok((ToolChoice::Auto, Vec::new())),
        "none" => Ok((ToolChoice::None, Vec::new())),
        "required" | "any" => Ok((ToolChoice::Required, Vec::new())),
        _ => Err("invalid tool_choice".into()),
    }
}

pub fn decode_request(
    protocol: Protocol,
    value: Value,
    path_model: Option<&str>,
) -> Result<ChatRequest, String> {
    match protocol {
        Protocol::OpenAI => openai::decode_request(value, path_model),
        Protocol::Anthropic => anthropic::decode_request(value, path_model),
        Protocol::Gemini => gemini::decode_request(value, path_model),
        Protocol::OpenAIResponses => {
            Err("openai-responses is an outbound-only provider protocol".into())
        }
    }
}
pub fn encode_request(
    protocol: Protocol,
    request: &ChatRequest,
    model: &str,
) -> Result<Value, String> {
    match protocol {
        Protocol::OpenAI => openai::encode_request(request, model),
        Protocol::Anthropic => anthropic::encode_request(request, model),
        Protocol::Gemini => gemini::encode_request(request, model),
        Protocol::OpenAIResponses => openai_responses::encode_request(request, model),
    }
}
pub fn decode_response(protocol: Protocol, value: Value) -> Result<ChatResponse, String> {
    match protocol {
        Protocol::OpenAI => openai::decode_response(value),
        Protocol::Anthropic => anthropic::decode_response(value),
        Protocol::Gemini => gemini::decode_response(value),
        Protocol::OpenAIResponses => openai_responses::decode_response(value),
    }
}
pub fn encode_response(protocol: Protocol, response: &ChatResponse) -> Value {
    match protocol {
        Protocol::OpenAI => openai::encode_response(response),
        Protocol::Anthropic => anthropic::encode_response(response),
        Protocol::Gemini => gemini::encode_response(response),
        Protocol::OpenAIResponses => crate::responses::encode_response(response),
    }
}
pub fn encode_sse(protocol: Protocol, events: &[StreamEvent]) -> String {
    let mapped = match protocol {
        Protocol::OpenAI => openai::encode_sse(events),
        Protocol::Anthropic => anthropic::encode_sse(events),
        Protocol::Gemini => gemini::encode_sse(events),
        Protocol::OpenAIResponses => {
            return crate::responses::StreamEncoder::new(String::new(), String::new())
                .encode(events);
        }
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
    SseDecoder::new(protocol).decode(input)
}

/// Streaming decoder whose state persists across individual upstream frames.
/// `decode_sse` resets its state per call and is only correct for whole inputs;
/// the relay reads one frame at a time and must keep this decoder alive for the
/// lifetime of the upstream stream.
#[derive(Default)]
pub struct SseDecoder {
    protocol: Protocol,
    anthropic: AnthropicState,
    responses: ResponsesState,
    gemini_started: bool,
    gemini_had_tool_call: bool,
}

impl SseDecoder {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            ..Self::default()
        }
    }
    pub fn decode(&mut self, frame: &str) -> Result<Vec<StreamEvent>, String> {
        let mut output = Vec::new();
        for block in frame.replace("\r\n", "\n").split("\n\n") {
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
            output.extend(match self.protocol {
                Protocol::OpenAI => openai::decode_sse(root)?,
                Protocol::Anthropic => anthropic::decode_sse(root, &mut self.anthropic)?,
                Protocol::Gemini => gemini::decode_sse(
                    value,
                    &mut self.gemini_started,
                    &mut self.gemini_had_tool_call,
                )?,
                Protocol::OpenAIResponses => {
                    openai_responses::decode_sse(root, &mut self.responses)?
                }
            });
        }
        Ok(output)
    }
}
