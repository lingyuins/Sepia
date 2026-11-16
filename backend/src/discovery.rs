use crate::{Provider, protocol::Protocol};
use serde_json::Value;

pub async fn discover_models(
    client: &reqwest::Client,
    provider: &Provider,
    secret: &str,
) -> Result<Vec<String>, String> {
    let protocol = crate::relay::outbound_protocol(provider);
    let url = format!(
        "{}/{}",
        provider.endpoint.trim_end_matches('/'),
        match protocol {
            Protocol::OpenAI | Protocol::OpenAIResponses => "models",
            Protocol::Anthropic => "v1/models",
            Protocol::Gemini => "models",
        }
    );
    let request = client.get(url);
    let request = match protocol {
        Protocol::OpenAI | Protocol::OpenAIResponses => request.bearer_auth(secret),
        Protocol::Anthropic => request
            .header("x-api-key", secret)
            .header("anthropic-version", "2023-06-01"),
        Protocol::Gemini => request.header("x-goog-api-key", secret),
    };
    let response = request
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| "provider connection failed".to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<Value>(&detail)
            .ok()
            .and_then(|body| {
                body.get("message")
                    .or_else(|| body.get("error"))
                    .and_then(|value| {
                        value
                            .as_str()
                            .or_else(|| value.get("message").and_then(Value::as_str))
                    })
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| detail.chars().take(300).collect());
        return Err(if message.trim().is_empty() {
            format!("provider returned HTTP {status}")
        } else {
            format!("provider returned HTTP {status}: {message}")
        });
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "invalid provider model list".to_string())?;
    let models = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .ok_or("provider model list missing".to_string())?
        .iter()
        .filter_map(|model| {
            model
                .get("id")
                .or_else(|| model.get("name"))
                .and_then(Value::as_str)
                .map(|name| name.strip_prefix("models/").unwrap_or(name).to_string())
        })
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err("provider returned no models".into());
    }
    Ok(models)
}
