use super::model::*;
use serde_json::{Map, Value, json};

// Outbound-only codec: converts the shared ChatRequest/ChatResponse model to and
// from the OpenAI Responses wire format. Inbound Responses requests are handled
// by `crate::responses`, which normalizes them into OpenAI chat JSON first.

fn input_part(block: &ContentBlock) -> Result<Value, String> {
    match block {
        ContentBlock::Text(text) => Ok(json!({"type":"input_text","text":text})),
        ContentBlock::Image {
            url,
            data,
            media_type,
            file_id,
        } => {
            let mut part = json!({"type":"input_image"});
            let map = part.as_object_mut().unwrap();
            if let Some(file_id) = file_id {
                map.insert("file_id".into(), json!(file_id));
            } else {
                map.insert(
                    "image_url".into(),
                    json!(media_url(url.clone(), data.clone(), media_type)),
                );
            }
            Ok(part)
        }
        ContentBlock::Audio { data, media_type } => Ok(json!({
            "type":"input_audio",
            "input_audio":{"data":data,"format":media_type.strip_prefix("audio/").unwrap_or(media_type)}
        })),
        ContentBlock::File {
            url,
            data,
            media_type,
            filename,
            file_id,
        } => {
            let mut part = json!({"type":"input_file"});
            let map = part.as_object_mut().unwrap();
            if let Some(file_id) = file_id {
                map.insert("file_id".into(), json!(file_id));
            }
            if let Some(url) = url {
                map.insert("file_url".into(), json!(url));
            }
            if let Some(data) = data {
                let encoded = match media_type {
                    Some(media_type) => format!("data:{media_type};base64,{data}"),
                    None => data.clone(),
                };
                map.insert("file_data".into(), json!(encoded));
            }
            if let Some(filename) = filename {
                map.insert("filename".into(), json!(filename));
            }
            Ok(part)
        }
        ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
            Err("tool items are encoded as standalone input items".into())
        }
    }
}

fn output_part(block: &ContentBlock) -> Result<Value, String> {
    match block {
        ContentBlock::Text(text) => Ok(json!({"type":"output_text","text":text})),
        _ => Err("unsupported_feature: assistant media output".into()),
    }
}

fn message_item(role: Role, parts: Vec<Value>) -> Value {
    json!({"type":"message","role":openai_role(role),"content":parts})
}

pub fn encode_request(request: &ChatRequest, model: &str) -> Result<Value, String> {
    if model.is_empty() {
        return Err("upstream model is required".into());
    }
    if !request.stop.is_empty() {
        return Err("unsupported_feature: stop sequences".into());
    }
    let mut input: Vec<Value> = Vec::new();
    let mut pending_text: Option<(Role, Vec<Value>)> = None;
    let flush = |pending: &mut Option<(Role, Vec<Value>)>, input: &mut Vec<Value>| {
        if let Some((role, parts)) = pending.take()
            && !parts.is_empty()
        {
            input.push(message_item(role, parts));
        }
    };
    for message in &request.messages {
        for block in &message.content {
            match block {
                ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    flush(&mut pending_text, &mut input);
                    input.push(json!({"type":"function_call","call_id":id,"name":name,"arguments":arguments}));
                }
                ContentBlock::ToolResult {
                    call_id, content, ..
                } => {
                    flush(&mut pending_text, &mut input);
                    input.push(
                        json!({"type":"function_call_output","call_id":call_id,"output":content}),
                    );
                }
                _ => {
                    let role = if message.role == Role::Tool {
                        Role::User
                    } else {
                        message.role
                    };
                    let part = if role == Role::Assistant {
                        output_part(block)?
                    } else {
                        input_part(block)?
                    };
                    match &mut pending_text {
                        Some((pending_role, pending_parts)) if *pending_role == role => {
                            pending_parts.push(part);
                        }
                        _ => {
                            flush(&mut pending_text, &mut input);
                            pending_text = Some((role, vec![part]));
                        }
                    }
                }
            }
        }
    }
    flush(&mut pending_text, &mut input);
    let mut value = json!({"model":model,"input":input,"stream":request.stream,"store":false});
    let map = value.as_object_mut().unwrap();
    if let Some(system) = &request.system {
        map.insert("instructions".into(), json!(system));
    }
    if let Some(n) = request.max_tokens {
        map.insert("max_output_tokens".into(), n.into());
    }
    if let Some(n) = request.temperature {
        map.insert("temperature".into(), n.into());
    }
    if let Some(n) = request.top_p {
        map.insert("top_p".into(), n.into());
    }
    if !request.tools.is_empty() {
        map.insert(
            "tools".into(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({"type":"function","name":tool.name,"description":tool.description,"parameters":tool.parameters})
                    })
                    .collect(),
            ),
        );
    }
    if request.tool_choice != ToolChoice::Auto || !request.allowed_tools.is_empty() {
        let choice = if request.allowed_tools.len() == 1 {
            json!({"type":"function","name":request.allowed_tools[0]})
        } else {
            json!(match request.tool_choice {
                ToolChoice::Auto => "auto",
                ToolChoice::None => "none",
                ToolChoice::Required => "required",
            })
        };
        map.insert("tool_choice".into(), choice);
    }
    for (key, value) in &request.passthrough {
        if matches!(key.as_str(), "messages" | "max_tokens" | "stream_options") {
            continue;
        }
        map.insert(key.clone(), value.clone());
    }
    Ok(value)
}

