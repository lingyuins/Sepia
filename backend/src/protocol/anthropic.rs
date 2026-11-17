use super::model::*;
use serde_json::{Map, Value, json};

fn content_block(value: &Value) -> Result<ContentBlock, String> {
    let part = object(value)?;
    match part
        .get("type")
        .and_then(Value::as_str)
        .ok_or("content type is required")?
    {
        "text" | "input_text" | "output_text" => Ok(ContentBlock::Text(text(part.get("text"))?)),
        "image" => {
            let source = object(part.get("source").ok_or("image source is required")?)?;
            match source.get("type").and_then(Value::as_str) {
                Some("base64") => Ok(ContentBlock::Image {
                    url: None,
                    data: Some(text(source.get("data"))?),
                    media_type: text(source.get("media_type"))?,
                    file_id: None,
                }),
                Some("url") => Ok(ContentBlock::Image {
                    url: Some(text(source.get("url"))?),
                    data: None,
                    media_type: "image/*".into(),
                    file_id: None,
                }),
                Some("file") => Ok(ContentBlock::Image {
                    url: None,
                    data: None,
                    media_type: "image/*".into(),
                    file_id: Some(text(source.get("file_id"))?),
                }),
                _ => Err("unsupported image source".into()),
            }
        }
        "document" => {
            let source = object(part.get("source").ok_or("document source is required")?)?;
            let (url, data, file_id, media_type) = match source.get("type").and_then(Value::as_str)
            {
                Some("base64") => (
                    None,
                    Some(text(source.get("data"))?),
                    None,
                    source
                        .get("media_type")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                ),
                Some("url") => (
                    Some(text(source.get("url"))?),
                    None,
                    None,
                    source
                        .get("media_type")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                ),
                Some("file") => (
                    None,
                    None,
                    Some(text(source.get("file_id"))?),
                    source
                        .get("media_type")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                ),
                Some("text") => (
                    None,
                    Some(text(source.get("data"))?),
                    None,
                    Some("text/plain".into()),
                ),
                _ => return Err("unsupported document source".into()),
            };
            Ok(ContentBlock::File {
                url,
                data,
                file_id,
                media_type,
                filename: part.get("title").and_then(Value::as_str).map(str::to_owned),
            })
        }
        "tool_use" => Ok(ContentBlock::ToolCall {
            id: text(part.get("id"))?,
            name: text(part.get("name"))?,
            arguments: part
                .get("input")
                .cloned()
                .unwrap_or_else(|| json!({}))
                .to_string(),
        }),
        "tool_result" => {
            let content = match part.get("content") {
                Some(Value::String(value)) => value.clone(),
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|part| part.get("text"))
                    .filter_map(Value::as_str)
                    .collect(),
                Some(value) => value.to_string(),
                None => String::new(),
            };
            Ok(ContentBlock::ToolResult {
                call_id: text(part.get("tool_use_id"))?,
                name: None,
                content,
                is_error: part
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        }
        kind => Err(format!("unsupported_feature: content type {kind}")),
    }
}
fn content(value: Option<&Value>) -> Result<Vec<ContentBlock>, String> {
    match value {
        Some(Value::String(text)) => Ok(vec![ContentBlock::Text(text.clone())]),
        Some(Value::Array(parts)) => parts.iter().map(content_block).collect(),
        _ => Err("content is required".into()),
    }
}
pub fn decode_request(value: Value, path_model: Option<&str>) -> Result<ChatRequest, String> {
    let root = object(&value)?;
    let system = root
        .get("system")
        .filter(|value| !value.is_null())
        .map(|value| -> Result<String, String> {
            match value {
                Value::String(text) => Ok(text.clone()),
                Value::Array(parts) => Ok(parts
                    .iter()
                    .map(content_block)
                    .collect::<Result<Vec<_>, _>>()?
                    .iter()
                    .filter_map(ContentBlock::as_text)
                    .collect::<Vec<_>>()
                    .join("\n")),
                _ => Err("system must contain text".into()),
            }
        })
        .transpose()?;
    let messages = root
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("messages must be an array")?
        .iter()
        .map(|value| {
            let message = object(value)?;
            let role = match message.get("role").and_then(Value::as_str) {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                _ => return Err("unsupported_feature: message role".into()),
            };
            Ok(Message::with_content(
                role,
                content(message.get("content"))?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if messages.is_empty() {
        return Err("messages must not be empty".into());
    }
    let (tool_choice, allowed_tools) = match root.get("tool_choice") {
        None | Some(Value::Null) => (ToolChoice::Auto, Vec::new()),
        Some(Value::Object(choice)) => match choice.get("type").and_then(Value::as_str) {
            Some("auto") => (ToolChoice::Auto, Vec::new()),
            Some("none") => (ToolChoice::None, Vec::new()),
            Some("any") => (ToolChoice::Required, Vec::new()),
            Some("tool") => (ToolChoice::Required, vec![text(choice.get("name"))?]),
            _ => return Err("invalid tool_choice".into()),
        },
        Some(Value::String(choice)) => super::openai_tool_choice_for_anthropic(choice)?,
        _ => return Err("tool_choice must be an object".into()),
    };
    let tools = root
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            let tool = object(value)?;
            Ok(Tool {
                name: text(tool.get("name"))?,
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                parameters: tool
                    .get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object","properties":{}})),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ChatRequest {
        model: path_model
            .map(str::to_owned)
            .or_else(|| root.get("model").and_then(Value::as_str).map(str::to_owned))
            .filter(|s| !s.is_empty())
            .ok_or("model is required")?,
        system,
        messages,
        max_tokens: Some(max_tokens(root.get("max_tokens"))?.ok_or("max_tokens is required")?),
        temperature: optional_number(root, "temperature")?,
        top_p: optional_number(root, "top_p")?,
        stop: stops(root.get("stop_sequences"))?,
        stream: optional_bool(root, "stream")?.unwrap_or(false),
        tools,
        tool_choice,
        allowed_tools,
        passthrough: root
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "model"
                        | "system"
                        | "messages"
                        | "max_tokens"
                        | "temperature"
                        | "top_p"
                        | "stop_sequences"
                        | "stream"
                        | "tools"
                        | "tool_choice"
                )
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    })
}
pub fn content_block_value(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text(text) => json!({"type":"text","text":text}),
        ContentBlock::Image {
            url,
            data,
            media_type,
            file_id,
        } => {
            json!({"type":"image","source":if let Some(data) = data { base64_source(data.clone(), media_type.clone()) } else if let Some(url) = url { json!({"type":"url","url":url}) } else { json!({"type":"file","file_id":file_id}) }})
        }
        ContentBlock::Audio { data, media_type } => {
            json!({"type":"document","source":base64_source(data.clone(), media_type.clone())})
        }
        ContentBlock::File {
            url,
            data,
            media_type,
            filename,
            file_id,
        } => {
            json!({"type":"document","title":filename,"source":if let Some(data) = data { base64_source(data.clone(), media_type.clone().unwrap_or_else(|| "application/octet-stream".into())) } else if let Some(url) = url { json!({"type":"url","url":url}) } else { json!({"type":"file","file_id":file_id}) }})
        }
        ContentBlock::ToolCall {
            id,
            name,
            arguments,
        } => {
            json!({"type":"tool_use","id":id,"name":name,"input":serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({"value":arguments}))})
        }
        ContentBlock::ToolResult {
            call_id,
            content,
            is_error,
            ..
        } => {
            json!({"type":"tool_result","tool_use_id":call_id,"content":content,"is_error":is_error})
        }
    }
}
pub fn encode_request(request: &ChatRequest, model: &str) -> Result<Value, String> {
    if model.is_empty() {
        return Err("upstream model is required".into());
    }
    let messages = request.messages.iter().map(|message| json!({"role":anthropic_role(message.role),"content":message.content.iter().map(content_block_value).collect::<Vec<_>>()})).collect::<Vec<_>>();
    let mut value = json!({"model":model,"messages":messages,"max_tokens":request.max_tokens.unwrap_or(1024),"stream":request.stream});
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
    if !request.tools.is_empty() {
        map.insert("tools".into(), Value::Array(request.tools.iter().map(|tool| json!({"name":tool.name,"description":tool.description,"input_schema":tool.parameters})).collect()));
    }
    if request.tool_choice != ToolChoice::Auto || !request.allowed_tools.is_empty() {
        let choice = if let Some(name) = request.allowed_tools.first() {
            json!({"type":"tool","name":name})
        } else {
            json!({"type":match request.tool_choice { ToolChoice::Auto => "auto", ToolChoice::None => "none", ToolChoice::Required => "any" }})
        };
        map.insert("tool_choice".into(), choice);
    }
    map.extend(request.passthrough.clone());
    Ok(value)
}
pub fn decode_response(value: Value) -> Result<ChatResponse, String> {
    let root = object(&value)?;
    let content = root
        .get("content")
        .and_then(Value::as_array)
        .ok_or("missing content")?
        .iter()
        .map(content_block)
        .collect::<Result<Vec<_>, _>>()?;
    let (input_tokens, output_tokens) = usage(root, "input_tokens", "output_tokens");
    let mut response = ChatResponse::from_content(
        root.get("id").and_then(Value::as_str).unwrap_or("").into(),
        root.get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        content,
        finish(root.get("stop_reason").and_then(Value::as_str)),
        input_tokens,
        output_tokens,
    );
    if let Some(usage) = root.get("usage") {
        (response.cached_tokens, response.reasoning_tokens) = usage_details(usage);
    }
    Ok(response)
}
pub fn encode_response(response: &ChatResponse) -> Value {
    json!({"id":response.id,"type":"message","role":"assistant","model":response.model,"content":response.content.iter().map(content_block_value).collect::<Vec<_>>(),"stop_reason":anthropic_finish(response.finish_reason),"usage":{"input_tokens":response.input_tokens,"output_tokens":response.output_tokens}})
}
pub fn encode_sse(events: &[StreamEvent]) -> Vec<(Option<&'static str>, Value)> {
    let usage = events.iter().rev().find_map(|event| {
        if let StreamEvent::Usage {
            input_tokens,
            output_tokens,
            ..
        } = event
        {
            Some((*input_tokens, *output_tokens))
        } else {
            None
        }
    });
    let stop = events.iter().rev().find_map(|event| {
        if let StreamEvent::Stop(reason) = event {
            Some(*reason)
        } else {
            None
        }
    });
    let mut output = Vec::new();
    let mut index = 0usize;
    let mut open = false;
    let mut text_started = true;
    let mut started = false;
    let mut stopped = false;
    fn close_block(output: &mut Vec<(Option<&'static str>, Value)>, open: &mut bool, index: usize) {
        if *open {
            output.push((
                Some("content_block_stop"),
                json!({"type":"content_block_stop","index":index}),
            ));
            *open = false;
        }
    }
    for event in events {
        match event {
            StreamEvent::Start { id, model } => {
                if started { continue; }
                started = true;
                let (input_tokens, output_tokens) = usage.unwrap_or((0, 0));
                output.push((Some("message_start"), json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens}}})));
                output.push((Some("content_block_start"), json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}})));
                text_started = true;
                open = true;
            }
            StreamEvent::Delta(text) => {
                if !text_started { output.push((Some("content_block_start"), json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}}))); text_started = true; }
                open = true;
                output.push((Some("content_block_delta"), json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":text}})));
            }
            StreamEvent::Content(ContentBlock::Text(text)) => {
                if !text_started { output.push((Some("content_block_start"), json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}}))); text_started = true; }
                open = true;
                output.push((Some("content_block_delta"), json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":text}})));
            }
            StreamEvent::Content(block) => {
                close_block(&mut output, &mut open, index);
                output.push((Some("content_block_start"), json!({"type":"content_block_start","index":index,"content_block":content_block_value(block)})));
                output.push((Some("content_block_stop"), json!({"type":"content_block_stop","index":index})));
                index += 1;
            }
            StreamEvent::ToolCallStart { index: call_index, id, name } => {
                close_block(&mut output, &mut open, index);
                index = *call_index;
                output.push((Some("content_block_start"), json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":id,"name":name,"input":{}}})));
                open = true;
            }
            StreamEvent::ToolCallDelta { index: call_index, arguments } => output.push((Some("content_block_delta"), json!({"type":"content_block_delta","index":call_index,"delta":{"type":"input_json_delta","partial_json":arguments}}))),
            StreamEvent::ToolCallEnd { index: call_index } => { close_block(&mut output, &mut open, index); index = call_index + 1; }
            StreamEvent::Usage { .. } => {}
            StreamEvent::Stop(reason) => {
                if stopped { continue; }
                if text_started && !started { open = true; }
                close_block(&mut output, &mut open, index);
                let (input_tokens, output_tokens) = usage.unwrap_or((0, 0));
                output.push((Some("message_delta"), json!({"type":"message_delta","delta":{"stop_reason":anthropic_finish(*reason)},"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens}})));
                output.push((Some("message_stop"), json!({"type":"message_stop"})));
                stopped = true;
            }
        }
    }
    if !stopped && let Some((input_tokens, output_tokens)) = usage {
        close_block(&mut output, &mut open, index);
        if let Some(reason) = stop {
            output.push((Some("message_delta"), json!({"type":"message_delta","delta":{"stop_reason":anthropic_finish(reason)},"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens}})));
            output.push((Some("message_stop"), json!({"type":"message_stop"})));
        } else {
            output.push((Some("message_delta"), json!({"type":"message_delta","delta":{},"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens}})));
        }
    }
    output
}
pub fn decode_sse(
    root: &Map<String, Value>,
    state: &mut super::AnthropicState,
) -> Result<Vec<StreamEvent>, String> {
    let mut output = Vec::new();
    match root.get("type").and_then(Value::as_str) {
        Some("message_start") => {
            let message = root
                .get("message")
                .and_then(Value::as_object)
                .ok_or("missing message")?;
            if let Some(usage) = message.get("usage") {
                state.input_tokens = usage
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                state.output_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                state.cached_tokens = usage_details(usage).0;
            }
            output.push(StreamEvent::Start {
                id: message
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                model: message
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
            });
        }
        Some("content_block_start") => {
            let index = root.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let block = object(root.get("content_block").ok_or("missing content block")?)?;
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    state.indices.insert(index, None);
                }
                Some("tool_use") => {
                    let id = text(block.get("id"))?;
                    let name = text(block.get("name"))?;
                    state
                        .indices
                        .insert(index, Some((id.clone(), name.clone())));
                    output.push(StreamEvent::ToolCallStart { index, id, name });
                }
                _ => {
                    output.push(StreamEvent::Content(content_block(&Value::Object(
                        block.clone(),
                    ))?));
                }
            }
        }
        Some("content_block_delta") => {
            let index = root.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let delta = object(root.get("delta").ok_or("missing content delta")?)?;
            match delta.get("type").and_then(Value::as_str) {
                None if delta.get("text").and_then(Value::as_str).is_some() => {
                    output.push(StreamEvent::Delta(text(delta.get("text"))?))
                }
                Some("text_delta") => output.push(StreamEvent::Delta(text(delta.get("text"))?)),
                Some("input_json_delta") => output.push(StreamEvent::ToolCallDelta {
                    index,
                    arguments: text(delta.get("partial_json"))?,
                }),
                _ => return Err("unsupported content delta".into()),
            }
        }
        Some("content_block_stop") => {
            let index = root.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if state
                .indices
                .remove(&index)
                .is_some_and(|call| call.is_some())
            {
                output.push(StreamEvent::ToolCallEnd { index });
            }
        }
        Some("message_delta") => {
            if let Some(usage) = root.get("usage") {
                state.input_tokens = usage
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(state.input_tokens);
                state.output_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(state.output_tokens);
                let cached = usage_details(usage).0;
                if cached > 0 {
                    state.cached_tokens = cached;
                }
                output.push(StreamEvent::Usage {
                    input_tokens: state.input_tokens,
                    output_tokens: state.output_tokens,
                    cached_tokens: state.cached_tokens,
                    reasoning_tokens: 0,
                });
            }
            if let Some(reason) = root
                .get("delta")
                .and_then(|delta| delta.get("stop_reason"))
                .and_then(Value::as_str)
            {
                output.push(StreamEvent::Stop(finish(Some(reason))));
                state.finished = true;
            }
        }
        Some("message_stop") => {
            if !state.finished {
                output.push(StreamEvent::Stop(FinishReason::Stop));
                state.finished = true;
            }
        }
        Some("ping") => {}
        Some("error") => return Err("upstream stream error".into()),
        _ => return Err("unsupported upstream SSE event".into()),
    }
    Ok(output)
}
