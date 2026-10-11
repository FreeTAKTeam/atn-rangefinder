//! RCH marker compatibility, inspected at 2a599275ee36ea809dbd5ff773ebda6bf9d2a8d1.
//! No RCH storage, normalization, networking, or identity authority is recreated here.
use atn_domain::{DomainError, GeographicPosition, PlePoint};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RchMarker {
    pub local_id: String,
    pub object_destination_hash: String,
    pub origin_rch: String,
    pub marker_type: String,
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub lat: f64,
    pub lon: f64,
    pub notes: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

impl RchMarker {
    /// # Errors
    /// Rejects missing marker identity or invalid host timestamps.
    pub fn as_ple(&self) -> Result<PlePoint, DomainError> {
        // Host identifiers are retained exactly. Timestamps are not a monotonic
        // concurrency token; the adapter compares full successive records.
        if self.object_destination_hash.is_empty() || self.updated_ts_ms < 0 {
            return Err(DomainError::Invalid(
                "invalid RCH marker identity or timestamp",
            ));
        }
        Ok(PlePoint {
            id: self.object_destination_hash.clone(),
            revision: self.updated_ts_ms.to_string(),
            position: GeographicPosition {
                latitude: self.lat,
                longitude: self.lon,
                altitude_m: None,
            },
            metadata: json!({"rch": self}),
        })
    }
}
