//! Real upstream availability probes for route tests and provider-key tests.
//! A probe sends one minimal non-streaming chat request per endpoint leg and
//! reports the decoded outcome; it never writes request-log records.
use crate::{
    Provider,
    protocol::{ChatRequest, Message, Protocol, Role},
};
use std::time::Instant;

pub struct ProbeOutcome {
    pub ok: bool,
    pub latency_ms: i64,
    pub endpoint: &'static str,
    pub error: Option<String>,
    pub reply: Option<String>,
    /// Effective upstream model after built-in parameter rewriting (the model
    /// actually sent to the upstream, which may differ from the candidate).
    pub effective_model: String,
    /// Static rewrite headers applied to this probe, masked for display.
    pub headers: Vec<(String, String)>,
}

fn failure(started: Instant, endpoint: &'static str, error: String) -> ProbeOutcome {
    ProbeOutcome {
        ok: false,
        latency_ms: started.elapsed().as_millis() as i64,
        endpoint,
        error: Some(error),
        reply: None,
        effective_model: String::new(),
        headers: Vec::new(),
    }
}

/// Mask a header value for probe display: keep a short prefix, redact the rest.
fn mask_header_value(value: &str) -> String {
    if value.chars().count() <= 4 {
        return "••••".into();
    }
    let prefix: String = value.chars().take(4).collect();
    format!("{prefix}…")
}

fn masked_headers(config: &crate::rewrite::RewriteConfig) -> Vec<(String, String)> {
    crate::rewrite::header_pairs(config)
        .into_iter()
        .map(|(name, value)| (name, mask_header_value(&value)))
        .collect()
}

fn upstream_detail(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .or_else(|| value.get("error"))
                .and_then(|error| {
                    error
                        .as_str()
                        .or_else(|| error.get("message").and_then(serde_json::Value::as_str))
                })
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.chars().take(200).collect())
}

/// Probe entry that routes plugin-backed providers through the plugin fetch
/// hook so route tests exercise the same path as inference.
pub async fn probe_model_for(
    state: &crate::server::AppState,
    provider: &Provider,
    secret: &str,
    model: &str,
    legs: &[Protocol],
    route_rewrite: &crate::rewrite::RewriteConfig,
) -> ProbeOutcome {
    let rewrite = crate::rewrite::merge(&provider.rewrite, route_rewrite);
    let headers = masked_headers(&rewrite);
    if provider.plugin_id.is_none() {
        let snapshot = state.snapshots.get();
        let mut outcome = probe_model(
            &state.http(),
            provider,
            secret,
            model,
            legs,
            crate::relay::Timeouts {
                first_byte_seconds: snapshot.first_byte_timeout_seconds,
                stream_idle_seconds: snapshot.stream_idle_timeout_seconds,
            },
            &rewrite,
        )
        .await;
        outcome.headers = headers;
        return outcome;
    }
    let mut request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message::new(Role::User, "ping")],
        max_tokens: Some(512),
        ..Default::default()
    };
    crate::rewrite::apply_prompt(&mut request, &rewrite.prompt_replace);
    let mut last: Option<ProbeOutcome> = None;
    for protocol in legs {
        let endpoint = crate::relay::endpoint_label(*protocol);
        let started = Instant::now();
        // Gemini carries the model in the URL path, so `model` rules rewrite the
        // string itself; every other protocol rewrites the encoded body field.
        let gemini_model = (*protocol == Protocol::Gemini)
            .then(|| crate::rewrite::effective_model_for(model, &rewrite));
        let encode_model = gemini_model.as_deref().unwrap_or(model);
        let mut encoded = match crate::protocol::encode_request(*protocol, &request, encode_model) {
            Ok(body) => body,
            Err(message) => {
                last = Some(failure(started, endpoint, message));
                continue;
            }
        };
        crate::rewrite::apply_params(&mut encoded, &rewrite, *protocol);
        let effective_model = match gemini_model {
            Some(rewritten) => rewritten,
            None => crate::rewrite::effective_model(&encoded, model),
        };
        let response = match crate::plugin::provider_fetch(
            state,
            provider,
            secret,
            &request,
            &effective_model,
            *protocol,
            &rewrite,
            &crate::rewrite::header_pairs(&rewrite),
        )
        .await
        {
            Ok(response) => response,
            Err(message) => {
                last = Some(failure(started, endpoint, message));
                continue;
            }
        };
        let latency_ms = started.elapsed().as_millis() as i64;
        let status = response.status();
        if !status.is_success() {
            let detail = upstream_detail(&response.text().await.unwrap_or_default());
            last = Some(ProbeOutcome {
                ok: false,
                latency_ms,
                endpoint,
                error: Some(format!("upstream HTTP {status} ({endpoint}): {detail}")),
                reply: None,
                effective_model: effective_model.clone(),
                headers: headers.clone(),
            });
            return last.unwrap();
        }
        let value = match response.json::<serde_json::Value>().await {
            Ok(value) => value,
            Err(_) => {
                last = Some(failure(
                    started,
                    endpoint,
                    format!("invalid upstream JSON ({endpoint})"),
                ));
                continue;
            }
        };
        match crate::protocol::decode_response(*protocol, value) {
            Ok(decoded) => {
                return ProbeOutcome {
                    ok: true,
                    latency_ms,
                    endpoint,
                    error: None,
                    reply: Some(decoded.text.chars().take(200).collect()),
                    effective_model: effective_model.clone(),
                    headers: headers.clone(),
                };
            }
            Err(message) => {
                last = Some(failure(started, endpoint, message));
            }
        }
    }
    last.unwrap_or_else(|| failure(Instant::now(), "plugin", "no probe leg".into()))
}

