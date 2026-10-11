//! Bounded JSON runtime for Android's trusted REM/BLE boundary.
mod config;
mod native;
mod rch;
mod runtime;
pub use config::AdapterConfig;
pub use rch::RchMarker;
pub use runtime::Adapter;
