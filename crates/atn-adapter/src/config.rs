use atn_domain::{EligibilityPolicy, MarkerPitchPolicy, SemanticsPolicy};
use atn_protocol::PngLimits;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceProfile {
    pub model: String,
    pub firmware: String,
    pub evidence_id: String,
    pub protocol_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransportConfig {
    pub request_mtu: u16,
    pub connect_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub host_request_timeout_ms: u64,
    pub max_pending_host_requests: usize,
    pub max_queued_writes: usize,
    pub scan_timeout_ms: u64,
}

/// How eligible observations become ordinary shared REM markers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostMarkerPolicy {
    /// A REM symbol catalog `id` or alias, checked against `map.symbols.list`.
    pub symbol: String,
    pub category: Option<String>,
    /// Marker name prefix; the observation identity is appended.
    pub name_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdapterConfig {
    pub device_profile: DeviceProfile,
    pub host_marker: HostMarkerPolicy,
    /// Ordered, operator-verified device display slots available to REM
    /// selection sessions. Allocation follows REM's selection order.
    pub slot_pool: Vec<u16>,
    pub semantics: SemanticsPolicy,
    pub eligibility: EligibilityPolicy,
    pub pitch_policy: MarkerPitchPolicy,
    pub marker_type: u8,
    pub color: u8,
    pub queue_capacity: usize,
    pub max_queued_bytes: usize,
    pub queue_ttl_ms: u64,
    pub operation_timeout_ms: u64,
    pub max_observations: usize,
    pub transport: TransportConfig,
    pub png_limits: PngLimits,
}

impl AdapterConfig {
    /// # Errors
    /// Rejects missing, zero, nonfinite, or unrepresentable configuration.
    pub fn validate(&self) -> Result<(), String> {
        self.semantics
            .validate_configuration()
            .map_err(|e| e.to_string())?;
        self.eligibility.validate().map_err(|e| e.to_string())?;
        let t = &self.transport;
        let marker = &self.host_marker;
        if marker.symbol.trim().is_empty()
            || !marker.symbol.bytes().all(|b| b.is_ascii_graphic())
            || marker.symbol.len() > 64
            || marker.name_prefix.trim().is_empty()
            || marker.name_prefix.chars().count() > 64
            || marker
                .category
                .as_ref()
                .is_some_and(|c| c.trim().is_empty() || c.chars().count() > 64)
        {
            return Err(
                "host marker symbol, category and name prefix must be short nonempty text".into(),
            );
        }
        let mut slots = std::collections::BTreeSet::new();
        if self.slot_pool.is_empty()
            || self.slot_pool.len() > 128
            || !self
                .slot_pool
                .iter()
                .all(|slot| *slot <= 127 && slots.insert(*slot))
        {
            return Err("slot pool must list unique seven-bit display slots".into());
        }
        if self.marker_type > 1 || self.color > 7 || !(23..=517).contains(&t.request_mtu) {
            return Err("unrepresentable marker fields or Bluetooth MTU".into());
        }
        if [
            self.queue_capacity,
            self.max_queued_bytes,
            self.max_observations,
            t.max_pending_host_requests,
            t.max_queued_writes,
            self.png_limits.max_encoded_bytes,
            self.png_limits.max_decoded_bytes,
            self.png_limits.max_chunks,
        ]
        .contains(&0)
            || [
                self.queue_ttl_ms,
                self.operation_timeout_ms,
                t.connect_timeout_ms,
                t.write_timeout_ms,
                t.host_request_timeout_ms,
                t.scan_timeout_ms,
            ]
            .contains(&0)
            || self.png_limits.max_width == 0
            || self.png_limits.max_height == 0
            || self.png_limits.max_pixels == 0
        {
            return Err("all resource bounds and timeouts must be positive".into());
        }
        // Implementation ceilings bound allocation and Java scheduling; they
        // are not firmware support claims or operational default policies.
        if t.max_pending_host_requests > 4096
            || t.max_queued_writes > 4096
            || [
                t.connect_timeout_ms,
                t.write_timeout_ms,
                t.host_request_timeout_ms,
                t.scan_timeout_ms,
            ]
            .iter()
            .any(|v| *v > 3_600_000)
            || self.queue_capacity > 4096
            || self.max_observations > 4096
            || self.max_queued_bytes > 16 * 1024 * 1024
            || self.png_limits.max_encoded_bytes > usize::from(u16::MAX)
            || self.png_limits.max_decoded_bytes > 16 * 1024 * 1024
            || self.png_limits.max_pixels > 4 * 1024 * 1024
        {
            return Err("configuration exceeds implementation resource ceiling".into());
        }
        if let Some((width, height)) = self.png_limits.expected_dimensions {
            if width == 0
                || height == 0
                || width > self.png_limits.max_width
                || height > self.png_limits.max_height
                || u64::from(width) * u64::from(height) > self.png_limits.max_pixels
            {
                return Err("inconsistent PNG dimensions and limits".into());
            }
        }
        if self.device_profile.model.trim().is_empty()
            || self.device_profile.firmware.trim().is_empty()
        {
            return Err("explicit device model and firmware profile required".into());
        }
        if self.device_profile.protocol_verified
            && self.device_profile.evidence_id.trim().is_empty()
        {
            return Err("verified device profile requires evidence".into());
        }
        Ok(())
    }
}