pub async fn probe_model(
    client: &reqwest::Client,
    provider: &Provider,
    secret: &str,
    model: &str,
    legs: &[Protocol],
    timeouts: crate::relay::Timeouts,
    rewrite: &crate::rewrite::RewriteConfig,
) -> ProbeOutcome {
    // Reasoning models spend output tokens on hidden reasoning before any
    // visible content; a tiny budget makes them answer with empty content,
    // which strict upstream gateways reject as a failed channel. Give the
    // probe enough room to finish reasoning and emit a short reply.
    let mut request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message::new(Role::User, "ping")],
        max_tokens: Some(512),
        ..Default::default()
    };
    crate::rewrite::apply_prompt(&mut request, &rewrite.prompt_replace);
    let mut last: Option<ProbeOutcome> = None;
    for protocol in legs {
        let endpoint = crate::relay::endpoint_label(*protocol);
        let started = Instant::now();
        // Same split as inference: Gemini rewrites its URL-path model, the rest
        // rewrite the encoded body field.
        let gemini_model = (*protocol == Protocol::Gemini)
            .then(|| crate::rewrite::effective_model_for(model, rewrite));
        let encode_model = gemini_model.as_deref().unwrap_or(model);
        let mut encoded = match crate::protocol::encode_request(*protocol, &request, encode_model) {
            Ok(body) => body,
            Err(message) => {
                last = Some(failure(started, endpoint, message));
                continue;
            }
        };
        crate::rewrite::apply_params(&mut encoded, rewrite, *protocol);
        let effective_model = match gemini_model {
            Some(rewritten) => rewritten,
            None => crate::rewrite::effective_model(&encoded, model),
        };
        let overrides = crate::relay::Overrides {
            body: Some(encoded),
            headers: crate::rewrite::header_pairs(rewrite),
        };
        let response = match crate::relay::call_with_overrides(
            client,
            provider,
            secret,
            &request,
            &effective_model,
            *protocol,
            &overrides,
            timeouts,
        )
        .await
        {
            Ok(response) => response,
            Err(message) => {
                last = Some(failure(started, endpoint, message));
                continue;
            }
        };
        let latency_ms = started.elapsed().as_millis() as i64;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            let detail = upstream_detail(&detail);
            let message = if detail.trim().is_empty() {
                format!("upstream HTTP {status} ({endpoint})")
            } else {
                format!("upstream HTTP {status} ({endpoint}): {detail}")
            };
            last = Some(ProbeOutcome {
                ok: false,
                latency_ms,
                endpoint,
                error: Some(message),
                reply: None,
                effective_model: effective_model.clone(),
                headers: Vec::new(),
            });
            // A missing endpoint may exist on the other leg of a universal
            // OpenAI provider; anything else is a real answer about this key.
            if matches!(status.as_u16(), 404 | 405 | 501) {
                continue;
            }
            return last.unwrap();
        }
        let value = match response.json::<serde_json::Value>().await {
            Ok(value) => value,
            Err(_) => {
                last = Some(failure(
                    started,
                    endpoint,
                    format!("invalid upstream JSON ({endpoint})"),
                ));
                continue;
            }
        };
        match crate::protocol::decode_response(*protocol, value) {
            Ok(decoded) => {
                return ProbeOutcome {
                    ok: true,
                    latency_ms,
                    endpoint,
                    error: None,
                    reply: Some(decoded.text.chars().take(200).collect()),
                    effective_model,
                    headers: Vec::new(),
                };
            }
            Err(message) => {
                last = Some(failure(
                    started,
                    endpoint,
                    format!("{message} ({endpoint})"),
                ));
                continue;
            }
        }
    }
    last.unwrap_or_else(|| {
        failure(
            Instant::now(),
            "upstream",
            "no upstream endpoint available".into(),
        )
    })
}

impl ProbeOutcome {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ok": self.ok,
            "latency_ms": self.latency_ms,
            "endpoint": self.endpoint,
            "error": self.error,
            "reply": self.reply,
            "effective_model": self.effective_model,
            "headers": self.headers.iter().map(|(name, value)| serde_json::json!({"name":name,"value":value})).collect::<Vec<_>>(),
        })
    }
}
