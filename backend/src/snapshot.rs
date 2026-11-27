use crate::{Provider, Route, Store};
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct ConfigSnapshot {
    pub providers: Vec<Provider>,
    pub routes: Vec<Route>,
    pub max_attempts: usize,
    /// TCP connect timeout for the shared outbound client.
    pub connect_timeout_seconds: u64,
    /// How long an upstream may take to return response headers.
    pub first_byte_timeout_seconds: u64,
    /// Maximum gap between chunks of an upstream SSE stream.
    pub stream_idle_timeout_seconds: u64,
    /// Upper bound on honoring an upstream `Retry-After` header.
    pub retry_after_cap_seconds: u64,
    pub log_bodies: bool,
    /// Configured channel prices in micros/token; only fully priced entries.
    pub prices: HashMap<(i64, String), (i64, i64)>,
    /// Known context windows from synced Model Plaza metadata (bare model id).
    pub contexts: HashMap<String, i64>,
}
/// Numeric runtime settings share one range contract with `Store::set_setting`
/// so persisted values and snapshot fallbacks can never disagree.
pub fn bounded_seconds(
    settings: &HashMap<String, serde_json::Value>,
    key: &str,
    default: u64,
    max: u64,
) -> u64 {
    settings
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(default)
        .clamp(1, max)
}
fn log_bodies(settings: &std::collections::HashMap<String, serde_json::Value>) -> bool {
    settings
        .get("log_bodies")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}
pub struct Snapshots(ArcSwap<ConfigSnapshot>);
async fn snapshot(store: &Store) -> Result<ConfigSnapshot, String> {
    let settings = store.settings().await?;
    let prices = store
        .model_prices()
        .await?
        .into_iter()
        .map(|(provider_id, model, input, output, _)| ((provider_id, model), (input, output)))
        .collect();
    let mut contexts = HashMap::new();
    for (model, meta) in store.model_metadata().await? {
        if meta.context_length > 0 {
            contexts
                .entry(crate::metadata::bare_id(&model))
                .and_modify(|window: &mut i64| *window = (*window).max(meta.context_length))
                .or_insert(meta.context_length);
        }
    }
    Ok(ConfigSnapshot {
        providers: store
            .providers()
            .await?
            .into_iter()
            .filter(|provider| provider.enabled)
            .collect(),
        routes: store
            .routes()
            .await?
            .into_iter()
            .filter(|route| route.enabled)
            .collect(),
        max_attempts: bounded_seconds(&settings, "max_attempts", 3, 16) as usize,
        connect_timeout_seconds: bounded_seconds(&settings, "connect_timeout_seconds", 10, 60),
        first_byte_timeout_seconds: bounded_seconds(
            &settings,
            "first_byte_timeout_seconds",
            60,
            600,
        ),
        stream_idle_timeout_seconds: bounded_seconds(
            &settings,
            "stream_idle_timeout_seconds",
            60,
            600,
        ),
        retry_after_cap_seconds: bounded_seconds(&settings, "retry_after_cap_seconds", 5, 60),
        log_bodies: log_bodies(&settings),
        prices,
        contexts,
    })
}
impl Snapshots {
    pub async fn load(store: &Store) -> Result<Self, String> {
        Ok(Self(ArcSwap::from_pointee(snapshot(store).await?)))
    }
    pub fn get(&self) -> Arc<ConfigSnapshot> {
        self.0.load_full()
    }
    pub async fn refresh(&self, store: &Store) -> Result<(), String> {
        self.0.store(Arc::new(snapshot(store).await?));
        Ok(())
    }
}
