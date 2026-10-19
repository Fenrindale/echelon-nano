pub mod auth;
pub mod bootstrap;
pub mod config;
pub mod error;
pub mod policy;
pub mod server;
pub mod store;

pub fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_secs()
}
