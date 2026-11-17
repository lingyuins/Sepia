use super::model::*;
use serde_json::{Map, Value, json};

fn content_text(value: &Value) -> Result<Vec<ContentBlock>, String> {
    match value {
        Value::String(text) => Ok(vec![ContentBlock::Text(text.clone())]),
        Value::Null => Ok(Vec::new()),
        Value::Array(parts) => parts.iter().map(content_block).collect(),
        _ => Err("content must be text or content parts".into()),
    }
}
fn content_block(value: &Value) -> Result<ContentBlock, String> {
    let part = object(value)?;
    let kind = part
        .get("type")
        .and_then(Value::as_str)
        .ok_or("content type is required")?;
    match kind {
        "text" | "input_text" | "output_text" => Ok(ContentBlock::Text(text(part.get("text"))?)),
        "image_url" | "input_image" => {
            let url =
                part.get("image_url")
                    .and_then(|image| {
                        image
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| image.get("url").and_then(Value::as_str).map(str::to_owned))
                    })
                    .or_else(|| {
                        part.get("image").and_then(|image| {
                            image.as_str().map(str::to_owned).or_else(|| {
                                image.get("url").and_then(Value::as_str).map(str::to_owned)
                            })
                        })
                    });
            let url = url.or_else(|| {
                part.get("image_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
            let (data, media_type) = url
                .as_deref()
                .and_then(data_url)
                .map(|(media_type, data)| (Some(data), media_type))
                .unwrap_or((None, "image/*".into()));
            let url = if data.is_some() { None } else { url };
            Ok(ContentBlock::Image {
                url,
                data,
                media_type,
                file_id: part
                    .get("file_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        "image" => {
            let image = object(part.get("image").ok_or("image is required")?)?;
            let url = image.get("url").and_then(Value::as_str).map(str::to_owned);
            let (data, media_type) = url
                .as_deref()
                .and_then(data_url)
                .map(|(media_type, data)| (Some(data), media_type))
                .unwrap_or((None, "image/*".into()));
            let url = if data.is_some() { None } else { url };
            Ok(ContentBlock::Image {
                url,
                data,
                media_type,
                file_id: image
                    .get("file_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        "input_audio" | "audio" => {
            let audio = object(
                part.get("input_audio")
                    .or_else(|| part.get("audio"))
                    .ok_or("audio is required")?,
            )?;
            let format = audio.get("format").and_then(Value::as_str).unwrap_or("wav");
            Ok(ContentBlock::Audio {
                data: text(audio.get("data"))?,
                media_type: audio
                    .get("media_type")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("audio/{format}")),
            })
        }
        "file" | "input_file" => {
            let file = part.get("file").and_then(Value::as_object);
            let get = |name: &str| {
                file.and_then(|file| file.get(name))
                    .or_else(|| part.get(name))
            };
            let url = get("file_url").and_then(Value::as_str).map(str::to_owned);
            let data = get("file_data")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| url.as_deref().and_then(data_url).map(|(_, data)| data));
            Ok(ContentBlock::File {
                url,
                data,
                media_type: get("media_type").and_then(Value::as_str).map(str::to_owned),
                filename: get("filename").and_then(Value::as_str).map(str::to_owned),
                file_id: get("file_id").and_then(Value::as_str).map(str::to_owned),
            })
        }
        _ => Err(format!("unsupported_feature: content type {kind}")),
    }
}
fn tool_call(value: &Value) -> Result<ContentBlock, String> {
    let call = object(value)?;
    let function = object(
        call.get("function")
            .or_else(|| call.get("function_call"))
            .ok_or("function call is required")?,
    )?;
    Ok(ContentBlock::ToolCall {
        id: call.get("id").and_then(Value::as_str).unwrap_or("").into(),
        name: text(function.get("name"))?,
        arguments: function
            .get("arguments")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                function
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}))
                    .to_string()
            }),
    })
}
fn tool(value: &Value) -> Result<Tool, String> {
    let tool = object(value)?;
    let function = match tool.get("function") {
        Some(value) => object(value)?,
        None => tool,
    };
    Ok(Tool {
        name: text(function.get("name"))?,
        description: function
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        parameters: function
            .get("parameters")
            .or_else(|| function.get("input_schema"))
            .cloned()
            .unwrap_or_else(|| json!({"type":"object","properties":{}})),
    })
}
fn tool_choice(value: Option<&Value>) -> Result<(ToolChoice, Vec<String>), String> {
    match value {
        None | Some(Value::Null) => Ok((ToolChoice::Auto, Vec::new())),
        Some(Value::String(s)) if s == "auto" => Ok((ToolChoice::Auto, Vec::new())),
        Some(Value::String(s)) if s == "none" => Ok((ToolChoice::None, Vec::new())),
        Some(Value::String(s)) if matches!(s.as_str(), "required" | "any") => {
            Ok((ToolChoice::Required, Vec::new()))
        }
        Some(Value::Object(choice)) => {
            let kind = choice.get("type").and_then(Value::as_str);
            if matches!(kind, Some("auto" | "none" | "any" | "required")) {
                return tool_choice(kind.map(|kind| Value::String(kind.into())).as_ref());
            }
            let name = choice
                .get("function")
                .and_then(|function| function.get("name"))
                .or_else(|| choice.get("name"))
                .and_then(Value::as_str)
                .ok_or("tool_choice function name is required")?;
            Ok((ToolChoice::Required, vec![name.into()]))
        }
        _ => Err("tool_choice must be auto, none, required, or a function".into()),
    }
}
pub fn decode_request(value: Value, path_model: Option<&str>) -> Result<ChatRequest, String> {
    let root = object(&value)?;
    if optional_u64(root, "n")?.is_some_and(|count| count != 1) {
        return Err("unsupported_feature: multiple candidates".into());
    }
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
            Some("tool") => Role::Tool,
            _ => return Err("unsupported_feature: message role".into()),
        };
        let mut content = content_text(message.get("content").unwrap_or(&Value::Null))?;
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            content.push(tool_call(call)?);
        }
        if let Some(call) = message
            .get("function_call")
            .filter(|value| !value.is_null())
        {
            content.push(tool_call(call)?);
        }
        if role == Role::Tool {
            content = vec![ContentBlock::ToolResult {
                call_id: message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .ok_or("tool_call_id is required")?
                    .into(),
                name: message
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                content: content.iter().filter_map(ContentBlock::as_text).collect(),
                is_error: false,
            }];
        }
        if role == Role::System {
            let value = content
                .iter()
                .filter_map(ContentBlock::as_text)
                .collect::<Vec<_>>()
                .join("\n");
            system = Some(match system {
                Some(previous) => format!("{previous}\n{value}"),
                None => value,
            });
        } else {
            messages.push(Message::with_content(role, content));
        }
    }
    if messages.is_empty() {
        return Err("messages must include user or assistant input".into());
    }
    let (tool_choice, allowed_tools) = tool_choice(root.get("tool_choice"))?;
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
        tools: root
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(tool)
            .collect::<Result<_, _>>()?,
        tool_choice,
        allowed_tools,
        passthrough: root
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "model"
                        | "messages"
                        | "max_tokens"
                        | "max_completion_tokens"
                        | "temperature"
                        | "top_p"
                        | "stop"
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
            json!({"type":"image_url","image_url":{ "url":media_url(url.clone(), data.clone(), media_type), "file_id":file_id }})
        }
        ContentBlock::Audio { data, media_type } => {
            json!({"type":"input_audio","input_audio":{"data":data,"format":media_type.strip_prefix("audio/").unwrap_or(media_type),"media_type":media_type}})
        }
        ContentBlock::File {
            url,
            data,
            media_type,
            filename,
            file_id,
        } => {
            json!({"type":"file","file":{"file_url":url,"file_data":data,"media_type":media_type,"filename":filename,"file_id":file_id}})
        }
        ContentBlock::ToolCall {
            id,
            name,
            arguments,
        } => json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments}}),
        ContentBlock::ToolResult { content, .. } => json!({"type":"text","text":content}),
    }
}
pub fn encode_request(request: &ChatRequest, model: &str) -> Result<Value, String> {
    if model.is_empty() {
        return Err("upstream model is required".into());
    }
    let mut messages = request
        .system
        .iter()
        .map(|text| json!({"role":"system","content":text}))
        .collect::<Vec<_>>();
    messages.extend(request.messages.iter().flat_map(|message| {
        if message.role == Role::Tool || message.content.iter().any(|block| matches!(block, ContentBlock::ToolResult { .. })) {
            message.content.iter().filter_map(|block| if let ContentBlock::ToolResult { call_id, content, .. } = block { Some(json!({"role":"tool","tool_call_id":call_id,"content":content})) } else { None }).collect::<Vec<_>>()
        } else {
        let mut value = json!({"role":openai_role(message.role),"content":if message.content.iter().all(|block| matches!(block, ContentBlock::Text(_))) { Value::String(message.text.clone()) } else { Value::Array(message.content.iter().filter(|block| !matches!(block, ContentBlock::ToolCall{..}|ContentBlock::ToolResult{..})).map(content_block_value).collect()) }});
        let calls: Vec<_> = message.content.iter().filter_map(|block| if let ContentBlock::ToolCall { .. } = block { Some(content_block_value(block)) } else { None }).collect();
        if !calls.is_empty() { value["tool_calls"] = Value::Array(calls); if message.text.is_empty() { value["content"] = Value::Null; } }
        vec![value]
        }
    }));
    let mut value = json!({"model":model,"messages":messages,"stream":request.stream});
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
    if !request.tools.is_empty() {
        map.insert("tools".into(), Value::Array(request.tools.iter().map(|tool| json!({"type":"function","function":{"name":tool.name,"description":tool.description,"parameters":tool.parameters}})).collect()));
    }
    if request.tool_choice != ToolChoice::Auto || !request.allowed_tools.is_empty() {
        let choice = if request.allowed_tools.len() == 1 {
            json!({"type":"function","function":{"name":request.allowed_tools[0]}})
        } else {
            json!(match request.tool_choice {
                ToolChoice::Auto => "auto",
                ToolChoice::None => "none",
                ToolChoice::Required => "required",
            })
        };
        map.insert("tool_choice".into(), choice);
    }
    map.extend(request.passthrough.clone());
    Ok(value)
}

