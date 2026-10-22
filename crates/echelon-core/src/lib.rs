pub mod auth;
pub mod autonomy;
pub mod bootstrap;
pub mod broker;
pub mod config;
pub mod console;
pub mod context;
pub mod credentials;
pub mod error;
pub mod execution;
pub mod ingestion;
pub mod knowledge;
pub mod maintenance;
pub mod mcp;
pub mod observability;
pub mod persistence;
pub mod policy;
pub mod protocol;
pub mod provider;
pub mod retention;
pub mod server;
pub mod store;
pub mod ui_auth;
pub mod work;
pub mod worker;
pub mod workspace;

pub fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_secs()
}
