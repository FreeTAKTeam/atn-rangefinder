use crate::{ICON_COMMAND, ICON_HEADER_BYTES, PngLimits, ProtocolError, SIGNATURE, validate_png};

/// A header-validated fragment borrowing the caller's frame buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconFragment<'a> {
    pub total_frames: u16,
    pub total_bytes: u16,
    pub frame_number: u16,
    pub slot_id: u16,
    /// The documented encoder supplies zero. No checksum meaning is inferred.
    pub crc_named_field: u16,
    pub payload: &'a [u8],
}

impl<'a> IconFragment<'a> {
    /// Parses one complete command-7 ATT value.
    ///
    /// # Errors
    /// Rejects malformed signature/command, truncated or inconsistent lengths,
    /// impossible frame counts/numbers, and unsupported nonzero CRC fields.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < ICON_HEADER_BYTES {
            return Err(ProtocolError::TooShort {
                minimum: ICON_HEADER_BYTES,
                actual: bytes.len(),
            });
        }
        if bytes[0] != SIGNATURE {
            return Err(ProtocolError::UnsupportedSignature(bytes[0]));
        }
        if bytes[1] != ICON_COMMAND {
            return Err(ProtocolError::InvalidField("icon command"));
        }
        let field = |index| u16::from_le_bytes([bytes[index], bytes[index + 1]]);
        let fragment = Self {
            total_frames: field(2),
            total_bytes: field(4),
            frame_number: field(6),
            slot_id: field(10),
            crc_named_field: field(12),
            payload: &bytes[ICON_HEADER_BYTES..],
        };
        if fragment.total_frames == 0
            || fragment.total_bytes == 0
            || fragment.total_frames > fragment.total_bytes
        {
            return Err(ProtocolError::InvalidField("empty or impossible transfer"));
        }
        if fragment.frame_number == 0 || fragment.frame_number > fragment.total_frames {
            return Err(ProtocolError::InvalidField("frame number"));
        }
        if field(8) == 0 || usize::from(field(8)) != fragment.payload.len() {
            return Err(ProtocolError::InvalidField("fragment byte count"));
        }
        if fragment.payload.len() > usize::from(fragment.total_bytes) {
            return Err(ProtocolError::InvalidField("fragment larger than transfer"));
        }
        if fragment.crc_named_field != 0 {
            return Err(ProtocolError::InvalidField(
                "unsupported nonzero CRC-named field",
            ));
        }
        Ok(fragment)
    }
}

/// Fragments bytes using actual writable ATT-value capacity, including the header.
/// This codec primitive accepts opaque synthetic bytes; `fragment_png` performs
/// image validation. Empty bytes yield no frames. No empty final frame is emitted.
///
/// # Errors
/// Rejects a capacity too small for payload or more than 65,535 total bytes.
pub fn fragment_payload(
    bytes: &[u8],
    slot_id: u16,
    write_capacity: usize,
) -> Result<Vec<Vec<u8>>, ProtocolError> {
    let payload_capacity = write_capacity
        .checked_sub(ICON_HEADER_BYTES)
        .filter(|capacity| *capacity > 0)
        .ok_or(ProtocolError::InvalidField(
            "write capacity must exceed icon header",
        ))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let total_bytes =
        u16::try_from(bytes.len()).map_err(|_| ProtocolError::LimitExceeded("icon bytes"))?;
    let total_frames = u16::try_from(bytes.len().div_ceil(payload_capacity))
        .map_err(|_| ProtocolError::LimitExceeded("icon frames"))?;
    let mut frames = Vec::with_capacity(usize::from(total_frames));
    for (index, payload) in bytes.chunks(payload_capacity).enumerate() {
        let frame_number =
            u16::try_from(index + 1).map_err(|_| ProtocolError::LimitExceeded("frame number"))?;
        let payload_len = u16::try_from(payload.len())
            .map_err(|_| ProtocolError::LimitExceeded("fragment bytes"))?;
        let mut frame = Vec::with_capacity(ICON_HEADER_BYTES + payload.len());
        frame.extend_from_slice(&[SIGNATURE, ICON_COMMAND]);
        for value in [
            total_frames,
            total_bytes,
            frame_number,
            payload_len,
            slot_id,
            0,
        ] {
            frame.extend_from_slice(&value.to_le_bytes());
        }
        frame.extend_from_slice(payload);
        frames.push(frame);
    }
    Ok(frames)
}

