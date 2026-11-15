use crate::protocol::{
    ChatRequest, ChatResponse, ContentBlock, FinishReason, Protocol, StreamEvent,
    decode_request as decode_chat_request,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct ResponseContext {
    pub previous_response_id: Option<String>,
    pub conversation: Option<String>,
    pub background: bool,
    pub store: bool,
    pub input_items: Vec<Value>,
    pub instructions: Option<String>,
    pub tools: Vec<Value>,
    pub original: Value,
}

pub fn decode_request_with_context(value: Value) -> Result<(ChatRequest, ResponseContext), String> {
    let root = value
        .as_object()
        .ok_or_else(|| "request must be a JSON object".to_string())?;
    let previous_response_id = root
        .get("previous_response_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let conversation = root
        .get("conversation")
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("id").and_then(Value::as_str))
        })
        .map(str::to_owned);
    if previous_response_id.is_some() && conversation.is_some() {
        return Err("previous_response_id and conversation are mutually exclusive".into());
    }
    if previous_response_id.is_some() && root.get("instructions").is_some() {
        return Err("instructions cannot be inherited through previous_response_id".into());
    }
    let background = root
        .get("background")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let store = root.get("store").and_then(Value::as_bool).unwrap_or(true);
    if background && root.get("stream").and_then(Value::as_bool) == Some(true) {
        return Err("background responses use polling instead of create streaming".into());
    }
    if root.get("input").is_none() && previous_response_id.is_none() && conversation.is_none() {
        return Err("input is required".into());
    }
    let instructions = root
        .get("instructions")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or("instructions must be a string")
        })
        .transpose()?;
    let mut messages = Vec::new();
    if let Some(instructions) = &instructions {
        messages.push(json!({"role":"system","content":instructions}));
    }
    let mut input_items = Vec::new();
    match root.get("input").unwrap_or(&Value::Array(Vec::new())) {
        Value::String(input) => {
            messages.push(json!({"role":"user","content":input}));
            input_items.push(json!({"type":"message","role":"user","content":[{"type":"input_text","text":input}]}));
        }
        Value::Array(items) => {
            for value in items {
                let item = value.as_object().ok_or("input items must be objects")?;
                let kind = item
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("message");
                match kind {
                    "message" => {
                        let role = item
                            .get("role")
                            .and_then(Value::as_str)
                            .ok_or("input message role is required")?;
                        if !matches!(role, "user" | "assistant" | "system" | "developer") {
                            return Err("unsupported_feature: message role".into());
                        }
                        let content = match item.get("content") {
                            Some(Value::String(content)) => Value::String(content.clone()),
                            Some(Value::Array(parts)) => Value::Array(parts.iter().map(|part| {
                                let part = part.as_object().ok_or("input content parts must be objects")?;
                                match part.get("type").and_then(Value::as_str) {
                                    Some("input_text" | "output_text") => Ok(json!({"type":"text","text":part.get("text")})),
                                    Some("input_image") => Ok(json!({"type":"image_url","image_url":{"url":part.get("image_url"),"detail":part.get("detail")},"file_id":part.get("file_id")})),
                                    Some("input_file") => Ok(json!({"type":"file","file":{"file_id":part.get("file_id"),"file_data":part.get("file_data"),"file_url":part.get("file_url"),"filename":part.get("filename")}})),
                                    Some("input_audio") => Ok(json!({"type":"input_audio","input_audio":part.get("input_audio")})),
                                    kind => Err(format!("unsupported_feature: content type {}", kind.unwrap_or("unknown"))),
                                }
                            }).collect::<Result<Vec<_>, String>>()?),
                            _ => return Err("message content is required".into()),
                        };
                        let message = json!({"role":role,"content":content});
                        messages.push(message);
                    }
                    "function_call" => {
                        let call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .ok_or("function_call call_id is required")?;
                        messages.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":call_id,"type":"function","function":{"name":item.get("name"),"arguments":item.get("arguments")}}]}));
                    }
                    "function_call_output" => {
                        let call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .ok_or("function_call_output call_id is required")?;
                        let output = match item.get("output") {
                            Some(Value::String(output)) => output.clone(),
                            Some(Value::Array(parts)) => parts
                                .iter()
                                .filter_map(|part| part.get("text"))
                                .filter_map(Value::as_str)
                                .collect(),
                            Some(value) => value.to_string(),
                            None => return Err("function_call_output output is required".into()),
                        };
                        messages
                            .push(json!({"role":"tool","tool_call_id":call_id,"content":output}));
                    }
                    "item_reference" => {
                        return Err("item_reference must be resolved before decoding".into());
                    }
                    _ => return Err(format!("unsupported_feature: input item {kind}")),
                }
                input_items.push(value.clone());
            }
        }
        _ => return Err("input must be a string or item array".into()),
    }
    let mut chat = Map::new();
    chat.insert(
        "model".into(),
        root.get("model").cloned().unwrap_or(Value::Null),
    );
    chat.insert("messages".into(), Value::Array(messages));
    for name in [
        "temperature",
        "top_p",
        "stream",
        "tools",
        "parallel_tool_calls",
    ] {
        if let Some(value) = root.get(name) {
            chat.insert(name.into(), value.clone());
        }
    }
    if let Some(tools) = root.get("tools").and_then(Value::as_array) {
        let converted = tools.iter().map(|tool| {
            let tool = tool.as_object().ok_or("tools must be objects")?;
            if tool.get("type").and_then(Value::as_str) != Some("function") { return Err("only function tools are supported by chat providers".into()); }
            Ok(json!({"type":"function","function":{"name":tool.get("name"),"description":tool.get("description"),"parameters":tool.get("parameters"),"strict":tool.get("strict")}}))
        }).collect::<Result<Vec<_>, String>>()?;
        chat.insert("tools".into(), Value::Array(converted));
    }
    if let Some(choice) = root.get("tool_choice") {
        chat.insert(
            "tool_choice".into(),
            if choice.get("type").and_then(Value::as_str) == Some("function") {
                json!({"type":"function","function":{"name":choice.get("name")}})
            } else {
                choice.clone()
            },
        );
    }
    chat.insert(
        "max_tokens".into(),
        root.get("max_output_tokens")
            .cloned()
            .unwrap_or(Value::Null),
    );
    let request = decode_chat_request(Protocol::OpenAI, Value::Object(chat), None)?;
    let tools = root
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((
        request,
        ResponseContext {
            previous_response_id,
            conversation,
            background,
            store,
            input_items,
            instructions,
            tools,
            original: value,
        },
    ))
}