pub fn decode_response(value: Value) -> Result<ChatResponse, String> {
    let root = object(&value)?;
    let choices = root
        .get("choices")
        .and_then(Value::as_array)
        .ok_or("missing completion choice")?;
    if choices.len() != 1 {
        return Err("unsupported_feature: multiple choices".into());
    }
    let message = object(
        choices[0]
            .get("message")
            .ok_or("missing completion message")?,
    )?;
    let mut content = content_text(message.get("content").unwrap_or(&Value::Null))?;
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        content.push(tool_call(call)?);
    }
    if let Some(call) = message
        .get("function_call")
        .filter(|value| !value.is_null())
    {
        content.push(tool_call(call)?);
    }
    let (input_tokens, output_tokens) = usage(root, "prompt_tokens", "completion_tokens");
    let mut response = ChatResponse::from_content(
        root.get("id").and_then(Value::as_str).unwrap_or("").into(),
        root.get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        content,
        finish(choices[0].get("finish_reason").and_then(Value::as_str)),
        input_tokens,
        output_tokens,
    );
    if let Some(usage) = root.get("usage") {
        (response.cached_tokens, response.reasoning_tokens) = usage_details(usage);
    }
    Ok(response)
}
pub fn encode_response(response: &ChatResponse) -> Value {
    let calls: Vec<_> = response
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::ToolCall { .. } = block {
                Some(content_block_value(block))
            } else {
                None
            }
        })
        .collect();
    let media: Vec<_> = response
        .content
        .iter()
        .filter(|block| !matches!(block, ContentBlock::Text(_) | ContentBlock::ToolCall { .. }))
        .map(content_block_value)
        .collect();
    let mut message = json!({"role":"assistant","content":if media.is_empty() { Value::String(response.text.clone()) } else { Value::Array(media) }});
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(calls);
        if response.text.is_empty() {
            message["content"] = Value::Null;
        }
    }
    json!({"id":response.id,"object":"chat.completion","created":0,"model":response.model,"choices":[{"index":0,"message":message,"finish_reason":openai_finish(response.finish_reason)}],"usage":{"prompt_tokens":response.input_tokens,"completion_tokens":response.output_tokens,"total_tokens":response.input_tokens + response.output_tokens}})
}
pub fn encode_sse(events: &[StreamEvent]) -> Vec<(Option<&'static str>, Value)> {
    events.iter().flat_map(|event| {
        let chunk = |delta, finish_reason| json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[{"index":0,"delta":delta,"finish_reason":finish_reason}]});
        match event {
            StreamEvent::Start { id, model } => vec![(None, json!({"id":id,"object":"chat.completion.chunk","created":0,"model":model,"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}))],
            StreamEvent::Delta(text) => vec![(None, chunk(json!({"content":text}), Value::Null))],
            StreamEvent::Content(block) => match block {
                ContentBlock::Text(text) => vec![(None, chunk(json!({"content":text}), Value::Null))],
                ContentBlock::ToolCall { id, name, arguments } => vec![(None, chunk(json!({"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments}}]}), Value::Null))],
                _ => vec![(None, chunk(json!({"content":[content_block_value(block)]}), Value::Null))],
            },
            StreamEvent::ToolCallStart { index, id, name } => vec![(None, chunk(json!({"tool_calls":[{"index":index,"id":id,"type":"function","function":{"name":name,"arguments":""}}]}), Value::Null))],
            StreamEvent::ToolCallDelta { index, arguments } => vec![(None, chunk(json!({"tool_calls":[{"index":index,"function":{"arguments":arguments}}]}), Value::Null))],
            StreamEvent::ToolCallEnd { .. } => Vec::new(),
            StreamEvent::Usage { input_tokens, output_tokens, cached_tokens, reasoning_tokens } => vec![(None, json!({"id":"","object":"chat.completion.chunk","created":0,"model":"","choices":[],"usage":{"prompt_tokens":input_tokens,"completion_tokens":output_tokens,"prompt_tokens_details":{"cached_tokens":cached_tokens},"completion_tokens_details":{"reasoning_tokens":reasoning_tokens}}}))],
            StreamEvent::Stop(reason) => vec![(None, chunk(json!({}), json!(openai_finish(*reason))))],
        }
    }).collect()
}
pub fn decode_sse(root: &Map<String, Value>) -> Result<Vec<StreamEvent>, String> {
    let mut output = Vec::new();
    if let Some(usage) = root.get("usage").filter(|usage| usage.is_object()) {
        let prompt = usage.get("prompt_tokens").and_then(Value::as_u64);
        let completion = usage.get("completion_tokens").and_then(Value::as_u64);
        if prompt.is_some() || completion.is_some() {
            let (cached_tokens, reasoning_tokens) = usage_details(usage);
            output.push(StreamEvent::Usage {
                input_tokens: prompt.unwrap_or(0),
                output_tokens: completion.unwrap_or(0),
                cached_tokens,
                reasoning_tokens,
            });
        }
    }
    let choices = root.get("choices").and_then(Value::as_array);
    if choices.is_some_and(|choices| choices.len() > 1) {
        return Err("unsupported_feature: multiple choices".into());
    }
    if let Some(choice) = choices.and_then(|choices| choices.first()) {
        let choice = object(choice)?;
        if let Some(delta) = choice.get("delta").filter(|value| value.is_object()) {
            if delta.get("role").and_then(Value::as_str) == Some("assistant") {
                output.push(StreamEvent::Start {
                    id: root.get("id").and_then(Value::as_str).unwrap_or("").into(),
                    model: root
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                });
            }
            if let Some(text) = delta
                .get("content")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                output.push(StreamEvent::Delta(text.into()));
            }
            for call in delta
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let call = object(call)?;
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let function = call
                    .get("function")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if call.get("id").and_then(Value::as_str).is_some()
                    || function.get("name").and_then(Value::as_str).is_some()
                {
                    output.push(StreamEvent::ToolCallStart {
                        index,
                        id: call.get("id").and_then(Value::as_str).unwrap_or("").into(),
                        name: function
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into(),
                    });
                }
                if let Some(arguments) = function
                    .get("arguments")
                    .and_then(Value::as_str)
                    .filter(|arguments| !arguments.is_empty())
                {
                    output.push(StreamEvent::ToolCallDelta {
                        index,
                        arguments: arguments.into(),
                    });
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            output.push(StreamEvent::Stop(finish(Some(reason))));
        }
    }
    Ok(output)
}
