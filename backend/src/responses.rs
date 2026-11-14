use crate::protocol::{
    ChatRequest, ChatResponse, FinishReason, Protocol, StreamEvent,
    decode_request as decode_chat_request,
};
use serde_json::{Map, Value, json};

pub fn decode_request(value: Value) -> Result<ChatRequest, String> {
    let root = value
        .as_object()
        .ok_or_else(|| "request must be a JSON object".to_string())?;
    for name in [
        "previous_response_id",
        "conversation",
        "background",
        "include",
        "reasoning",
        "text",
        "truncation",
        "tool_choice",
        "max_tool_calls",
    ] {
        if root.get(name).is_some_and(|value| !value.is_null()) {
            return Err(format!("unsupported_feature: {name}"));
        }
    }
    let mut messages = Vec::new();
    if let Some(instructions) = root.get("instructions").filter(|value| !value.is_null()) {
        let instructions = instructions
            .as_str()
            .ok_or("instructions must be a string")?;
        messages.push(json!({"role":"system","content":instructions}));
    }
    match root.get("input").ok_or("input is required")? {
        Value::String(input) => messages.push(json!({"role":"user","content":input})),
        Value::Array(items) => {
            if items.is_empty() {
                return Err("input must not be empty".into());
            }
            for item in items {
                let item = item
                    .as_object()
                    .ok_or("input items must be message objects")?;
                if item
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "message")
                {
                    return Err("unsupported_feature: non-message input item".into());
                }
                let role = item
                    .get("role")
                    .and_then(Value::as_str)
                    .ok_or("input message role is required")?;
                if !matches!(role, "user" | "assistant" | "system" | "developer") {
                    return Err("unsupported_feature: message role".into());
                }
                let content = match item.get("content") {
                    Some(Value::String(content)) => content.clone(),
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .map(|part| {
                            let part = part
                                .as_object()
                                .ok_or("input content parts must be objects")?;
                            let kind = part.get("type").and_then(Value::as_str);
                            let text_part = kind == Some("input_text")
                                || (role == "assistant" && kind == Some("output_text"));
                            if !text_part {
                                return Err("unsupported_feature: non-text content".into());
                            }
                            part.get("text")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .ok_or_else(|| "text content is required".into())
                        })
                        .collect::<Result<Vec<_>, String>>()?
                        .join(""),
                    _ => return Err("unsupported_feature: non-text content".into()),
                };
                messages.push(json!({"role":role,"content":content}));
            }
        }
        _ => return Err("input must be a string or message array".into()),
    }
    let mut chat = Map::new();
    chat.insert(
        "model".into(),
        root.get("model").cloned().unwrap_or(Value::Null),
    );
    chat.insert("messages".into(), Value::Array(messages));
    for name in ["temperature", "top_p", "stream", "tools"] {
        if let Some(value) = root.get(name) {
            chat.insert(name.into(), value.clone());
        }
    }
    chat.insert(
        "max_tokens".into(),
        root.get("max_output_tokens")
            .cloned()
            .unwrap_or(Value::Null),
    );
    decode_chat_request(Protocol::OpenAI, Value::Object(chat), None)
}

fn message(id: &str, text: &str, status: &str) -> Value {
    json!({
        "id":format!("msg_{id}"),
        "type":"message",
        "status":status,
        "role":"assistant",
        "content":[{"type":"output_text","text":text,"annotations":[]}]
    })
}

fn response(
    id: &str,
    model: &str,
    text: &str,
    reason: FinishReason,
    input_tokens: u64,
    output_tokens: u64,
    in_progress: bool,
) -> Value {
    let (status, item_status) = if in_progress {
        ("in_progress", "in_progress")
    } else {
        match reason {
            FinishReason::Stop => ("completed", "completed"),
            FinishReason::Length => ("incomplete", "incomplete"),
            FinishReason::Other => ("failed", "incomplete"),
        }
    };
    json!({
        "id":format!("resp_{id}"),
        "object":"response",
        "created_at":0,
        "completed_at":if in_progress { Value::Null } else { json!(0) },
        "status":status,
        "error":if reason == FinishReason::Other && !in_progress {
            json!({"code":"server_error","message":"Upstream response failed"})
        } else {
            Value::Null
        },
        "incomplete_details":if reason == FinishReason::Length && !in_progress {
            json!({"reason":"max_output_tokens"})
        } else {
            Value::Null
        },
        "instructions":null,
        "model":model,
        "output":if in_progress { json!([]) } else { json!([message(id, text, item_status)]) },
        "usage":if in_progress {
            Value::Null
        } else {
            json!({
                "input_tokens":input_tokens,
                "input_tokens_details":{"cached_tokens":0},
                "output_tokens":output_tokens,
                "output_tokens_details":{"reasoning_tokens":0},
                "total_tokens":input_tokens + output_tokens
            })
        }
    })
}

