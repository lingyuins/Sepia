mod anthropic;
mod gemini;
mod model;
mod openai;

use serde_json::Value;

pub use model::*;

#[derive(Default)]
pub struct AnthropicState {
    indices: std::collections::HashMap<usize, Option<(String, String)>>,
    input_tokens: u64,
    output_tokens: u64,
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
    }
}
pub fn decode_response(protocol: Protocol, value: Value) -> Result<ChatResponse, String> {
    match protocol {
        Protocol::OpenAI => openai::decode_response(value),
        Protocol::Anthropic => anthropic::decode_response(value),
        Protocol::Gemini => gemini::decode_response(value),
    }
}
pub fn encode_response(protocol: Protocol, response: &ChatResponse) -> Value {
    match protocol {
        Protocol::OpenAI => openai::encode_response(response),
        Protocol::Anthropic => anthropic::encode_response(response),
        Protocol::Gemini => gemini::encode_response(response),
    }
}
pub fn encode_sse(protocol: Protocol, events: &[StreamEvent]) -> String {
    let mapped = match protocol {
        Protocol::OpenAI => openai::encode_sse(events),
        Protocol::Anthropic => anthropic::encode_sse(events),
        Protocol::Gemini => gemini::encode_sse(events),
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
    let mut anthropic_state = AnthropicState::default();
    let mut gemini_started = false;
    let mut gemini_had_tool_call = false;
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
        output.extend(match protocol {
            Protocol::OpenAI => openai::decode_sse(root)?,
            Protocol::Anthropic => anthropic::decode_sse(root, &mut anthropic_state)?,
            Protocol::Gemini => {
                gemini::decode_sse(value, &mut gemini_started, &mut gemini_had_tool_call)?
            }
        });
    }
    Ok(output)
}