fn output_items(root: &Map<String, Value>) -> Result<Vec<ContentBlock>, String> {
    let mut content = Vec::new();
    for item in root
        .get("output")
        .and_then(Value::as_array)
        .ok_or("missing response output")?
    {
        let item = object(item)?;
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let part = object(part)?;
                    match part.get("type").and_then(Value::as_str) {
                        Some("output_text") => {
                            content.push(ContentBlock::Text(text(part.get("text"))?));
                        }
                        Some("refusal") => {
                            content.push(ContentBlock::Text(text(part.get("refusal"))?));
                        }
                        kind => {
                            return Err(format!(
                                "unsupported_feature: output content {}",
                                kind.unwrap_or("unknown")
                            ));
                        }
                    }
                }
            }
            Some("function_call") => {
                content.push(ContentBlock::ToolCall {
                    id: text(item.get("call_id"))?,
                    name: text(item.get("name"))?,
                    arguments: item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                });
            }
            // Reasoning summaries are not part of the shared content model.
            Some("reasoning") => {}
            kind => {
                return Err(format!(
                    "unsupported_feature: output item {}",
                    kind.unwrap_or("unknown")
                ));
            }
        }
    }
    Ok(content)
}

fn response_finish(root: &Map<String, Value>, has_tool_call: bool) -> FinishReason {
    match root.get("status").and_then(Value::as_str) {
        Some("incomplete") => match root
            .get("incomplete_details")
            .and_then(|details| details.get("reason"))
            .and_then(Value::as_str)
        {
            Some("max_output_tokens") => FinishReason::Length,
            Some("content_filter") => FinishReason::ContentFilter,
            _ => FinishReason::Other,
        },
        Some("failed") | Some("cancelled") => FinishReason::Other,
        _ if has_tool_call => FinishReason::ToolUse,
        _ => FinishReason::Stop,
    }
}

pub fn decode_response(value: Value) -> Result<ChatResponse, String> {
    let root = object(&value)?;
    if matches!(
        root.get("status").and_then(Value::as_str),
        Some("queued") | Some("in_progress")
    ) {
        return Err("upstream response is not complete".into());
    }
    let content = output_items(root)?;
    let has_tool_call = content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
    let (input_tokens, output_tokens) = usage(root, "input_tokens", "output_tokens");
    Ok(ChatResponse::from_content(
        root.get("id").and_then(Value::as_str).unwrap_or("").into(),
        root.get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        content,
        response_finish(root, has_tool_call),
        input_tokens,
        output_tokens,
    ))
}

fn call_index(state: &mut super::ResponsesState, output_index: usize) -> usize {
    if let Some(index) = state.calls.get(&output_index) {
        return *index;
    }
    let index = state.next_call;
    state.next_call += 1;
    state.calls.insert(output_index, index);
    index
}

