use crate::{
    Provider, Store,
    protocol::{
        ChatRequest, ChatResponse, Protocol, StreamEvent, decode_sse, encode_request,
        encode_response, encode_sse,
    },
};
use axum::{
    body::Body,
    http::{Response, header},
};
use futures_util::StreamExt;
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
pub fn json_response(protocol: Protocol, response: ChatResponse) -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(encode_response(protocol, &response).to_string()))
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
}
pub fn stream_response(
    inbound: Protocol,
    outbound: Protocol,
    response: reqwest::Response,
    start: StreamEvent,
    context: StreamContext,
) -> Response<Body> {
    let stream = async_stream::stream! {
        let mut buffer = Vec::new();
        yield Ok::<_, Infallible>(encode_sse(inbound, &[start]));
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
            for index in 0..buffer.len().saturating_sub(1) {
                if buffer[index] != b'\n' || buffer[index + 1] != b'\n' { continue; }
                let frame = String::from_utf8_lossy(&buffer[start..index]).replace("\r\n", "\n");
                start = index + 2;
                match decode_sse(outbound, &frame) {
                    Ok(decoded) => {
                        let events: Vec<_> = decoded.into_iter().filter(|event| !matches!(event, StreamEvent::Start { .. })).collect();
                        let (stops, events): (Vec<_>, Vec<_>) = events.into_iter().partition(|event| matches!(event, StreamEvent::Stop(_)));
                        if let Some(stop) = stops.into_iter().next() { terminal = Some(stop); }
                        stopped |= terminal.is_some();
                        for event in &events { if let StreamEvent::Usage { input_tokens, output_tokens } = event { usage = (*input_tokens, *output_tokens); } }
                        yield Ok(encode_sse(inbound, &events));
                    },
                    Err(_) => { failed = true; break; },
                }
            }
            buffer.drain(..start);
            if failed { break; }
        }
        if failed {
            if inbound == Protocol::Anthropic { yield Ok("event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"Upstream stream failed\"}}\n\n".into()); }
        } else if let Some(stop) = terminal {
            yield Ok(encode_sse(inbound, &[stop]));
        } else if !stopped {
            yield Ok(encode_sse(inbound, &[StreamEvent::Stop(crate::protocol::FinishReason::Other)]));
        }
        let status = if stopped && !failed { "success" } else { "failed" };
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
