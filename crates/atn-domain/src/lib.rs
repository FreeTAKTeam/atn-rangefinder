//! Pure geographic derivation and ephemeral, explicitly selected PLE synchronization.
//!
//! There is no calibrated default. Callers supply and attest the physical semantics,
//! observer source, eligibility thresholds and session limits. Protocol replies are
//! recorded as opaque evidence; a completed write never implies a rendered marker.

mod geo;
mod session;

pub use geo::*;
pub use session::*;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid configuration or input: {0}")]
    Invalid(&'static str),
    #[error("unverified physical semantics: {0}")]
    Unverified(&'static str),
    #[error("observer fix is required")]
    MissingObserver,
    #[error("{0} is stale")]
    Stale(&'static str),
    #[error("{0} timestamp is in the future")]
    Future(&'static str),
    #[error("observer accuracy exceeds the configured maximum")]
    InaccurateObserver,
    #[error("geodesic calculation did not converge")]
    Geodesic,
    #[error("{0} cannot be represented by the signed 16-bit protocol field")]
    NumericOverflow(&'static str),
    #[error("session is stopped")]
    SessionStopped,
    #[error("PLE is outside the selected set")]
    NotSelected,
    #[error("display slot does not belong to this selected PLE")]
    SlotOwnership,
    #[error("queue exceeds the configured bound")]
    QueueFull,
    #[error("operation or frame does not match the active transfer")]
    TransferMismatch,
    #[error("the active transfer has not completed transport")]
    TransportPending,
    #[error("there is no active transfer awaiting this reply")]
    UnexpectedReply,
    #[error("transfer or queued operation expired; session cancelled")]
    TimedOut,
    #[error("protocol encoding failed: {0}")]
    Protocol(String),
}

impl From<atn_protocol::ProtocolError> for DomainError {
    fn from(value: atn_protocol::ProtocolError) -> Self {
        Self::Protocol(value.to_string())
    }
}
