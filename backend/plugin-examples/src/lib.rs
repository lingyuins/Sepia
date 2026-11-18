//! Reference Sepia WASM plugin exercising every hook and host capability.
//! Build: cargo build -p sepia-plugin-examples --target wasm32-unknown-unknown --release
//! Events: relay.request.before_route (rewrites model to config.model),
//! relay.route.filter / relay.strategy (excludes config.exclude provider ids),
//! relay.request.before_upstream (adds x-sepia-plugin header),
//! relay.response.before_send (appends marker to content),
//! relay.completed (counts completions in kv), provider.models / provider.fetch
//! (synthetic catalog + chat response), http.rpc (echo + kv counter),
//! plugin.migrate (kv schema bump). Capabilities: kv, secret, http, log.
#![no_std]
extern crate alloc;
use alloc::string::ToString;
use sepia_plugin_sdk::{kv_get, kv_set, log, secret_set};
use sepia_plugin_sdk::json::Value;

#[no_mangle]
pub extern "Rust" fn dispatch(event: &str, payload: &Value, config: &Value) -> Value {
    match event {
        "relay.request.before_route" => {
            let mut request = payload.get("request").cloned().unwrap_or(Value::object());
            if let Some(model) = config.get("model").and_then(Value::as_str) {
                request.insert("model", Value::str(model));
            }
            let mut out = Value::object();
            out.insert("request", request);
            out
        }
        "relay.route.filter" | "relay.strategy" => {
            let mut out = Value::object();
            if let Some(exclude) = config.get("exclude").and_then(Value::as_array) {
                out.insert("exclude_channel_ids", Value::Arr(exclude.clone()));
            }
            out
        }
        "relay.request.before_upstream" => {
            let mut out = Value::object();
            let mut headers = Value::Arr(alloc::vec::Vec::new());
            if let Value::Arr(items) = &mut headers {
                let mut h = Value::object();
                h.insert("name", Value::str("x-sepia-plugin"));
                h.insert("value", Value::str("echo"));
                items.push(h);
            }
            out.insert("headers", headers);
            out
        }
        "relay.response.before_send" => {
            let mut response = payload.get("response").cloned().unwrap_or(Value::object());
            if let Some(Value::Arr(choices)) = response.get("choices").cloned() {
                let mut new_choices = alloc::vec::Vec::new();
                for choice in choices {
                    let mut choice = choice;
                    if let Some(message) = choice.get("message").cloned() {
                        let mut message = message;
                        let text = message.get("content").and_then(Value::as_str).unwrap_or("");
                        message.insert("content", Value::str(&alloc::format!("{text} [plugin]")));
                        choice.insert("message", message);
                    }
                    new_choices.push(choice);
                }
                response.insert("choices", Value::Arr(new_choices));
            }
            let mut out = Value::object();
            out.insert("response", response);
            out
        }
        "relay.completed" => {
            let count = kv_get("completions")
                .and_then(|b| core::str::from_utf8(&b).ok().and_then(|s| s.parse::<i64>().ok()))
                .unwrap_or(0);
            kv_set("completions", &(count + 1).to_string());
            Value::object()
        }
        "provider.models" => {
            let mut out = Value::object();
            out.insert("models", Value::Arr(alloc::vec![Value::str("plugin-chat")]));
            out
        }
        "provider.fetch" => {
            let model = payload.get("model").and_then(Value::as_str).unwrap_or("plugin-chat");
            let body = alloc::format!(
                "{{\"id\":\"chat_plugin\",\"model\":\"{model}\",\"choices\":[{{\"index\":0,\"message\":{{\"role\":\"assistant\",\"content\":\"plugin pong\"}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":2,\"completion_tokens\":2}}}}"
            );
            let mut out = Value::object();
            out.insert("status", Value::num(200));
            let mut headers = Value::object();
            headers.insert("content-type", Value::str("application/json"));
            out.insert("headers", headers);
            out.insert("body", Value::str(&body));
            out
        }
        "http.rpc" => {
            let path = payload.get("path").and_then(Value::as_str).unwrap_or("");
            if path == "count" {
                let count = kv_get("completions")
                    .and_then(|b| core::str::from_utf8(&b).ok().map(|s| s.to_string()))
                    .unwrap_or_else(|| "0".to_string());
                let mut out = Value::object();
                out.insert("completions", Value::str(&count));
                return out;
            }
            if path == "store-secret" {
                let value = payload.get("body").and_then(|b| b.get("value")).and_then(Value::as_str).unwrap_or("");
                secret_set("demo", value);
                let mut out = Value::object();
                out.insert("stored", Value::Bool(true));
                return out;
            }
            log(&alloc::format!("rpc {path}"));
            let mut out = Value::object();
            out.insert("echo", payload.get("body").cloned().unwrap_or(Value::Null));
            out.insert("path", Value::str(path));
            out
        }
        "plugin.migrate" => {
            kv_set("schema", "1");
            Value::object()
        }
        _ => Value::object(),
    }
}