use crate::{Provider, Route, Store};
use arc_swap::ArcSwap;
use std::sync::Arc;

#[derive(Default)]
pub struct ConfigSnapshot {
    pub providers: Vec<Provider>,
    pub routes: Vec<Route>,
    pub max_attempts: usize,
    pub log_bodies: bool,
}
fn max_attempts(settings: std::collections::HashMap<String, serde_json::Value>) -> usize {
    settings
        .get("max_attempts")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(3)
        .clamp(1, 16) as usize
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
        max_attempts: max_attempts(settings.clone()),
        log_bodies: log_bodies(&settings),
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