pub fn encode_response(response_value: &ChatResponse) -> Value {
    response(
        &response_value.id,
        &response_value.model,
        &response_value.text,
        response_value.finish_reason,
        response_value.input_tokens,
        response_value.output_tokens,
        false,
    )
}

pub struct StreamEncoder {
    id: String,
    model: String,
    text: String,
    input_tokens: u64,
    output_tokens: u64,
    sequence: u64,
    started: bool,
    finished: bool,
}

impl StreamEncoder {
    pub fn new(id: String, model: String) -> Self {
        Self {
            id,
            model,
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            sequence: 0,
            started: false,
            finished: false,
        }
    }

    fn frame(&mut self, name: &str, mut data: Value) -> String {
        data["type"] = name.into();
        data["sequence_number"] = self.sequence.into();
        self.sequence += 1;
        format!("event: {name}\ndata: {data}\n\n")
    }

    fn start(&mut self, output: &mut String) {
        if self.started {
            return;
        }
        self.started = true;
        let created = response(&self.id, &self.model, "", FinishReason::Stop, 0, 0, true);
        output.push_str(&self.frame("response.created", json!({"response":created})));
        let item = json!({
            "id":format!("msg_{}",self.id),
            "type":"message",
            "status":"in_progress",
            "role":"assistant",
            "content":[]
        });
        output.push_str(&self.frame(
            "response.output_item.added",
            json!({"output_index":0,"item":item}),
        ));
        output.push_str(&self.frame(
            "response.content_part.added",
            json!({
                "item_id":format!("msg_{}",self.id),
                "output_index":0,
                "content_index":0,
                "part":{"type":"output_text","text":"","annotations":[]}
            }),
        ));
    }

    fn stop(&mut self, reason: FinishReason, output: &mut String) {
        if self.finished {
            return;
        }
        self.start(output);
        self.finished = true;
        let item_id = format!("msg_{}", self.id);
        output.push_str(&self.frame(
            "response.output_text.done",
            json!({
                "item_id":item_id,
                "output_index":0,
                "content_index":0,
                "text":self.text
            }),
        ));
        output.push_str(&self.frame(
            "response.content_part.done",
            json!({
                "item_id":item_id,
                "output_index":0,
                "content_index":0,
                "part":{"type":"output_text","text":self.text,"annotations":[]}
            }),
        ));
        let item_status = if reason == FinishReason::Stop {
            "completed"
        } else {
            "incomplete"
        };
        let item = message(&self.id, &self.text, item_status);
        output.push_str(&self.frame(
            "response.output_item.done",
            json!({"output_index":0,"item":item}),
        ));
        let event = match reason {
            FinishReason::Stop => "response.completed",
            FinishReason::Length => "response.incomplete",
            FinishReason::Other => "response.failed",
        };
        let final_response = response(
            &self.id,
            &self.model,
            &self.text,
            reason,
            self.input_tokens,
            self.output_tokens,
            false,
        );
        output.push_str(&self.frame(event, json!({"response":final_response})));
    }

    pub fn encode(&mut self, events: &[StreamEvent]) -> String {
        let mut output = String::new();
        for event in events {
            match event {
                StreamEvent::Start { .. } => self.start(&mut output),
                StreamEvent::Delta(delta) => {
                    if self.finished || delta.is_empty() {
                        continue;
                    }
                    self.start(&mut output);
                    self.text.push_str(delta);
                    output.push_str(&self.frame(
                        "response.output_text.delta",
                        json!({
                            "item_id":format!("msg_{}",self.id),
                            "output_index":0,
                            "content_index":0,
                            "delta":delta
                        }),
                    ));
                }
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
