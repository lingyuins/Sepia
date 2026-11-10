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
            Protocol::OpenAI => "models",
            Protocol::Anthropic => "v1/models",
            Protocol::Gemini => "models",
        }
    );
    let request = client.get(url);
    let request = match protocol {
        Protocol::OpenAI => request.bearer_auth(secret),
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
        return Err(format!("provider returned HTTP {}", response.status()));
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
