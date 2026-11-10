use crate::{Provider, Route, Store};
use arc_swap::ArcSwap;
use std::sync::Arc;

#[derive(Default)]
pub struct ConfigSnapshot {
    pub providers: Vec<Provider>,
    pub routes: Vec<Route>,
}
pub struct Snapshots(ArcSwap<ConfigSnapshot>);
impl Snapshots {
    pub async fn load(store: &Store) -> Result<Self, String> {
        Ok(Self(ArcSwap::from_pointee(ConfigSnapshot {
            providers: store.providers().await?,
            routes: store.routes().await?,
        })))
    }
    pub fn get(&self) -> Arc<ConfigSnapshot> {
        self.0.load_full()
    }
    pub async fn refresh(&self, store: &Store) -> Result<(), String> {
        self.0.store(Arc::new(ConfigSnapshot {
            providers: store.providers().await?,
            routes: store.routes().await?,
        }));
        Ok(())
    }
}