fn terminal(
    state: &mut super::ResponsesState,
    response: &Value,
    reason: FinishReason,
) -> Result<Vec<StreamEvent>, String> {
    let mut output = Vec::new();
    if state.finished {
        return Ok(output);
    }
    state.finished = true;
    if let Some(usage) = response.get("usage").filter(|usage| usage.is_object()) {
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
    output.push(StreamEvent::Stop(reason));
    Ok(output)
}

pub fn decode_sse(
    root: &Map<String, Value>,
    state: &mut super::ResponsesState,
) -> Result<Vec<StreamEvent>, String> {
    let mut output = Vec::new();
    let output_index = root
        .get("output_index")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    match root.get("type").and_then(Value::as_str) {
        Some("response.created") | Some("response.in_progress") => {
            let response = root.get("response").ok_or("missing response")?;
            output.push(StreamEvent::Start {
                id: response
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                model: response
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
            });
        }
        Some("response.output_item.added") | Some("response.output_item.done") => {
            let item = object(root.get("item").ok_or("missing output item")?)?;
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                let added =
                    root.get("type").and_then(Value::as_str) == Some("response.output_item.added");
                let index = call_index(state, output_index);
                if added {
                    output.push(StreamEvent::ToolCallStart {
                        index,
                        id: item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into(),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into(),
                    });
                    if let Some(arguments) = state.pending_arguments.remove(&output_index)
                        && !arguments.is_empty()
                    {
                        output.push(StreamEvent::ToolCallDelta { index, arguments });
                    }
                } else {
                    // Minimal upstreams may only emit the final item; recover the
                    // full call unless argument deltas already delivered it.
                    let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or("");
                    if !state.calls.contains_key(&output_index) && !arguments.is_empty() {
                        let index = call_index(state, output_index);
                        output.push(StreamEvent::ToolCallStart {
                            index,
                            id: item
                                .get("call_id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                            name: item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                        });
                        output.push(StreamEvent::ToolCallDelta {
                            index,
                            arguments: arguments.into(),
                        });
                    }
                    state.pending_arguments.remove(&output_index);
                    output.push(StreamEvent::ToolCallEnd { index });
                }
            }
        }
        Some("response.output_text.delta") => {
            let delta = text(root.get("delta"))?;
            if !delta.is_empty() {
                state.text_delta_seen.insert(output_index);
                output.push(StreamEvent::Delta(delta));
            }
        }
        Some("response.output_text.done") => {
            if !state.text_delta_seen.remove(&output_index)
                && let Some(full) = root.get("text").and_then(Value::as_str)
                && !full.is_empty()
            {
                output.push(StreamEvent::Delta(full.into()));
            }
        }
        Some("response.function_call_arguments.delta") => {
            let arguments = text(root.get("delta"))?;
            if !arguments.is_empty() {
                if state.calls.contains_key(&output_index) {
                    output.push(StreamEvent::ToolCallDelta {
                        index: call_index(state, output_index),
                        arguments,
                    });
                } else {
                    state
                        .pending_arguments
                        .entry(output_index)
                        .or_default()
                        .push_str(&arguments);
                }
            }
        }
        Some("response.function_call_arguments.done") => {
            if !state.calls.contains_key(&output_index)
                && let Some(arguments) = root.get("arguments").and_then(Value::as_str)
                && !arguments.is_empty()
            {
                state
                    .pending_arguments
                    .entry(output_index)
                    .or_default()
                    .push_str(arguments);
            }
        }
        Some("response.completed") => {
            let response = root.get("response").ok_or("missing response")?;
            let has_tool_call = !state.calls.is_empty()
                || response
                    .get("output")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("function_call")
                        })
                    });
            let reason = response_finish(object(response)?, has_tool_call);
            output.extend(terminal(state, response, reason)?);
        }
        Some("response.incomplete") => {
            let response = root.get("response").ok_or("missing response")?;
            let reason = response_finish(object(response)?, false);
            output.extend(terminal(state, response, reason)?);
        }
        Some("response.failed") | Some("error") => return Err("upstream stream error".into()),
        _ => {}
    }
    Ok(output)
}
