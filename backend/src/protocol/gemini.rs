use super::model::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub fn decode_request(value: Value, path_model: Option<&str>) -> Result<ChatRequest, String> {
    let root = object(&value)?;
    let generation = root
        .get("generationConfig")
        .filter(|value| !value.is_null())
        .map(object)
        .transpose()?
        .cloned()
        .unwrap_or_default();
    if optional_u64(&generation, "candidateCount")?.is_some_and(|count| count != 1) {
        return Err("unsupported_feature: multiple candidates".into());
    }
    let system = root
        .get("systemInstruction")
        .filter(|value| !value.is_null())
        .map(|value| -> Result<String, String> {
            Ok(object(value)?
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|part| text(part.get("text")))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n"))
        })
        .transpose()?;
    let messages = root
        .get("contents")
        .and_then(Value::as_array)
        .ok_or("contents must be an array")?
        .iter()
        .map(|value| {
            let message = object(value)?;
            let role = match message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user")
            {
                "user" => Role::User,
                "model" => Role::Assistant,
                _ => return Err("unsupported_feature: content role".into()),
            };
            let content = message
                .get("parts")
                .and_then(Value::as_array)
                .ok_or("parts must be an array")?
                .iter()
                .map(content_block)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Message::with_content(role, content))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if messages.is_empty() {
        return Err("contents must not be empty".into());
    }
    let mut tools = Vec::new();
    for tool in root
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for declaration in tool
            .get("functionDeclarations")
            .or_else(|| tool.get("function_declarations"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let declaration = object(declaration)?;
            tools.push(Tool {
                name: text(declaration.get("name"))?,
                description: declaration
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                parameters: declaration
                    .get("parameters")
                    .or_else(|| declaration.get("parametersJsonSchema"))
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object","properties":{}})),
            });
        }
    }
    let config = root
        .get("toolConfig")
        .or_else(|| root.get("tool_config"))
        .and_then(|config| {
            config
                .get("functionCallingConfig")
                .or_else(|| config.get("function_calling_config"))
        });
    let (tool_choice, allowed_tools) = match config
        .and_then(|config| config.get("mode"))
        .and_then(Value::as_str)
    {
        None | Some("AUTO" | "MODE_UNSPECIFIED") => (ToolChoice::Auto, Vec::new()),
        Some("NONE") => (ToolChoice::None, Vec::new()),
        Some("ANY" | "VALIDATED") => (
            ToolChoice::Required,
            config
                .and_then(|config| config.get("allowedFunctionNames"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ),
        _ => return Err("invalid function calling mode".into()),
    };
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
        tools,
        tool_choice,
        allowed_tools,
        passthrough: root
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "contents"
                        | "systemInstruction"
                        | "generationConfig"
                        | "tools"
                        | "toolConfig"
                        | "tool_config"
                )
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    })
}
pub fn content_block(value: &Value) -> Result<ContentBlock, String> {
    let part = object(value)?;
    if let Some(text) = part.get("text") {
        return Ok(ContentBlock::Text(
            text.as_str().ok_or("text must be a string")?.into(),
        ));
    }
    if let Some(data) = part.get("inlineData").or_else(|| part.get("inline_data")) {
        let data = object(data)?;
        let media_type = data
            .get("mimeType")
            .or_else(|| data.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_string();
        let data = text(data.get("data"))?;
        return Ok(if media_type.starts_with("image/") {
            ContentBlock::Image {
                url: None,
                data: Some(data),
                media_type,
                file_id: None,
            }
        } else if media_type.starts_with("audio/") {
            ContentBlock::Audio { data, media_type }
        } else {
            ContentBlock::File {
                url: None,
                data: Some(data),
                media_type: Some(media_type),
                filename: None,
                file_id: None,
            }
        });
    }
    if let Some(data) = part.get("fileData").or_else(|| part.get("file_data")) {
        let data = object(data)?;
        let url = data
            .get("fileUri")
            .or_else(|| data.get("file_uri"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let media_type = data
            .get("mimeType")
            .or_else(|| data.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_string();
        return Ok(if media_type.starts_with("image/") {
            ContentBlock::Image {
                url,
                data: None,
                media_type,
                file_id: None,
            }
        } else if media_type.starts_with("audio/") {
            ContentBlock::Audio {
                data: String::new(),
                media_type,
            }
        } else {
            ContentBlock::File {
                url,
                data: None,
                media_type: Some(media_type),
                filename: None,
                file_id: None,
            }
        });
    }
    if let Some(call) = part
        .get("functionCall")
        .or_else(|| part.get("function_call"))
    {
        let call = object(call)?;
        return Ok(ContentBlock::ToolCall {
            id: call.get("id").and_then(Value::as_str).unwrap_or("").into(),
            name: text(call.get("name"))?,
            arguments: call
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}))
                .to_string(),
        });
    }
    if let Some(result) = part
        .get("functionResponse")
        .or_else(|| part.get("function_response"))
    {
        let result = object(result)?;
        let response = result.get("response").cloned().unwrap_or_else(|| json!({}));
        let content = if response.as_object().is_some_and(|object| object.len() == 1) {
            response
                .get("result")
                .map(|result| {
                    if let Value::String(text) = result {
                        text.clone()
                    } else {
                        result.to_string()
                    }
                })
                .unwrap_or_else(|| response.to_string())
        } else {
            response.to_string()
        };
        return Ok(ContentBlock::ToolResult {
            call_id: result
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            name: result
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            content,
            is_error: false,
        });
    }
    if part.get("thought").and_then(Value::as_bool) == Some(true) {
        return Err("unsupported_feature: thinking".into());
    }
    Err("unsupported_feature: non-text content".into())
}
pub fn content_block_value(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text(text) => json!({"text":text}),
        ContentBlock::Image {
            url,
            data,
            media_type,
            ..
        } => {
            if let Some(url) = url {
                json!({"fileData":{"mimeType":media_type,"fileUri":url}})
            } else {
                json!({"inlineData":{"mimeType":media_type,"data":data}})
            }
        }
        ContentBlock::Audio { data, media_type } => {
            json!({"inlineData":{"mimeType":media_type,"data":data}})
        }
        ContentBlock::File {
            url,
            data,
            media_type,
            filename,
            ..
        } => {
            if let Some(url) = url {
                json!({"fileData":{"mimeType":media_type,"fileUri":url,"displayName":filename}})
            } else {
                json!({"inlineData":{"mimeType":media_type,"data":data,"displayName":filename}})
            }
        }
        ContentBlock::ToolCall {
            id,
            name,
            arguments,
        } => {
            json!({"functionCall":{"id":id,"name":name,"args":serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({"value":arguments}))}})
        }
        ContentBlock::ToolResult {
            call_id,
            name,
            content,
            ..
        } => {
            json!({"functionResponse":{"id":call_id,"name":name,"response":if let Ok(value) = serde_json::from_str::<Value>(content) { if value.get("result").is_some() { value } else { json!({"result":value}) } } else { json!({"result":content}) }}})
        }
    }
}
pub fn encode_request(request: &ChatRequest, model: &str) -> Result<Value, String> {
    if model.is_empty() {
        return Err("upstream model is required".into());
    }
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
    let mut value = json!({"contents":request.messages.iter().map(|message| json!({"role":gemini_role(message.role),"parts":message.content.iter().map(content_block_value).collect::<Vec<_>>()})).collect::<Vec<_>>(),"generationConfig":config});
    if let Some(text) = &request.system {
        value["systemInstruction"] = json!({"parts":[{"text":text}]});
    }
    if !request.tools.is_empty() {
        value["tools"] = json!([{"functionDeclarations":request.tools.iter().map(|tool| json!({"name":tool.name,"description":tool.description,"parameters":tool.parameters})).collect::<Vec<_>>()}]);
    }
    if request.tool_choice != ToolChoice::Auto || !request.allowed_tools.is_empty() {
        value["toolConfig"] = json!({"functionCallingConfig":{"mode":match request.tool_choice { ToolChoice::Auto => "AUTO", ToolChoice::None => "NONE", ToolChoice::Required => "ANY" },"allowedFunctionNames":request.allowed_tools}});
    }
    value
        .as_object_mut()
        .unwrap()
        .extend(request.passthrough.clone());
    Ok(value)
}
pub fn decode_response(value: Value) -> Result<ChatResponse, String> {
    let root = object(&value)?;
    let candidates = root.get("candidates").and_then(Value::as_array);
    if candidates.is_some_and(|items| items.len() > 1) {
        return Err("unsupported_feature: multiple candidates".into());
    }
    let choice = candidates
        .and_then(|items| items.first())
        .map(object)
        .transpose()?;
    if choice.is_none()
        && root.get("promptFeedback").is_none()
        && root.get("usageMetadata").is_none()
        && !root.contains_key("candidates")
    {
        return Err("missing completion candidate".into());
    }
    let content = match choice.and_then(|choice| choice.get("content")) {
        None | Some(Value::Null) => Vec::new(),
        Some(content) => object(content)?
            .get("parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(content_block)
            .collect::<Result<Vec<_>, _>>()?,
    };
    let reason = choice
        .and_then(|choice| choice.get("finishReason"))
        .and_then(Value::as_str)
        .map(|reason| finish(Some(reason)))
        .unwrap_or(if root.get("promptFeedback").is_some() {
            FinishReason::Other
        } else {
            FinishReason::Stop
        });
    let (input_tokens, output_tokens) = usage(root, "promptTokenCount", "candidatesTokenCount");
    let (cached_tokens, reasoning_tokens) = root
        .get("usageMetadata")
        .map(usage_details)
        .unwrap_or((0, 0));
    let tool_use = content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
    let mut response = ChatResponse::from_content(
        String::new(),
        String::new(),
        content,
        if tool_use {
            FinishReason::ToolUse
        } else {
            reason
        },
        input_tokens,
        output_tokens,
    );
    response.cached_tokens = cached_tokens;
    response.reasoning_tokens = reasoning_tokens;
    Ok(response)
}
pub fn encode_response(response: &ChatResponse) -> Value {
    json!({"candidates":[{"content":{"role":"model","parts":response.content.iter().map(content_block_value).collect::<Vec<_>>()},"finishReason":gemini_finish(response.finish_reason)}],"usageMetadata":{"promptTokenCount":response.input_tokens,"candidatesTokenCount":response.output_tokens,"totalTokenCount":response.input_tokens + response.output_tokens}})
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
    let mut calls: HashMap<usize, (String, String, String)> = HashMap::new();
    let mut output = Vec::new();
    for event in events {
        match event {
            StreamEvent::Start { .. } | StreamEvent::Usage { .. } | StreamEvent::Stop(_) => {}
            StreamEvent::Delta(text) => output.push((None, json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]}}]}))),
            StreamEvent::Content(block) => output.push((None, json!({"candidates":[{"content":{"role":"model","parts":[content_block_value(block)]}}]}))),
            StreamEvent::ToolCallStart { index, id, name } => { calls.insert(*index, (id.clone(), name.clone(), String::new())); }
            StreamEvent::ToolCallDelta { index, arguments } => { if let Some(call) = calls.get_mut(index) { call.2.push_str(arguments); } }
            StreamEvent::ToolCallEnd { index } => { if let Some((id, name, arguments)) = calls.remove(index) { output.push((None, json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":id,"name":name,"args":serde_json::from_str::<Value>(&arguments).unwrap_or_else(|_| json!({"value":arguments}))}}]}}]}))); } }
        }
    }
    for (_, (id, name, arguments)) in calls {
        output.push((None, json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":id,"name":name,"args":serde_json::from_str::<Value>(&arguments).unwrap_or_else(|_| json!({"value":arguments}))}}]}}]})));
    }
    if let Some(reason) = stop {
        let mut response = json!({"candidates":[{"content":{"role":"model","parts":[]},"finishReason":gemini_finish(reason)}]});
        if let Some((input_tokens, output_tokens)) = usage {
            response["usageMetadata"] = json!({"promptTokenCount":input_tokens,"candidatesTokenCount":output_tokens,"totalTokenCount":input_tokens + output_tokens});
        }
        output.push((None, response));
    } else if let Some((input_tokens, output_tokens)) = usage {
        output.push((None, json!({"usageMetadata":{"promptTokenCount":input_tokens,"candidatesTokenCount":output_tokens,"totalTokenCount":input_tokens + output_tokens}})));
    }
    output
}
pub fn decode_sse(
    value: Value,
    started: &mut bool,
    had_tool_call: &mut bool,
) -> Result<Vec<StreamEvent>, String> {
    let response = decode_response(value.clone())?;
    let mut output = Vec::new();
    if !*started {
        output.push(StreamEvent::Start {
            id: String::new(),
            model: String::new(),
        });
        *started = true;
    }
    let has_tool_call = response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
    *had_tool_call |= has_tool_call;
    let mut index = 0;
    for block in response.content {
        match block {
            ContentBlock::Text(text) => output.push(StreamEvent::Delta(text)),
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
            } => {
                output.push(StreamEvent::ToolCallStart { index, id, name });
                output.push(StreamEvent::ToolCallDelta { index, arguments });
                output.push(StreamEvent::ToolCallEnd { index });
                index += 1;
            }
            block => output.push(StreamEvent::Content(block)),
        }
    }
    if response.input_tokens > 0 || response.output_tokens > 0 {
        output.push(StreamEvent::Usage {
            input_tokens: response.input_tokens,
            output_tokens: response.output_tokens,
            cached_tokens: response.cached_tokens,
            reasoning_tokens: response.reasoning_tokens,
        });
    }
    let has_finish = value.pointer("/candidates/0/finishReason").is_some()
        || value.get("promptFeedback").is_some();
    if has_finish {
        output.push(StreamEvent::Stop(
            if *had_tool_call && response.finish_reason == FinishReason::Stop {
                FinishReason::ToolUse
            } else {
                response.finish_reason
            },
        ));
    }
    Ok(output)
}
