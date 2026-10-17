use std::collections::{BTreeMap, BTreeSet, VecDeque};

use atn_protocol::{DisplayMarker, OpaqueReply, PngLimits, fragment_png};
use serde::{Deserialize, Serialize};

use crate::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedPle {
    pub ple_id: String,
    pub slot_id: u16,
}

/// No default or persistence: each instance represents one explicit authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionConfig {
    pub session_id: String,
    pub selected: Vec<SelectedPle>,
    pub queue_capacity: usize,
    pub max_queued_bytes: usize,
    pub queue_ttl_ms: u64,
    pub operation_timeout_ms: u64,
}

impl SessionConfig {
    /// # Errors
    /// Rejects absent identity/selection, nonpositive bounds and ambiguous slots.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.session_id.trim().is_empty() || self.selected.is_empty() {
            return Err(DomainError::Invalid(
                "session identity and selected PLEs required",
            ));
        }
        if self.queue_capacity == 0
            || self.max_queued_bytes == 0
            || self.queue_ttl_ms == 0
            || self.operation_timeout_ms == 0
        {
            return Err(DomainError::Invalid("session bounds must be positive"));
        }
        let mut ids = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for selected in &self.selected {
            if selected.ple_id.trim().is_empty() || !ids.insert(&selected.ple_id) {
                return Err(DomainError::Invalid("duplicate or empty selected PLE"));
            }
            // Packed (slot << 1) + marker_type has exactly seven slot bits.
            if selected.slot_id > 127 || !slots.insert(selected.slot_id) {
                return Err(DomainError::Invalid(
                    "duplicate or unrepresentable owned slot",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Position,
    Icon,
}

impl OperationKind {
    const fn command(self) -> u8 {
        match self {
            Self::Position => 2,
            Self::Icon => 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFrame {
    pub operation_id: u64,
    pub ple_id: String,
    pub slot_id: u16,
    pub kind: OperationKind,
    /// One-based frame number; host completion must echo the same number.
    pub frame_number: usize,
    pub total_frames: usize,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReport {
    pub operation_id: u64,
    pub ple_id: String,
    pub slot_id: u16,
    pub kind: OperationKind,
    pub transport_complete: bool,
    pub reply: Option<OpaqueReply>,
    /// Explicit observed rendering evidence, independent of transport or reply.
    pub display_confirmation: Option<String>,
    pub cancelled: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SessionState {
    Active,
    Stopped { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub session_id: String,
    pub state: SessionState,
    pub selected: Vec<SelectedPle>,
    pub queued_operations: usize,
    pub queued_bytes: usize,
    pub active: Option<TransferReport>,
    /// Retained reports are bounded by `queue_capacity`. External observers can
    /// retain returned reports privately if a longer audit is required.
    pub completed: Vec<TransferReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedSelection {
    pub ple_id: String,
    pub slot_id: u16,
    /// Removal releases authorization and does not issue a device clear command.
    pub device_marker_cleared: bool,
}

struct QueuedOperation {
    report: TransferReport,
    frames: Vec<Vec<u8>>,
    expires_at_ms: u64,
    total_bytes: usize,
}

struct ActiveTransfer {
    operation: QueuedOperation,
    completed_frames: usize,
    awaiting_transport: bool,
    deadline_ms: u64,
}

/// Serialized writes, finite queue and no automatic retry or reconnect behavior.
pub struct SyncSession {
    config: SessionConfig,
    selected: BTreeMap<String, u16>,
    state: SessionState,
    queue: VecDeque<QueuedOperation>,
    active: Option<ActiveTransfer>,
    reports: VecDeque<TransferReport>,
    queued_bytes: usize,
    next_operation_id: u64,
    last_time_ms: u64,
}

impl SyncSession {
    /// # Errors
    /// Rejects invalid authorization configuration and overflowing deadlines.
    pub fn start(config: SessionConfig, now_ms: u64) -> Result<Self, DomainError> {
        config.validate()?;
        checked_deadline(now_ms, config.queue_ttl_ms)?;
        checked_deadline(now_ms, config.operation_timeout_ms)?;
        let selected = config
            .selected
            .iter()
            .map(|item| (item.ple_id.clone(), item.slot_id))
            .collect();
        Ok(Self {
            config,
            selected,
            state: SessionState::Active,
            queue: VecDeque::new(),
            active: None,
            reports: VecDeque::new(),
            queued_bytes: 0,
            next_operation_id: 1,
            last_time_ms: now_ms,
        })
    }

    /// # Errors
    /// Rejects stopped/expired sessions, unauthorized PLEs/slots and full queues.
    /// Replaces an unsent position for this PLE with the newest relative values;
    /// active transfers and separately queued icons retain their ordering.
    pub fn enqueue_position(
        &mut self,
        ple_id: &str,
        marker: &DisplayMarker,
        now_ms: u64,
    ) -> Result<u64, DomainError> {
        self.check(now_ms)?;
        self.check_owner(ple_id, marker.slot_id())?;
        self.enqueue(
            ple_id,
            marker.slot_id(),
            OperationKind::Position,
            vec![marker.encode().to_vec()],
            now_ms,
        )
    }

    /// # Errors
    /// Rejects stopped/expired sessions, unauthorized PLEs, invalid PNGs/write
    /// capacity, overflowing deadlines and full queues.
    pub fn enqueue_icon(
        &mut self,
        ple_id: &str,
        png: &[u8],
        write_capacity: usize,
        png_limits: PngLimits,
        now_ms: u64,
    ) -> Result<u64, DomainError> {
        self.check(now_ms)?;
        let slot_id = *self.selected.get(ple_id).ok_or(DomainError::NotSelected)?;
        let frames = fragment_png(png, slot_id, write_capacity, png_limits)?;
        self.enqueue(ple_id, slot_id, OperationKind::Icon, frames, now_ms)
    }

    fn check_owner(&self, ple_id: &str, slot_id: u16) -> Result<(), DomainError> {
        match self.selected.get(ple_id) {
            None => Err(DomainError::NotSelected),
            Some(owned) if *owned != slot_id => Err(DomainError::SlotOwnership),
            Some(_) => Ok(()),
        }
    }

    fn enqueue(
        &mut self,
        ple_id: &str,
        slot_id: u16,
        kind: OperationKind,
        frames: Vec<Vec<u8>>,
        now_ms: u64,
    ) -> Result<u64, DomainError> {
        let total_bytes = frames
            .iter()
            .try_fold(0_usize, |total, frame| total.checked_add(frame.len()))
            .ok_or(DomainError::QueueFull)?;
        let mut superseded_bytes = 0_usize;
        let mut superseded_count = 0_usize;
        if kind == OperationKind::Position {
            for operation in &self.queue {
                if operation.report.kind == OperationKind::Position
                    && operation.report.ple_id == ple_id
                {
                    superseded_bytes = superseded_bytes
                        .checked_add(operation.total_bytes)
                        .ok_or(DomainError::QueueFull)?;
                    superseded_count += 1;
                }
            }
        }
        let queued_bytes = self
            .queued_bytes
            .checked_sub(superseded_bytes)
            .and_then(|bytes| bytes.checked_add(total_bytes))
            .ok_or(DomainError::QueueFull)?;
        if self.queue.len() - superseded_count + usize::from(self.active.is_some())
            >= self.config.queue_capacity
            || queued_bytes > self.config.max_queued_bytes
        {
            return Err(DomainError::QueueFull);
        }
        let expires_at_ms = checked_deadline(now_ms, self.config.queue_ttl_ms)?;
        let operation_id = self.next_operation_id;
        self.next_operation_id = operation_id
            .checked_add(1)
            .ok_or(DomainError::Invalid("operation identity exhausted"))?;
        let mut replacement = Some(QueuedOperation {
            report: TransferReport {
                operation_id,
                ple_id: ple_id.to_owned(),
                slot_id,
                kind,
                transport_complete: false,
                reply: None,
                display_confirmation: None,
                cancelled: None,
            },
            frames,
            expires_at_ms,
            total_bytes,
        });
        if superseded_count > 0 {
            let mut remaining = VecDeque::new();
            while let Some(mut operation) = self.queue.pop_front() {
                if operation.report.kind == OperationKind::Position
                    && operation.report.ple_id == ple_id
                {
                    operation.report.cancelled = Some("superseded by newer position".to_owned());
                    self.retain_report(operation.report);
                    if let Some(newer) = replacement.take() {
                        remaining.push_back(newer);
                    }
                } else {
                    remaining.push_back(operation);
                }
            }
            self.queue = remaining;
        }
        if let Some(newer) = replacement {
            self.queue.push_back(newer);
        }
        self.queued_bytes = queued_bytes;
        Ok(operation_id)
    }

    /// Return at most one frame. Further writes wait for explicit completion;
    /// after the final frame the application reply remains a separate gate.
    ///
    /// # Errors
    /// Rejects stopped/expired sessions, clock rollback and overflowing deadlines.
    pub fn next_write(&mut self, now_ms: u64) -> Result<Option<WriteFrame>, DomainError> {
        self.check(now_ms)?;
        if self.active.is_none() {
            if let Some(operation) = self.queue.pop_front() {
                self.active = Some(ActiveTransfer {
                    operation,
                    completed_frames: 0,
                    awaiting_transport: false,
                    deadline_ms: checked_deadline(now_ms, self.config.operation_timeout_ms)?,
                });
            }
        }
        let Some(active) = self.active.as_mut() else {
            return Ok(None);
        };
        if active.awaiting_transport || active.operation.report.transport_complete {
            return Ok(None);
        }
        let frame = WriteFrame {
            operation_id: active.operation.report.operation_id,
            ple_id: active.operation.report.ple_id.clone(),
            slot_id: active.operation.report.slot_id,
            kind: active.operation.report.kind,
            frame_number: active.completed_frames + 1,
            total_frames: active.operation.frames.len(),
            bytes: active.operation.frames[active.completed_frames].clone(),
        };
        active.awaiting_transport = true;
        Ok(Some(frame))
    }

    /// # Errors
    /// Rejects expired/stopped sessions and wrong or duplicate frame completions.
    pub fn transport_completed(
        &mut self,
        operation_id: u64,
        frame_number: usize,
        now_ms: u64,
    ) -> Result<TransferReport, DomainError> {
        self.check(now_ms)?;
        let active = self.active.as_mut().ok_or(DomainError::TransferMismatch)?;
        if active.operation.report.operation_id != operation_id
            || !active.awaiting_transport
            || frame_number != active.completed_frames + 1
        {
            return Err(DomainError::TransferMismatch);
        }
        active.completed_frames += 1;
        active.awaiting_transport = false;
        active.operation.report.transport_complete =
            active.completed_frames == active.operation.frames.len();
        let report = active.operation.report.clone();
        if report.transport_complete && report.reply.is_some() {
            self.complete_active()?;
        }
        Ok(report)
    }

    /// Correlation uses the command byte only, because no wire transaction ID
    /// is established. Serialization limits ambiguity; reply status is opaque.
    /// A reply preceding the final host callback is retained independently.
    ///
    /// # Errors
    /// Rejects expired/stopped sessions, wrong commands and responses before the
    /// final frame has even been handed to the transport.
    pub fn observe_reply(
        &mut self,
        reply: OpaqueReply,
        now_ms: u64,
    ) -> Result<TransferReport, DomainError> {
        self.check(now_ms)?;
        let active = self.active.as_mut().ok_or(DomainError::UnexpectedReply)?;
        if active.operation.report.kind.command() != reply.command {
            return Err(DomainError::UnexpectedReply);
        }
        if !(active.operation.report.transport_complete
            || active.awaiting_transport
                && active.completed_frames + 1 == active.operation.frames.len())
        {
            return Err(DomainError::TransportPending);
        }
        active.operation.report.reply = Some(reply);
        let report = active.operation.report.clone();
        if report.transport_complete {
            self.complete_active()?;
        }
        Ok(report)
    }

    fn complete_active(&mut self) -> Result<(), DomainError> {
        let active = self.active.take().ok_or(DomainError::UnexpectedReply)?;
        self.queued_bytes -= active.operation.total_bytes;
        self.retain_report(active.operation.report);
        Ok(())
    }

    /// # Errors
    /// Rejects empty evidence or an unknown/evicted operation identity.
    pub fn confirm_display(
        &mut self,
        operation_id: u64,
        evidence_id: String,
    ) -> Result<TransferReport, DomainError> {
        if evidence_id.trim().is_empty() {
            return Err(DomainError::Invalid("display confirmation evidence"));
        }
        if let Some(active) = self.active.as_mut() {
            if active.operation.report.operation_id == operation_id {
                active.operation.report.display_confirmation = Some(evidence_id);
                return Ok(active.operation.report.clone());
            }
        }
        let report = self
            .reports
            .iter_mut()
            .find(|report| report.operation_id == operation_id)
            .ok_or(DomainError::TransferMismatch)?;
        report.display_confirmation = Some(evidence_id);
        Ok(report.clone())
    }

    /// Removing a selected PLE never clears other markers and never expands
    /// authorization. A partially transferred removed point ends this session.
    ///
    /// # Errors
    /// Rejects stopped sessions and PLEs outside the selected set.
    pub fn remove_selected(&mut self, ple_id: &str) -> Result<RemovedSelection, DomainError> {
        if !matches!(self.state, SessionState::Active) {
            return Err(DomainError::SessionStopped);
        }
        let slot_id = self
            .selected
            .remove(ple_id)
            .ok_or(DomainError::NotSelected)?;
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.operation.report.ple_id == ple_id)
        {
            self.stop("selected PLE removed during active transfer".to_owned());
        } else {
            let mut remaining = VecDeque::new();
            while let Some(mut operation) = self.queue.pop_front() {
                if operation.report.ple_id == ple_id {
                    self.queued_bytes -= operation.total_bytes;
                    operation.report.cancelled = Some("selected PLE removed".to_owned());
                    self.retain_report(operation.report);
                } else {
                    remaining.push_back(operation);
                }
            }
            self.queue = remaining;
            if self.selected.is_empty() {
                self.stop("selected set is empty".to_owned());
            }
        }
        Ok(RemovedSelection {
            ple_id: ple_id.to_owned(),
            slot_id,
            device_marker_cleared: false,
        })
    }

    pub fn stop(&mut self, reason: String) {
        if let Some(mut active) = self.active.take() {
            active.operation.report.cancelled = Some(reason.clone());
            self.retain_report(active.operation.report);
        }
        while let Some(mut operation) = self.queue.pop_front() {
            operation.report.cancelled = Some(reason.clone());
            self.retain_report(operation.report);
        }
        self.queued_bytes = 0;
        self.state = SessionState::Stopped { reason };
    }

    pub fn disconnect(&mut self) {
        self.stop("device disconnected; new authorization required".to_owned());
    }

    /// The transfer that currently owns this session's write slot, if any. A
    /// scheduler serving several sessions over one device uses this to keep a
    /// single transfer (and its reply) outstanding device-wide.
    #[must_use]
    pub fn active_transfer(&self) -> Option<&TransferReport> {
        self.active.as_ref().map(|active| &active.operation.report)
    }

    #[must_use]
    pub fn queued_operations(&self) -> usize {
        self.queue.len()
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self.state, SessionState::Active)
    }

    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.config.session_id
    }

    /// Slot owned by a selected PLE.
    #[must_use]
    pub fn slot_of(&self, ple_id: &str) -> Option<u16> {
        self.selected.get(ple_id).copied()
    }

    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            session_id: self.config.session_id.clone(),
            state: self.state.clone(),
            selected: self
                .selected
                .iter()
                .map(|(id, slot)| SelectedPle {
                    ple_id: id.clone(),
                    slot_id: *slot,
                })
                .collect(),
            queued_operations: self.queue.len(),
            queued_bytes: self.queued_bytes,
            active: self
                .active
                .as_ref()
                .map(|active| active.operation.report.clone()),
            completed: self.reports.iter().cloned().collect(),
        }
    }

    /// Polling from the lifecycle timer cancels expired operations even when
    /// there are no further BLE callbacks.
    ///
    /// # Errors
    /// Rejects expired/stopped sessions and backward-moving session time.
    pub fn tick(&mut self, now_ms: u64) -> Result<(), DomainError> {
        self.check(now_ms)
    }

    fn check(&mut self, now_ms: u64) -> Result<(), DomainError> {
        if !matches!(self.state, SessionState::Active) {
            return Err(DomainError::SessionStopped);
        }
        if now_ms < self.last_time_ms {
            self.stop("session clock moved backwards".to_owned());
            return Err(DomainError::Invalid("session time moved backwards"));
        }
        self.last_time_ms = now_ms;
        let expired = self
            .queue
            .iter()
            .any(|operation| now_ms >= operation.expires_at_ms)
            || self.active.as_ref().is_some_and(|active| {
                now_ms >= active.deadline_ms || now_ms >= active.operation.expires_at_ms
            });
        if expired {
            self.stop("transfer or queued operation timed out; no retry".to_owned());
            return Err(DomainError::TimedOut);
        }
        Ok(())
    }

    fn retain_report(&mut self, report: TransferReport) {
        if self.reports.len() >= self.config.queue_capacity {
            self.reports.pop_front();
        }
        self.reports.push_back(report);
    }
}

fn checked_deadline(now_ms: u64, duration_ms: u64) -> Result<u64, DomainError> {
    now_ms
        .checked_add(duration_ms)
        .ok_or(DomainError::Invalid("deadline timestamp overflow"))
}
