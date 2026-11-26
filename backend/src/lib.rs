pub mod admin;
pub mod ai;
pub mod attempts;
pub mod auxiliary;
pub mod bootstrap;
pub(crate) mod client_ip;
pub mod controls;
pub mod crypto;
pub mod discovery;
pub mod events;
pub mod lifecycle;
pub mod metadata;
pub mod observability;
pub mod plugin;
pub mod policy;
pub mod probe;
pub mod protocol;
pub mod quota;
pub mod relay;
pub mod response_api;
pub mod response_store;
pub mod responses;
pub mod rewrite;
pub mod self_update;
pub mod server;
pub mod settings;
pub mod snapshot;
pub mod store;
pub mod strategy;
pub mod updates;
pub mod webauthn;

pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub use crypto::{KeyStore, default_data_directory};
pub use store::{
    NewProvider, NewRoute, Provider, ProviderKeyInfo, ProviderProtocol, Route, Store,
    UpsertClientKey,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientKey {
    pub id: i64,
    pub name: String,
    pub models: Vec<String>,
    pub token_limit: i64,
    pub max_amount_micros: i64,
    pub rpm: i64,
    pub tpm: i64,
    pub expires_at: Option<String>,
    pub enabled: bool,
}
