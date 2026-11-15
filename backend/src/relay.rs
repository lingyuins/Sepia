use crate::{
    Provider, Store,
    protocol::{
        ChatRequest, ChatResponse, Protocol, StreamEvent, decode_sse, encode_request,
        encode_response, encode_sse,
    },
};
use axum::{
    body::Body,
    http::{Response, StatusCode, header},
};
use futures_util::StreamExt;
use serde_json::json;
use std::{convert::Infallible, time::Instant};

pub fn protected_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
}
pub fn outbound_protocol(provider: &Provider) -> Protocol {
    match provider.protocol {
        crate::ProviderProtocol::OpenAI => Protocol::OpenAI,
        crate::ProviderProtocol::Anthropic => Protocol::Anthropic,
        crate::ProviderProtocol::Gemini => Protocol::Gemini,
    }
}
pub fn upstream_url(provider: &Provider, model: &str, stream: bool) -> String {
    let path = match outbound_protocol(provider) {
        Protocol::OpenAI => "chat/completions".into(),
        Protocol::Anthropic => "v1/messages".into(),
        Protocol::Gemini => format!(
            "models/{}:{}",
            url::form_urlencoded::byte_serialize(model.as_bytes()).collect::<String>(),
            if stream {
                "streamGenerateContent?alt=sse"
            } else {
                "generateContent"
            }
        ),
    };
    format!("{}/{path}", provider.endpoint.trim_end_matches('/'))
}
pub async fn call(
    client: &reqwest::Client,
    provider: &Provider,
    secret: &str,
    request: &ChatRequest,
    model: &str,
) -> Result<reqwest::Response, String> {
    let protocol = outbound_protocol(provider);
    let body = encode_request(protocol, request, model)?;
    let outgoing = client
        .post(upstream_url(provider, model, request.stream))
        .json(&body);
    let outgoing = match protocol {
        Protocol::OpenAI => outgoing.bearer_auth(secret),
        Protocol::Anthropic => outgoing
            .header("x-api-key", secret)
            .header("anthropic-version", "2023-06-01"),
        Protocol::Gemini => outgoing.header("x-goog-api-key", secret),
    };
    outgoing
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|_| "upstream unavailable".into())
}
pub async fn json_response(
    inbound: crate::server::InboundProtocol,
    response: ChatResponse,
    context: Option<&crate::responses::ResponseContext>,
    response_state: Option<(i64, String)>,
    state: &crate::server::AppState,
) -> Response<Body> {
    let value = match inbound {
        crate::server::InboundProtocol::Gemini { .. } => {
            encode_response(Protocol::Gemini, &response)
        }
        crate::server::InboundProtocol::Provider(protocol) => encode_response(protocol, &response),
        crate::server::InboundProtocol::OpenAIResponses => {
            let value = context
                .map(|context| crate::responses::encode_response_with_context(&response, context))
                .unwrap_or_else(|| crate::responses::encode_response(&response));
            if let (Some((client_key, id)), Some(context)) = (&response_state, context) {
                let id = id.strip_prefix("resp_").unwrap_or(id);
                if state
                    .store
                    .response(*client_key, id)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|stored| stored.cancel_requested)
                {
                    return Response::builder()
                        .status(StatusCode::CONFLICT)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            json!({"error":{"message":"response was cancelled"}}).to_string(),
                        ))
                        .unwrap();
                }
                let output = crate::responses::output_items(&response);
                let status = value
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("completed");
                let _ = state
                    .store
                    .update_response(*client_key, id, status, &value, &output)
                    .await;
                if let Some(conversation) = &context.conversation {
                    let mut items = context.input_items.clone();
                    items.extend(output);
                    let _ = state
                        .store
                        .append_conversation_items(*client_key, conversation, &items)
                        .await;
                }
            }
            value
        }
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}
pub struct StreamContext {
    pub store: Store,
    pub request_id: String,
    pub provider_id: i64,
    pub provider_key_id: i64,
    pub model: String,
    pub guard: crate::lifecycle::RequestGuard,
    pub started: Instant,
    pub response_context: Option<crate::responses::ResponseContext>,
    pub response_state: Option<(i64, String)>,
}
fn sse_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(boundary), None) | (None, Some(boundary)) => Some(boundary),
        (None, None) => None,
    }
}
pub fn stream_response(
    inbound: crate::server::InboundProtocol,
    outbound: Protocol,
    response: reqwest::Response,
    start: StreamEvent,
    context: StreamContext,
) -> Response<Body> {
    let stream = async_stream::stream! {
        let mut buffer = Vec::new();
        let mut responses = match inbound {
            crate::server::InboundProtocol::OpenAIResponses => match &start {
                StreamEvent::Start { id, model } => Some(crate::responses::StreamEncoder::with_context(id.clone(), model.clone(), context.response_context.clone())),
                _ => None,
            },
            _ => None,
        };
        let encode = |inbound: &crate::server::InboundProtocol, events: &[StreamEvent], responses: &mut Option<crate::responses::StreamEncoder>| {
            match (inbound, responses) {
                (crate::server::InboundProtocol::OpenAIResponses, Some(encoder)) => encoder.encode(events),
                (crate::server::InboundProtocol::Provider(protocol), _) => encode_sse(*protocol, events),
                (crate::server::InboundProtocol::Gemini { .. }, _) => encode_sse(Protocol::Gemini, events),
                _ => String::new(),
            }
        };
        yield Ok::<_, Infallible>(encode(&inbound, std::slice::from_ref(&start), &mut responses));
        let mut recorded_events = Vec::new();
        let mut recorded_content = Vec::new();
        let mut terminal_reason = None;
        if inbound == crate::server::InboundProtocol::OpenAIResponses {
            recorded_events.push(json!({"type":"response.created","sequence_number":0}));
        }
        let mut bytes = response.bytes_stream();
        let mut stopped = false;
        let mut terminal = None;
        let mut usage = (0, 0);
        let mut failed = false;
        loop {
            let chunk = match tokio::time::timeout(std::time::Duration::from_secs(60), bytes.next()).await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(_) => { failed = true; break; },
            };
            let Ok(chunk) = chunk else { failed = true; break; };
            buffer.extend_from_slice(&chunk);
            let mut start = 0;
            while let Some((index, separator_length)) = sse_boundary(&buffer[start..]) {
                let end = start + index;
                let frame = String::from_utf8_lossy(&buffer[start..end]).replace("\r\n", "\n");
                start = end + separator_length;
                match decode_sse(outbound, &frame) {
                    Ok(decoded) => {
                        let events: Vec<_> = decoded.into_iter().filter(|event| !matches!(event, StreamEvent::Start { .. })).collect();
                        let (stops, events): (Vec<_>, Vec<_>) = events.into_iter().partition(|event| matches!(event, StreamEvent::Stop(_)));
                        if let Some(stop) = stops.into_iter().next() { terminal = Some(stop); }
                        stopped |= terminal.is_some();
                        for event in &events { if let StreamEvent::Usage { input_tokens, output_tokens } = event { usage = (*input_tokens, *output_tokens); } }
                        for event in &events {
                            match event {
                                StreamEvent::Delta(text) => recorded_content.push(crate::protocol::ContentBlock::Text(text.clone())),
                                StreamEvent::Content(block) => recorded_content.push(block.clone()),
                                StreamEvent::ToolCallStart { id, name, .. } => recorded_content.push(crate::protocol::ContentBlock::ToolCall { id:id.clone(), name:name.clone(), arguments:String::new() }),
                                StreamEvent::ToolCallDelta { arguments, .. } => { if let Some(crate::protocol::ContentBlock::ToolCall { arguments: existing, .. }) = recorded_content.iter_mut().rev().find(|block| matches!(block, crate::protocol::ContentBlock::ToolCall { .. })) { existing.push_str(arguments); } }
                                StreamEvent::Stop(reason) => terminal_reason = Some(*reason),
                                _ => {}
                            }
                        }
                        yield Ok(encode(&inbound, &events, &mut responses));
                    },
                    Err(_) => { failed = true; break; },
                }
            }
            buffer.drain(..start);
            if failed { break; }
        }
        if failed {
            match inbound {
                crate::server::InboundProtocol::Provider(Protocol::Anthropic) => yield Ok("event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"Upstream stream failed\"}}\n\n".into()),
                crate::server::InboundProtocol::OpenAIResponses => yield Ok(encode(&inbound, &[StreamEvent::Stop(crate::protocol::FinishReason::Other)], &mut responses)),
                crate::server::InboundProtocol::Provider(_) | crate::server::InboundProtocol::Gemini { .. } => {}
            }
        } else if let Some(stop) = terminal {
            yield Ok(encode(&inbound, &[stop], &mut responses));
        } else if !stopped {
            yield Ok(encode(&inbound, &[StreamEvent::Stop(crate::protocol::FinishReason::Other)], &mut responses));
        }
        let status = if stopped && !failed { "success" } else { "failed" };
        if let (Some((client_key, id)), Some(response_context)) = (&context.response_state, &context.response_context) {
            let id = id.strip_prefix("resp_").unwrap_or(id);
            if context.store.response(*client_key, id).await.ok().flatten().is_some_and(|stored| stored.cancel_requested) {
                return;
            }
            let response = crate::protocol::ChatResponse::from_content(context.request_id.clone(), context.model.clone(), recorded_content, if stopped && !failed { terminal_reason.unwrap_or(crate::protocol::FinishReason::Stop) } else { crate::protocol::FinishReason::Other }, usage.0, usage.1);
            let value = crate::responses::encode_response_with_context(&response, response_context);
            let output = crate::responses::output_items(&response);
            let _ = context.store.update_response(*client_key, id, value.get("status").and_then(serde_json::Value::as_str).unwrap_or("failed"), &value, &output).await;
            let _ = context.store.append_response_events(*client_key, id, &recorded_events).await;
            if let Some(conversation) = &response_context.conversation { let mut items = response_context.input_items.clone(); items.extend(output); let _ = context.store.append_conversation_items(*client_key, conversation, &items).await; }
        }
        let _ = context.store.record_attempt(crate::observability::NewAttempt { request_id: &context.request_id, provider_id: Some(context.provider_id), provider_key_id: Some(context.provider_key_id), model: &context.model, status, error: None, input_tokens: usage.0, output_tokens: usage.1, latency_ms: context.started.elapsed().as_millis() as i64 }).await;
        let cost = context.store.model_cost(&context.model, usage.0, usage.1).await.unwrap_or(0);
        let _ = context.guard.complete(status, usage.0, usage.1, cost).await;
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}
