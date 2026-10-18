//! Bounded JSON runtime for Android's trusted REM/BLE boundary.
mod config;
pub mod icons;
mod native;
mod rch;
mod runtime;
pub use config::AdapterConfig;
pub use rch::{MapPosition, RemMapObject, rfc3339_to_unix_ms};
pub use runtime::Adapter;