pub fn decode_request(value: Value) -> Result<ChatRequest, String> {
    decode_request_with_context(value).map(|(request, _)| request)
}

pub fn output_items(response: &ChatResponse) -> Vec<Value> {
    let mut output = Vec::new();
    let text = response
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text(text) = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<String>();
    if !text.is_empty() || response.content.is_empty() {
        output.push(json!({"id":format!("msg_{}",response.id),"type":"message","status":if response.finish_reason == FinishReason::Stop || response.finish_reason == FinishReason::ToolUse { "completed" } else { "incomplete" },"role":"assistant","content":[{"type":"output_text","text":text,"annotations":[],"logprobs":[]}]}));
    }
    for (index, block) in response
        .content
        .iter()
        .filter(|block| matches!(block, ContentBlock::ToolCall { .. }))
        .enumerate()
    {
        if let ContentBlock::ToolCall {
            id,
            name,
            arguments,
        } = block
        {
            output.push(json!({"id":format!("fc_{}_{}",response.id,index),"type":"function_call","status":"completed","call_id":id,"name":name,"arguments":arguments}));
        }
    }
    output
}

pub fn response_object(
    response: &ChatResponse,
    context: Option<&ResponseContext>,
    in_progress: bool,
) -> Value {
    let reason = response.finish_reason;
    let status = if in_progress {
        "in_progress"
    } else {
        match reason {
            FinishReason::Stop | FinishReason::ToolUse => "completed",
            FinishReason::Length | FinishReason::ContentFilter => "incomplete",
            FinishReason::Other => "failed",
        }
    };
    let original = context
        .and_then(|context| context.original.as_object())
        .cloned()
        .unwrap_or_default();
    json!({
        "id":format!("resp_{}",response.id),"object":"response","created_at":original.get("created_at").cloned().unwrap_or(json!(0)),"completed_at":if in_progress { Value::Null } else { json!(0) },"status":status,
        "error":if reason == FinishReason::Other && !in_progress { json!({"code":"server_error","message":"Upstream response failed"}) } else { Value::Null },
        "incomplete_details":if matches!(reason, FinishReason::Length | FinishReason::ContentFilter) && !in_progress { json!({"reason":if reason == FinishReason::Length { "max_output_tokens" } else { "content_filter" }}) } else { Value::Null },
        "instructions":context.and_then(|context| context.instructions.as_ref()).map(|value| json!(value)).unwrap_or(Value::Null),"max_output_tokens":original.get("max_output_tokens").cloned().unwrap_or(Value::Null),"max_tool_calls":original.get("max_tool_calls").cloned().unwrap_or(Value::Null),
        "model":response.model,"output":if in_progress { json!([]) } else { json!(output_items(response)) },"parallel_tool_calls":original.get("parallel_tool_calls").cloned().unwrap_or(json!(true)),"previous_response_id":context.and_then(|context| context.previous_response_id.as_ref()).map(|value| json!(value)).unwrap_or(Value::Null),"conversation":context.and_then(|context| context.conversation.as_ref()).map(|id| json!({"id":id})).unwrap_or(Value::Null),
        "background":context.is_some_and(|context| context.background),"reasoning":original.get("reasoning").cloned().unwrap_or(json!({"effort":null,"summary":null})),"store":context.is_none_or(|context| context.store),"temperature":original.get("temperature").cloned().unwrap_or(json!(1.0)),"top_p":original.get("top_p").cloned().unwrap_or(json!(1.0)),"top_logprobs":original.get("top_logprobs").cloned().unwrap_or(Value::Null),
        "text":original.get("text").cloned().unwrap_or(json!({"format":{"type":"text"}})),"tool_choice":original.get("tool_choice").cloned().unwrap_or(json!("auto")),"tools":context.map(|context| context.tools.clone()).unwrap_or_default(),"truncation":original.get("truncation").cloned().unwrap_or(json!("disabled")),"metadata":original.get("metadata").cloned().unwrap_or(json!({})),
        "usage":if in_progress { Value::Null } else { json!({"input_tokens":response.input_tokens,"input_tokens_details":{"cached_tokens":0},"output_tokens":response.output_tokens,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":response.input_tokens + response.output_tokens}) }
    })
}
pub fn encode_response(response_value: &ChatResponse) -> Value {
    response_object(response_value, None, false)
}
pub fn encode_response_with_context(
    response_value: &ChatResponse,
    context: &ResponseContext,
) -> Value {
    response_object(response_value, Some(context), false)
}

#[derive(Default, Clone)]
struct Call {
    id: String,
    name: String,
    arguments: String,
    output_index: usize,
}

pub struct StreamEncoder {
    id: String,
    model: String,
    text: String,
    input_tokens: u64,
    output_tokens: u64,
    sequence: u64,
    started: bool,
    message_started: bool,
    text_output_index: usize,
    message_done: bool,
    finished: bool,
    calls: BTreeMap<usize, Call>,
    context: Option<ResponseContext>,
}
impl StreamEncoder {
    pub fn new(id: String, model: String) -> Self {
        Self::with_context(id, model, None)
    }
    pub fn with_context(id: String, model: String, context: Option<ResponseContext>) -> Self {
        Self {
            id,
            model,
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            sequence: 0,
            started: false,
            message_started: false,
            message_done: false,
            text_output_index: 0,
            finished: false,
            calls: BTreeMap::new(),
            context,
        }
    }
    fn frame(&mut self, name: &str, mut data: Value) -> String {
        data["type"] = name.into();
        data["sequence_number"] = self.sequence.into();
        self.sequence += 1;
        format!("event: {name}\ndata: {data}\n\n")
    }
    fn response(&self, in_progress: bool) -> Value {
        response_object(
            &ChatResponse::from_content(
                self.id.clone(),
                self.model.clone(),
                self.content(),
                FinishReason::Stop,
                self.input_tokens,
                self.output_tokens,
            ),
            self.context.as_ref(),
            in_progress,
        )
    }
    fn content(&self) -> Vec<ContentBlock> {
        let mut content = Vec::new();
        if !self.text.is_empty() {
            content.push(ContentBlock::Text(self.text.clone()));
        }
        content.extend(self.calls.values().map(|call| ContentBlock::ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        }));
        content
    }
    fn start(&mut self, output: &mut String) {
        if self.started {
            return;
        }
        self.started = true;
        output.push_str(&self.frame("response.created", json!({"response":self.response(true)})));
        output.push_str(&self.frame(
            "response.in_progress",
            json!({"response":self.response(true)}),
        ));
    }
    fn start_message(&mut self, output: &mut String) {
        self.start(output);
        if self.message_started || self.message_done {
            return;
        }
        self.message_started = true;
        let index = self.calls.len();
        self.text_output_index = index;
        let item = json!({"id":format!("msg_{}",self.id),"type":"message","status":"in_progress","role":"assistant","content":[]});
        output.push_str(&self.frame(
            "response.output_item.added",
            json!({"output_index":index,"item":item}),
        ));
        output.push_str(&self.frame("response.content_part.added", json!({"item_id":format!("msg_{}",self.id),"output_index":index,"content_index":0,"part":{"type":"output_text","text":"","annotations":[],"logprobs":[]}})));
    }
    fn finish_message(&mut self, output: &mut String) {
        if !self.message_started || self.message_done {
            return;
        }
        self.message_done = true;
        let index = self.text_output_index;
        output.push_str(&self.frame("response.output_text.done", json!({"item_id":format!("msg_{}",self.id),"output_index":index,"content_index":0,"text":self.text,"logprobs":[]})));
        output.push_str(&self.frame("response.content_part.done", json!({"item_id":format!("msg_{}",self.id),"output_index":index,"content_index":0,"part":{"type":"output_text","text":self.text,"annotations":[],"logprobs":[]}})));
        output.push_str(&self.frame("response.output_item.done", json!({"output_index":index,"item":{"id":format!("msg_{}",self.id),"type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":self.text,"annotations":[],"logprobs":[]}]}})));
    }
    fn stop(&mut self, reason: FinishReason, output: &mut String) {
        if self.finished {
            return;
        }
        self.start(output);
        self.finish_message(output);
        let calls: Vec<Call> = self.calls.values().cloned().collect();
        for call in calls {
            output.push_str(&self.frame("response.function_call_arguments.done", json!({"item_id":format!("fc_{}_{}",self.id,call.output_index),"output_index":call.output_index,"arguments":call.arguments})));
            output.push_str(&self.frame("response.output_item.done", json!({"output_index":call.output_index,"item":{"id":format!("fc_{}_{}",self.id,call.output_index),"type":"function_call","status":"completed","call_id":call.id,"name":call.name,"arguments":call.arguments}})));
        }
        self.finished = true;
        let response = response_object(
            &ChatResponse::from_content(
                self.id.clone(),
                self.model.clone(),
                self.content(),
                reason,
                self.input_tokens,
                self.output_tokens,
            ),
            self.context.as_ref(),
            false,
        );
        let event = match reason {
            FinishReason::Stop | FinishReason::ToolUse => "response.completed",
            FinishReason::Length | FinishReason::ContentFilter => "response.incomplete",
            FinishReason::Other => "response.failed",
        };
        output.push_str(&self.frame(event, json!({"response":response})));
    }
    pub fn encode(&mut self, events: &[StreamEvent]) -> String {
        let mut output = String::new();
        for event in events {
            match event {
                StreamEvent::Start { .. } => self.start(&mut output),
                StreamEvent::Delta(delta) | StreamEvent::Content(ContentBlock::Text(delta)) => {
                    if self.finished || delta.is_empty() {
                        continue;
                    }
                    self.start_message(&mut output);
                    self.text.push_str(delta);
                    let index = self.text_output_index;
                    output.push_str(&self.frame("response.output_text.delta", json!({"item_id":format!("msg_{}",self.id),"output_index":index,"content_index":0,"delta":delta,"logprobs":[]})));
                }
                StreamEvent::Content(ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                }) => {
                    let index = self.calls.len() + usize::from(self.message_started);
                    self.calls.insert(
                        index,
                        Call {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: arguments.clone(),
                            output_index: index,
                        },
                    );
                    output.push_str(&self.frame("response.output_item.added", json!({"output_index":index,"item":{"id":format!("fc_{}_{}",self.id,index),"type":"function_call","status":"in_progress","call_id":id,"name":name,"arguments":""}})));
                    output.push_str(&self.frame("response.function_call_arguments.delta", json!({"item_id":format!("fc_{}_{}",self.id,index),"output_index":index,"delta":arguments})));
                }
                StreamEvent::Content(_) => {}
                StreamEvent::ToolCallStart { index, id, name } => {
                    self.finish_message(&mut output);
                    let output_index = *index + usize::from(self.message_started);
                    self.calls.insert(
                        *index,
                        Call {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: String::new(),
                            output_index,
                        },
                    );
                    output.push_str(&self.frame("response.output_item.added", json!({"output_index":output_index,"item":{"id":format!("fc_{}_{}",self.id,output_index),"type":"function_call","status":"in_progress","call_id":id,"name":name,"arguments":""}})));
                }
                StreamEvent::ToolCallDelta { index, arguments } => {
                    if let Some(call) = self.calls.get_mut(index) {
                        call.arguments.push_str(arguments);
                        let item_id = format!("fc_{}_{}", self.id, call.output_index);
                        let output_index = call.output_index;
                        output.push_str(&self.frame("response.function_call_arguments.delta", json!({"item_id":item_id,"output_index":output_index,"delta":arguments})));
                    }
                }
                StreamEvent::ToolCallEnd { .. } => {}
                StreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                } => {
                    self.input_tokens = *input_tokens;
                    self.output_tokens = *output_tokens;
                }
                StreamEvent::Stop(reason) => self.stop(*reason, &mut output),
            }
        }
        output
    }
}
