//! Side-effect-free codecs for the provisional protocol in `docs/protocol.md`.
//!
//! Numeric fields do not establish meters, degrees, north reference, calibration,
//! location, or device validity. Replies preserve their undocumented status and
//! tail; transport completion is not evidence of device rendering.

mod icon;
mod png;

pub use icon::{
    IconFragment, IconReassembler, ReassemblyLimits, ReassemblyProgress, fragment_payload,
    fragment_png,
};
pub use png::{PngInfo, PngLimits, validate_png};

use serde::{Deserialize, Serialize};
use std::fmt;

pub const SIGNATURE: u8 = 1;
pub const MEASUREMENT_COMMAND: u8 = 1;
pub const MARKER_COMMAND: u8 = 2;
pub const ICON_COMMAND: u8 = 7;
pub const ICON_HEADER_BYTES: usize = 14;
/// Local diagnostic bound, not a firmware capability claim.
pub const MAX_INBOUND_BYTES: usize = 514;
pub const DECODER_VERSION: &str = "atn-protocol/0.1.0";
pub const SERVICE_UUID: &str = "d973f2e0-b19e-11e2-9e96-0800200c9a66";
pub const NOTIFICATION_UUID: &str = "d973f2e1-b19e-11e2-9e96-0800200c9a66";
pub const WRITE_UUID: &str = "d973f2e2-b19e-11e2-9e96-0800200c9a66";
pub const CCCD_UUID: &str = "00002902-0000-1000-8000-00805f9b34fb";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawObservation {
    pub distance: i16,
    pub pitch: i16,
    pub compass: i16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpaqueReply {
    pub command: u8,
    pub answer_signed: i8,
    pub opaque_tail: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownFrame {
    pub command: u8,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboundFrame {
    Measurement(RawObservation),
    Reply(OpaqueReply),
    Unknown(UnknownFrame),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    TooShort {
        minimum: usize,
        actual: usize,
    },
    UnsupportedSignature(u8),
    UnsupportedLength {
        command: u8,
        expected: usize,
        actual: usize,
    },
    InvalidField(&'static str),
    LimitExceeded(&'static str),
    InconsistentTransfer,
    ConflictingDuplicate(u16),
    IncompleteTransfer {
        expected: usize,
        received: usize,
    },
    InvalidPng(&'static str),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ProtocolError {}

/// Parses exactly the supported eight-byte measurement or six-byte reply.
/// Valid-signature unknown commands are retained as diagnostics, never coerced.
///
/// # Errors
/// Rejects unsupported signatures, malformed supported frame lengths, and
/// notifications beyond the local diagnostic bound.
pub fn parse_inbound(bytes: &[u8]) -> Result<InboundFrame, ProtocolError> {
    if bytes.len() > MAX_INBOUND_BYTES {
        return Err(ProtocolError::LimitExceeded("inbound bytes"));
    }
    if bytes.len() < 2 {
        return Err(ProtocolError::TooShort {
            minimum: 2,
            actual: bytes.len(),
        });
    }
    if bytes[0] != SIGNATURE {
        return Err(ProtocolError::UnsupportedSignature(bytes[0]));
    }
    let command = bytes[1];
    match command {
        MEASUREMENT_COMMAND => {
            exact_length(bytes, 8)?;
            Ok(InboundFrame::Measurement(RawObservation {
                distance: i16::from_be_bytes([bytes[2], bytes[3]]),
                pitch: i16::from_be_bytes([bytes[4], bytes[5]]),
                compass: i16::from_be_bytes([bytes[6], bytes[7]]),
            }))
        }
        MARKER_COMMAND | ICON_COMMAND => {
            exact_length(bytes, 6)?;
            Ok(InboundFrame::Reply(OpaqueReply {
                command,
                answer_signed: i8::from_ne_bytes([bytes[2]]),
                opaque_tail: [bytes[3], bytes[4], bytes[5]],
            }))
        }
        _ => Ok(InboundFrame::Unknown(UnknownFrame {
            command,
            bytes: bytes.to_vec(),
        })),
    }
}

fn exact_length(bytes: &[u8], expected: usize) -> Result<(), ProtocolError> {
    if bytes.len() < expected {
        Err(ProtocolError::TooShort {
            minimum: expected,
            actual: bytes.len(),
        })
    } else if bytes.len() != expected {
        Err(ProtocolError::UnsupportedLength {
            command: bytes[1],
            expected,
            actual: bytes.len(),
        })
    } else {
        Ok(())
    }
}

/// Validated representable command-2 fields. Bounds are encoding bounds only;
/// firmware slot counts and permitted physical values remain unverified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayMarker {
    slot_id: u8,
    marker_type: u8,
    enabled: bool,
    color: u8,
    distance: i16,
    pitch: i16,
    compass: i16,
}

impl DisplayMarker {
    /// Constructs a marker without integer truncation or implied physical units.
    ///
    /// # Errors
    /// Rejects slots beyond seven bits, unsupported type/color, and numeric
    /// fields that cannot be represented as signed 16-bit values.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slot_id: u16,
        marker_type: u8,
        enabled: bool,
        color: u8,
        distance: i32,
        pitch: i32,
        compass: i32,
    ) -> Result<Self, ProtocolError> {
        if slot_id > 127 {
            return Err(ProtocolError::InvalidField("slot_id must fit seven bits"));
        }
        if marker_type > 1 {
            return Err(ProtocolError::InvalidField("marker_type must be 0 or 1"));
        }
        if color > 7 {
            return Err(ProtocolError::InvalidField("color must be 0 through 7"));
        }
        Ok(Self {
            slot_id: u8::try_from(slot_id).map_err(|_| ProtocolError::InvalidField("slot_id"))?,
            marker_type,
            enabled,
            color,
            distance: i16::try_from(distance)
                .map_err(|_| ProtocolError::InvalidField("distance must fit signed 16 bits"))?,
            pitch: i16::try_from(pitch)
                .map_err(|_| ProtocolError::InvalidField("pitch must fit signed 16 bits"))?,
            compass: i16::try_from(compass)
                .map_err(|_| ProtocolError::InvalidField("compass must fit signed 16 bits"))?,
        })
    }

    #[must_use]
    pub fn encode(&self) -> [u8; 10] {
        let mut bytes = [
            SIGNATURE,
            MARKER_COMMAND,
            (self.slot_id << 1) | self.marker_type,
            (u8::from(self.enabled) << 4) | self.color,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        bytes[4..6].copy_from_slice(&self.distance.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.pitch.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.compass.to_le_bytes());
        bytes
    }

    #[must_use]
    pub fn slot_id(&self) -> u16 {
        u16::from(self.slot_id)
    }
    #[must_use]
    pub fn marker_type(&self) -> u8 {
        self.marker_type
    }
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    #[must_use]
    pub fn color(&self) -> u8 {
        self.color
    }
    #[must_use]
    pub fn distance(&self) -> i16 {
        self.distance
    }
    #[must_use]
    pub fn pitch(&self) -> i16 {
        self.pitch
    }
    #[must_use]
    pub fn compass(&self) -> i16 {
        self.compass
    }
}