/// Validates a supported PNG before fragmenting it for an actual write capacity.
///
/// # Errors
/// Returns PNG validation or fragmentation bound/capacity errors.
pub fn fragment_png(
    bytes: &[u8],
    slot_id: u16,
    write_capacity: usize,
    limits: PngLimits,
) -> Result<Vec<Vec<u8>>, ProtocolError> {
    validate_png(bytes, limits)?;
    fragment_payload(bytes, slot_id, write_capacity)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReassemblyLimits {
    pub max_total_bytes: usize,
    pub max_frames: usize,
    pub max_frame_bytes: usize,
}

impl Default for ReassemblyLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: usize::from(u16::MAX),
            max_frames: 4096,
            max_frame_bytes: 514,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReassemblyProgress {
    Accepted {
        received_frames: usize,
        total_frames: usize,
    },
    Duplicate {
        frame_number: u16,
    },
    Complete,
}

#[derive(Debug)]
struct Transfer {
    total_bytes: usize,
    slot_id: u16,
    parts: Vec<Option<Vec<u8>>>,
    received_bytes: usize,
    received_frames: usize,
}

/// One bounded transfer. Out-of-order frames are accepted, byte-identical
/// duplicates are idempotent, conflicting duplicates are rejected. A caller
/// owns lifecycle/timeouts and discards this object on cancellation/disconnect.
#[derive(Debug)]
pub struct IconReassembler {
    limits: ReassemblyLimits,
    transfer: Option<Transfer>,
}

impl IconReassembler {
    #[must_use]
    pub fn new(limits: ReassemblyLimits) -> Self {
        Self {
            limits,
            transfer: None,
        }
    }

    /// Accepts an unordered fragment without replacing existing transfer data.
    ///
    /// # Errors
    /// Rejects malformed headers, configured memory bounds, mixed transfers,
    /// conflicting duplicates, and impossible aggregate byte counts.
    pub fn push(&mut self, bytes: &[u8]) -> Result<ReassemblyProgress, ProtocolError> {
        if bytes.len() > self.limits.max_frame_bytes {
            return Err(ProtocolError::LimitExceeded("frame bytes"));
        }
        let fragment = IconFragment::parse(bytes)?;
        let total_bytes = usize::from(fragment.total_bytes);
        let total_frames = usize::from(fragment.total_frames);
        if total_bytes > self.limits.max_total_bytes {
            return Err(ProtocolError::LimitExceeded("transfer bytes"));
        }
        if total_frames > self.limits.max_frames {
            return Err(ProtocolError::LimitExceeded("transfer frames"));
        }
        if self.transfer.is_none() {
            self.transfer = Some(Transfer {
                total_bytes,
                slot_id: fragment.slot_id,
                parts: vec![None; total_frames],
                received_bytes: 0,
                received_frames: 0,
            });
        }
        let transfer = self
            .transfer
            .as_mut()
            .ok_or(ProtocolError::InvalidField("transfer state"))?;
        if total_bytes != transfer.total_bytes
            || total_frames != transfer.parts.len()
            || fragment.slot_id != transfer.slot_id
        {
            return Err(ProtocolError::InconsistentTransfer);
        }
        let index = usize::from(fragment.frame_number) - 1;
        if let Some(existing) = &transfer.parts[index] {
            if existing != fragment.payload {
                return Err(ProtocolError::ConflictingDuplicate(fragment.frame_number));
            }
            return Ok(ReassemblyProgress::Duplicate {
                frame_number: fragment.frame_number,
            });
        }
        let new_bytes = transfer
            .received_bytes
            .checked_add(fragment.payload.len())
            .ok_or(ProtocolError::LimitExceeded("transfer bytes"))?;
        if new_bytes > transfer.total_bytes {
            return Err(ProtocolError::InvalidField(
                "fragments exceed declared total",
            ));
        }
        // Every remaining frame must carry at least one byte.
        let remaining_frames = transfer.parts.len() - transfer.received_frames - 1;
        if new_bytes + remaining_frames > transfer.total_bytes {
            return Err(ProtocolError::InvalidField("impossible remaining transfer"));
        }
        if remaining_frames == 0 && new_bytes != transfer.total_bytes {
            return Err(ProtocolError::InvalidField("completed byte count"));
        }
        transfer.parts[index] = Some(fragment.payload.to_vec());
        transfer.received_bytes = new_bytes;
        transfer.received_frames += 1;
        if remaining_frames == 0 {
            Ok(ReassemblyProgress::Complete)
        } else {
            Ok(ReassemblyProgress::Accepted {
                received_frames: transfer.received_frames,
                total_frames: transfer.parts.len(),
            })
        }
    }

    /// Returns complete original bytes in frame-number order.
    ///
    /// # Errors
    /// Rejects an empty/incomplete transfer or aggregate byte-count mismatch.
    pub fn finish(self) -> Result<Vec<u8>, ProtocolError> {
        let transfer = self.transfer.ok_or(ProtocolError::IncompleteTransfer {
            expected: 1,
            received: 0,
        })?;
        if transfer.received_frames != transfer.parts.len() {
            return Err(ProtocolError::IncompleteTransfer {
                expected: transfer.parts.len(),
                received: transfer.received_frames,
            });
        }
        if transfer.received_bytes != transfer.total_bytes {
            return Err(ProtocolError::InvalidField("completed byte count"));
        }
        let mut bytes = Vec::with_capacity(transfer.total_bytes);
        for part in transfer.parts {
            bytes.extend_from_slice(&part.ok_or(ProtocolError::InvalidField("missing fragment"))?);
        }
        Ok(bytes)
    }

    /// Returns complete original bytes after validating the supported PNG subset.
    ///
    /// # Errors
    /// Returns incomplete transfer, byte-count, or PNG validation errors.
    pub fn finish_png(self, limits: PngLimits) -> Result<Vec<u8>, ProtocolError> {
        let bytes = self.finish()?;
        validate_png(&bytes, limits)?;
        Ok(bytes)
    }
}
