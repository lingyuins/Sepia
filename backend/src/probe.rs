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
}

fn failure(started: Instant, endpoint: &'static str, error: String) -> ProbeOutcome {
    ProbeOutcome {
        ok: false,
        latency_ms: started.elapsed().as_millis() as i64,
        endpoint,
        error: Some(error),
        reply: None,
    }
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
) -> ProbeOutcome {
    if provider.plugin_id.is_none() {
        let snapshot = state.snapshots.get();
        return probe_model(
            &state.http(),
            provider,
            secret,
            model,
            legs,
            crate::relay::Timeouts {
                first_byte_seconds: snapshot.first_byte_timeout_seconds,
                stream_idle_seconds: snapshot.stream_idle_timeout_seconds,
            },
        )
        .await;
    }
    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message::new(Role::User, "ping")],
        max_tokens: Some(512),
        ..Default::default()
    };
    let mut last: Option<ProbeOutcome> = None;
    for protocol in legs {
        let endpoint = crate::relay::endpoint_label(*protocol);
        let started = Instant::now();
        let response = match crate::plugin::provider_fetch(
            state, provider, secret, &request, model, *protocol,
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
) -> ProbeOutcome {
    // Reasoning models spend output tokens on hidden reasoning before any
    // visible content; a tiny budget makes them answer with empty content,
    // which strict upstream gateways reject as a failed channel. Give the
    // probe enough room to finish reasoning and emit a short reply.
    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message::new(Role::User, "ping")],
        max_tokens: Some(512),
        ..Default::default()
    };
    let mut last: Option<ProbeOutcome> = None;
    for protocol in legs {
        let endpoint = crate::relay::endpoint_label(*protocol);
        let started = Instant::now();
        let response = match crate::relay::call_with_overrides(
            client,
            provider,
            secret,
            &request,
            model,
            *protocol,
            &crate::relay::Overrides::default(),
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
        })
    }
}
