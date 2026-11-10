pub mod admin;
pub mod attempts;
pub mod crypto;
pub mod discovery;
pub mod observability;
pub mod protocol;
pub mod quota;
pub mod relay;
pub mod server;
pub mod store;

pub use crypto::{KeyStore, default_data_directory};
pub use store::{NewProvider, NewRoute, Provider, ProviderProtocol, Route, Store, UpsertClientKey};

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
}
