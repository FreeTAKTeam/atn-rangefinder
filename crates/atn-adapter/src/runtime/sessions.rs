//! REM operator-selected sessions (`map.session.*`) drive display authorization.
//! REM owns session identity and membership; this side owns slot allocation,
//! relative geometry and the one device-wide ATN write scheduler.
//!
//! Each accepted session keeps its own [`SyncSession`] (queue, slots, reports).
//! The scheduler here lets exactly one transfer, and one frame of it, be
//! outstanding across all sessions, because replies carry only a command byte.
use super::{
    Adapter, Display, ENDED_SESSION_BOUND, InFlight, MAX_DISPLAY_SESSIONS, MarkerStyle, OP_NOTIFY,
    OP_SESSION_LIST, OP_SESSION_OBJECTS, OP_SESSION_STOP, Purpose, RELEASED_SLOT_BOUND,
    ReleasedSlot, RemMapObject, SelectedPle, SessionConfig, SyncSession, TERMINAL_SESSION_BOUND,
    Value, WRITE_UUID, decode, gatt_uuid, integer, json, marker_for_ple, text,
};
use crate::icons::Affiliation;
use atn_domain::{DomainError, OperationKind};
use atn_protocol::OpaqueReply;
use std::collections::BTreeSet;

/// REM shows notification text in an Android notification; keep it short.
const NOTIFY_BODY_LIMIT: usize = 240;

impl Display {
    /// An icon whose first frame was handed off but whose last frame is not
    /// complete. Cancelling it leaves the device mid-transfer.
    fn partial_icon(&self) -> bool {
        self.sync
            .active_transfer()
            .is_some_and(|a| a.kind == OperationKind::Icon && !a.transport_complete)
    }
}

impl Adapter {
    fn is_terminal(&self, session_id: &str) -> bool {
        self.terminal_sessions.iter().any(|id| id == session_id)
    }

    fn mark_terminal(&mut self, session_id: &str) {
        if self.is_terminal(session_id) {
            return;
        }
        while self.terminal_sessions.len() >= TERMINAL_SESSION_BOUND {
            self.terminal_sessions.pop_front();
        }
        self.terminal_sessions.push_back(session_id.to_owned());
    }

    fn display_index(&self, session_id: &str) -> Option<usize> {
        self.displays
            .iter()
            .position(|d| d.sync.session_id() == session_id)
    }

    /// Configured slots not owned by any accepted session, in pool order.
    pub(super) fn free_slots(&self) -> Vec<u16> {
        let used: BTreeSet<u16> = self
            .displays
            .iter()
            .flat_map(|d| d.sync.snapshot().selected)
            .map(|item| item.slot_id)
            .collect();
        self.config
            .as_ref()
            .map(|c| {
                c.slot_pool
                    .iter()
                    .copied()
                    .filter(|slot| !used.contains(slot))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn release_slot(&mut self, slot_id: u16, session_id: &str, ple_id: &str, reason: &str) {
        while self.released_slots.len() >= RELEASED_SLOT_BOUND {
            self.released_slots.pop_front();
        }
        self.released_slots.push_back(ReleasedSlot {
            slot_id,
            session_id: session_id.to_owned(),
            ple_id: ple_id.to_owned(),
            reason: reason.to_owned(),
            device_display_cleared: false,
        });
    }

    /// Stops a REM session this plugin cannot or will no longer serve. The ID
    /// stays terminal whatever REM answers; REM sends no `stopped` event back.
    fn stop_on_host(&mut self, session_id: &str, now: u64, actions: &mut Vec<Value>) {
        self.mark_terminal(session_id);
        if !self.api_supported()
            || self.pending_purpose(|p| matches!(p, Purpose::SessionStop(id) if id == session_id))
        {
            return;
        }
        // A failed stop request is reported through status; the session is
        // already terminal locally and REM lists it with its own Stop control.
        if let Err(reason) = self.request(
            Purpose::SessionStop(session_id.to_owned()),
            OP_SESSION_STOP,
            json!({"sessionId":session_id}),
            now,
            actions,
        ) {
            actions.push(json!({"kind":"status","message":format!("could not ask REM to stop session: {reason}")}));
        }
    }

    /// Best-effort operator notification through REM. A refusal or failure is
    /// reported in status only and never cancels anything.
    fn notify(&mut self, body: &str, now: u64, actions: &mut Vec<Value>) {
        if !self.api_supported() {
            return;
        }
        let body: String = body.chars().take(NOTIFY_BODY_LIMIT).collect();
        if let Err(reason) = self.request(
            Purpose::Notify,
            OP_NOTIFY,
            json!({"title":"ATN Rangefinder","body":body}),
            now,
            actions,
        ) {
            actions.push(
                json!({"kind":"status","message":format!("notification not sent: {reason}")}),
            );
        }
    }

    /// Rejects one new session: nothing is allocated, existing sessions are
    /// untouched, REM is asked once to stop it and the ID stays terminal.
    fn reject_session(
        &mut self,
        session_id: &str,
        reason: &str,
        now: u64,
        actions: &mut Vec<Value>,
    ) {
        actions.push(json!({"kind":"status","message":format!("REM session {session_id} rejected: {reason}")}));
        self.notify(&format!("Share rejected: {reason}"), now, actions);
        self.stop_on_host(session_id, now, actions);
    }

    pub(super) fn request_session_list(
        &mut self,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        if self.pending_purpose(|p| matches!(p, Purpose::SessionList(_))) {
            return Ok(());
        }
        self.last_reconcile_ms = Some(now);
        self.request(
            Purpose::SessionList(now),
            OP_SESSION_LIST,
            json!({}),
            now,
            actions,
        )
    }

    /// Considers a session REM reports as active, from `map.session.started`
    /// or `map.session.list`. Allocation is atomic: either every selected
    /// object receives a free slot or this session alone is rejected.
    pub(super) fn consider_session(
        &mut self,
        session: &Value,
        objects: Option<Vec<RemMapObject>>,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let id = text(session, "sessionId")?.to_owned();
        let ids: Vec<String> = decode(session, "objectDestinationHashes")?;
        if self.is_terminal(&id) {
            return Ok(());
        }
        if self.display_index(&id).is_some() {
            // Duplicate `started`, or a listed session already served.
            if let Some(objects) = objects {
                self.apply_objects(&id, objects, now, actions)?;
            }
            return Ok(());
        }
        if self.displays.iter().filter(|d| !d.nearby).count() >= MAX_DISPLAY_SESSIONS {
            self.reject_session(&id, "REM session limit reached", now, actions);
            return Ok(());
        }
        if let Err(reason) = self.write_gate(now) {
            self.reject_session(&id, &reason, now, actions);
            return Ok(());
        }
        if let Some(reason) =
            self.start_display(&id, &ids, false, objects.is_none(), now, actions)?
        {
            self.reject_session(&id, &reason, now, actions);
            return Ok(());
        }
        match objects {
            Some(objects) => self.apply_objects(&id, objects, now, actions)?,
            None => self.request(
                Purpose::SessionObjects(id.clone()),
                OP_SESSION_OBJECTS,
                json!({"sessionId":id}),
                now,
                actions,
            )?,
        }
        self.refresh_observer(now, actions)
    }

    /// Allocates free slots in selection order and starts a display. Returns
    /// the refusal reason when the selection cannot be served; nothing is
    /// allocated then.
    pub(super) fn start_display(
        &mut self,
        id: &str,
        ids: &[String],
        nearby: bool,
        dirty: bool,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Option<String>, String> {
        if ids.is_empty() || ids.iter().any(|i| i.trim().is_empty()) {
            return Ok(Some("selection is empty".into()));
        }
        let free = self.free_slots();
        if ids.len() > free.len() {
            return Ok(Some("configured slot pool exhausted".into()));
        }
        let c = self.config.as_ref().ok_or("configuration required")?;
        let selected = ids
            .iter()
            .zip(free)
            .map(|(ple_id, slot_id)| SelectedPle {
                ple_id: ple_id.clone(),
                slot_id,
            })
            .collect();
        let config = SessionConfig {
            session_id: id.to_owned(),
            selected,
            queue_capacity: c.queue_capacity,
            max_queued_bytes: c.max_queued_bytes,
            queue_ttl_ms: c.queue_ttl_ms,
            operation_timeout_ms: c.operation_timeout_ms,
        };
        let sync = match SyncSession::start(config, now) {
            Ok(sync) => sync,
            Err(error) => return Ok(Some(error.to_string())),
        };
        self.displays.push(Display {
            sync,
            markers: std::collections::BTreeMap::new(),
            last_encoded: std::collections::BTreeMap::new(),
            accepted_at_ms: now,
            snapshot_at_ms: now,
            dirty,
            nearby,
            icons: std::collections::BTreeMap::new(),
        });
        actions.push(json!({"kind":"sessionAuthorized","sessionId":id,
            "connectionId":self.connection.as_ref().map(|(c,_)|c)}));
        Ok(None)
    }

    /// Reconciles against an authoritative `map.session.list`. Sessions
    /// accepted before the request and absent from it have ended without an
    /// event; stale or event-touched sessions get a fresh `objects.get`.
    pub(super) fn session_list_result(
        &mut self,
        items: &[Value],
        sent_at_ms: u64,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let listed: BTreeSet<&str> = items
            .iter()
            .filter_map(|s| s.get("sessionId").and_then(Value::as_str))
            .collect();
        let missing: Vec<String> = self
            .displays
            .iter()
            .filter(|d| {
                !d.nearby && d.accepted_at_ms <= sent_at_ms && !listed.contains(d.sync.session_id())
            })
            .map(|d| d.sync.session_id().to_owned())
            .collect();
        for id in missing {
            self.session_stopped(&id, "absent from REM session list", now, actions);
        }
        for session in items {
            self.consider_session(session, None, now, actions)?;
        }
        let interval = self
            .config
            .as_ref()
            .map_or(u64::MAX, |c| c.reconcile_interval_ms);
        let fetch: Vec<String> = self
            .displays
            .iter()
            .filter(|d| !d.nearby && listed.contains(d.sync.session_id()))
            .filter(|d| d.dirty || now.saturating_sub(d.snapshot_at_ms) >= interval)
            .map(|d| d.sync.session_id().to_owned())
            .collect();
        for id in fetch {
            if self.pending_purpose(|p| matches!(p, Purpose::SessionObjects(s) if *s == id)) {
                continue;
            }
            self.request(
                Purpose::SessionObjects(id.clone()),
                OP_SESSION_OBJECTS,
                json!({"sessionId":id}),
                now,
                actions,
            )?;
        }
        Ok(())
    }

    /// Lifecycle tick: per-session deadlines, then periodic reconciliation.
    pub(super) fn sessions_tick(
        &mut self,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let ids: Vec<(String, bool)> = self
            .displays
            .iter()
            .map(|d| (d.sync.session_id().to_owned(), d.partial_icon()))
            .collect();
        for (id, partial) in ids {
            let Some(index) = self.display_index(&id) else {
                continue;
            };
            if let Err(error) = self.displays[index].sync.tick(now) {
                self.end_display(&id, &error.to_string(), true, partial, now, actions);
            }
        }
        if !self.host_ready {
            return Ok(());
        }
        let interval = self
            .config
            .as_ref()
            .map_or(u64::MAX, |c| c.reconcile_interval_ms);
        if self
            .last_reconcile_ms
            .is_none_or(|last| now.saturating_sub(last) >= interval)
        {
            self.request_session_list(now, actions)?;
        }
        self.nearby_tick(now, actions)
    }

    /// Applies an authoritative snapshot of one session's objects. Objects REM
    /// omits no longer exist and leave the selection.
    pub(super) fn apply_objects(
        &mut self,
        session_id: &str,
        objects: Vec<RemMapObject>,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let Some(index) = self.display_index(session_id) else {
            return Ok(());
        };
        let selected = self.displays[index].sync.snapshot().selected;
        for item in selected {
            if !objects
                .iter()
                .any(|o| o.object_destination_hash == item.ple_id)
            {
                self.remove_object(session_id, &item.ple_id, now, actions)?;
            }
        }
        let Some(index) = self.display_index(session_id) else {
            return Ok(());
        };
        self.displays[index].snapshot_at_ms = now;
        self.displays[index].dirty = false;
        for object in objects {
            self.update_marker(session_id, object, now, actions)?;
        }
        Ok(())
    }

    pub(super) fn mark_dirty(&mut self, session_id: &str) -> bool {
        match self.display_index(session_id) {
            Some(index) => {
                self.displays[index].dirty = true;
                true
            }
            None => false,
        }
    }

    pub(super) fn remove_object(
        &mut self,
        session_id: &str,
        ple_id: &str,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let Some(index) = self.display_index(session_id) else {
            return Ok(());
        };
        let display = &mut self.displays[index];
        let Some(slot_id) = display.sync.slot_of(ple_id) else {
            return Ok(());
        };
        let partial = display.partial_icon()
            && display
                .sync
                .active_transfer()
                .is_some_and(|a| a.ple_id == ple_id);
        display
            .sync
            .remove_selected(ple_id)
            .map_err(|e| e.to_string())?;
        display.markers.remove(ple_id);
        display.icons.remove(ple_id);
        display.last_encoded.remove(ple_id);
        let remaining = display.sync.snapshot();
        self.release_slot(slot_id, session_id, ple_id, "object left the REM session");
        if !self.displays[index].sync.is_active() {
            let reason = match remaining.state {
                atn_domain::SessionState::Stopped { reason } => reason,
                atn_domain::SessionState::Active => "stopped".into(),
            };
            // An emptied selection is REM's own removal; otherwise REM still
            // shows the session and must be told it ended here.
            let tell_host = !remaining.selected.is_empty();
            self.end_display(session_id, &reason, tell_host, partial, now, actions);
        }
        Ok(())
    }

    /// Handles `map.session.stopped`, a session missing from the list, or a
    /// host answer proving the session is gone. No `map.session.stop` is sent.
    pub(super) fn session_stopped(
        &mut self,
        session_id: &str,
        reason: &str,
        now: u64,
        actions: &mut Vec<Value>,
    ) {
        self.mark_terminal(session_id);
        self.end_display(
            session_id,
            &format!("REM session stopped: {reason}"),
            false,
            false,
            now,
            actions,
        );
    }

    /// Removes one session: unsent work is cancelled, its slots are released
    /// (the device display is not cleared) and its ID becomes terminal. Returns
    /// whether a partial icon transfer was cut off.
    fn finish_display(
        &mut self,
        index: usize,
        reason: &str,
        tell_host: bool,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> bool {
        let mut display = self.displays.remove(index);
        if self.next_writer > index {
            self.next_writer -= 1;
        }
        let partial = display.partial_icon();
        if display.sync.is_active() {
            display.sync.stop(reason.to_owned());
        }
        let id = display.sync.session_id().to_owned();
        let snapshot = display.sync.snapshot();
        for item in &snapshot.selected {
            self.release_slot(item.slot_id, &id, &item.ple_id, reason);
        }
        while self.ended_sessions.len() >= ENDED_SESSION_BOUND {
            self.ended_sessions.pop_front();
        }
        self.ended_sessions.push_back(snapshot);
        if tell_host && !self.is_terminal(&id) {
            self.stop_on_host(&id, now, actions);
        } else {
            self.mark_terminal(&id);
        }
        // A frame of this session still in flight stays recorded in
        // `in_flight` until its echo or a disconnect, so no other session's
        // frame is issued over it.
        actions.push(json!({"kind":"sessionEnded","sessionId":id,"reason":reason}));
        partial
    }

    pub(super) fn end_display(
        &mut self,
        session_id: &str,
        reason: &str,
        tell_host: bool,
        was_partial: bool,
        now: u64,
        actions: &mut Vec<Value>,
    ) {
        let Some(index) = self.display_index(session_id) else {
            self.mark_terminal(session_id);
            return;
        };
        let partial = self.finish_display(index, reason, tell_host, now, actions) || was_partial;
        if partial {
            self.invalidate_transport(reason, now, actions);
        }
    }

    /// Ends every session (connection, host, configuration or local failure).
    pub(super) fn end_all_displays(
        &mut self,
        reason: &str,
        tell_host: bool,
        now: u64,
        actions: &mut Vec<Value>,
    ) {
        let mut partial = false;
        while !self.displays.is_empty() {
            partial |= self.finish_display(0, reason, tell_host, now, actions);
        }
        self.next_writer = 0;
        if partial {
            self.invalidate_transport(reason, now, actions);
        }
    }

    /// A cancelled partial icon transfer leaves the device mid-protocol: GATT
    /// is closed, every session and allocation ends, and only an explicit
    /// reconnect followed by new Share actions can authorize writes again.
    fn invalidate_transport(&mut self, reason: &str, now: u64, actions: &mut Vec<Value>) {
        let reason = format!("partial icon transfer cancelled: {reason}");
        while !self.displays.is_empty() {
            self.finish_display(0, &reason, true, now, actions);
        }
        self.next_writer = 0;
        self.in_flight = None;
        self.connection = None;
        self.colocation = None;
        self.observer = None;
        actions.push(json!({"kind":"cancelWrites"}));
        actions.push(json!({"kind":"disconnect"}));
        actions.push(
            json!({"kind":"status","message":format!("{reason}; reconnect and share again")}),
        );
    }

    pub(super) fn update_marker(
        &mut self,
        session_id: &str,
        marker: RemMapObject,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let Some(index) = self.display_index(session_id) else {
            return Ok(());
        };
        let display = &mut self.displays[index];
        let id = marker.object_destination_hash.clone();
        if display.sync.slot_of(&id).is_none() {
            return Ok(());
        }
        let updated = marker.updated_at_ms().map_err(|e| e.to_string())?;
        if let Some(old) = display.markers.get(&id) {
            // Older host records are refused; same-time records are compared by content.
            if old.updated_at_ms().map_err(|e| e.to_string())? > updated {
                return Err("stale REM object update".into());
            }
            if *old == marker {
                return Ok(());
            }
        }
        marker
            .as_ple()
            .map_err(|e| e.to_string())?
            .position
            .validate()
            .map_err(|e| e.to_string())?;
        display.markers.insert(id, marker);
        self.refresh_selected(Some(session_id), now, actions)
    }

    /// Queues position frames whose encoding changed, for one session or all.
    pub(super) fn refresh_selected(
        &mut self,
        only: Option<&str>,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        if self.displays.is_empty() {
            return Ok(());
        }
        self.write_gate(now)?;
        if self.observer_valid(now).is_err() {
            return self.pump(now, actions);
        }
        let c = self.config.as_ref().ok_or("configuration required")?;
        let observer = self.observer.as_ref();
        let capacity = self.connection.as_ref().map(|(_, capacity)| *capacity);
        let mut failed = Vec::new();
        let mut icon_problems = Vec::new();
        for display in &mut self.displays {
            if only.is_some_and(|id| id != display.sync.session_id()) {
                continue;
            }
            for item in display.sync.snapshot().selected {
                let Some(record) = display.markers.get(&item.ple_id) else {
                    continue;
                };
                let ple = record.as_ple().map_err(|e| e.to_string())?;
                // NATO affiliation colours; generic symbols keep the configured colour (blue).
                let affiliation = Affiliation::of_symbol(record.symbol.as_deref());
                let style = MarkerStyle {
                    slot_id: item.slot_id,
                    marker_type: c.marker_type,
                    color: if affiliation == Affiliation::Generic {
                        c.color
                    } else {
                        affiliation.binox_color()
                    },
                    enabled: true,
                };
                let marker = marker_for_ple(
                    &ple,
                    observer,
                    &c.semantics,
                    &c.eligibility,
                    &style,
                    c.pitch_policy,
                    now,
                )
                .map_err(|e| e.to_string())?;
                let bytes = marker.encode().to_vec();
                if display.last_encoded.get(&item.ple_id) != Some(&bytes) {
                    if let Err(error) = display.sync.enqueue_position(&item.ple_id, &marker, now) {
                        failed.push((
                            display.sync.session_id().to_owned(),
                            error.to_string(),
                            display.partial_icon(),
                        ));
                        break;
                    }
                    display.last_encoded.insert(item.ple_id.clone(), bytes);
                }
                // The icon follows the first position, and again when the symbol changes.
                if c.send_icons && display.icons.get(&item.ple_id) != Some(&affiliation) {
                    display.icons.insert(item.ple_id.clone(), affiliation);
                    let queued = capacity
                        .ok_or_else(|| "BLE disconnected".to_owned())
                        .and_then(|capacity| {
                            let png = affiliation.icon_png()?;
                            display
                                .sync
                                .enqueue_icon(&item.ple_id, &png, capacity, c.png_limits, now)
                                .map_err(|e| e.to_string())
                        });
                    if let Err(error) = queued {
                        icon_problems.push(format!("icon not sent: {error}"));
                    }
                }
            }
        }
        for (id, reason, partial) in failed {
            self.end_display(&id, &reason, true, partial, now, actions);
        }
        for problem in icon_problems {
            actions.push(json!({"kind":"status","message":problem}));
        }
        self.pump(now, actions)
    }

    /// Issues at most one frame device-wide. The session owning the active
    /// transfer continues it (or waits for its reply); otherwise sessions with
    /// queued work take turns.
    pub(super) fn pump(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        if self.displays.is_empty() || self.in_flight.is_some() {
            return Ok(());
        }
        self.write_gate(now)?;
        if let Err(reason) = self.observer_valid(now) {
            // Hold rather than send relative geometry from an ineligible fix.
            // A specific reason from REM's last answer stays on screen.
            if self.observer_problem.is_none() {
                self.observer_report(&format!("holding display writes: {reason}"), actions);
            }
            return self.refresh_observer(now, actions);
        }
        let count = self.displays.len();
        let owner = self
            .displays
            .iter()
            .position(|d| d.sync.active_transfer().is_some());
        let order: Vec<usize> = match owner {
            Some(index) => vec![index],
            None => (0..count)
                .map(|k| (self.next_writer + k) % count)
                .filter(|&i| self.displays[i].sync.queued_operations() > 0)
                .collect(),
        };
        let Some(&index) = order.first() else {
            return Ok(());
        };
        let partial = self.displays[index].partial_icon();
        match self.displays[index].sync.next_write(now) {
            Ok(Some(frame)) => {
                let session_id = self.displays[index].sync.session_id().to_owned();
                if owner.is_none() {
                    self.next_writer = (index + 1) % count;
                }
                self.in_flight = Some(InFlight {
                    session_id: session_id.clone(),
                    operation_id: frame.operation_id,
                    frame_number: frame.frame_number,
                });
                actions.push(json!({"kind":"write","operationId":frame.operation_id,
                    "frameNumber":frame.frame_number,"totalFrames":frame.total_frames,"bytes":frame.bytes,
                    "slotId":frame.slot_id,"sessionId":session_id,
                    "connectionId":self.connection.as_ref().map(|(id,_)|id)}));
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(error) => {
                let id = self.displays[index].sync.session_id().to_owned();
                self.end_display(&id, &error.to_string(), true, partial, now, actions);
                self.pump(now, actions)
            }
        }
    }

    /// Platform completion of the in-flight frame. Only an echo matching the
    /// in-flight connection, session, operation and frame is accepted; an echo
    /// for an ended session frees the device but advances nothing.
    pub(super) fn transport_result(
        &mut self,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let connection_matches = self.connection.as_ref().is_some_and(|(id, _)| {
            v.get("connectionId").and_then(Value::as_str) == Some(id.as_str())
        });
        let echo = InFlight {
            session_id: v
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            operation_id: integer(v, "operationId")?,
            frame_number: usize::try_from(integer(v, "frameNumber")?).map_err(|e| e.to_string())?,
        };
        if !connection_matches
            || !gatt_uuid(v, "characteristic", WRITE_UUID)
            || self.in_flight.as_ref() != Some(&echo)
        {
            return Ok(json!({"ignored":"stale transport callback"}));
        }
        self.in_flight = None;
        let completed = v
            .get("completed")
            .and_then(Value::as_bool)
            .ok_or("completed boolean required")?;
        let Some(index) = self.display_index(&echo.session_id) else {
            self.pump(now, actions)?;
            return Ok(json!({"ignored":"transport callback for ended session"}));
        };
        if !completed {
            // Typed platform error, e.g. `gattStatus`, `writeRejected`, `timeout`.
            let code = v
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("unspecified");
            return Err(format!(
                "transport failed ({code}); explicit new session required"
            ));
        }
        let report = self.displays[index]
            .sync
            .transport_completed(echo.operation_id, echo.frame_number, now)
            .map_err(|e| e.to_string())?;
        self.pump(now, actions)?;
        Ok(json!({"sessionId":echo.session_id,"report":report}))
    }

    /// A reply belongs to the single active transfer device-wide, if any.
    pub(super) fn route_reply(
        &mut self,
        reply: OpaqueReply,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let Some(index) = self
            .displays
            .iter()
            .position(|d| d.sync.active_transfer().is_some())
        else {
            return Ok(json!({"reply":reply,"associated":false}));
        };
        let id = self.displays[index].sync.session_id().to_owned();
        let partial = self.displays[index].partial_icon();
        let echo = reply.clone();
        match self.displays[index].sync.observe_reply(reply, now) {
            Ok(report) => {
                self.pump(now, actions)?;
                Ok(json!({"sessionId":id,"report":report}))
            }
            Err(
                DomainError::UnexpectedReply
                | DomainError::TransportPending
                | DomainError::SessionStopped,
            ) => Ok(json!({"reply":echo,"associated":false})),
            Err(error) => {
                self.end_display(&id, &error.to_string(), true, partial, now, actions);
                Ok(json!({"reply":echo,"associated":false}))
            }
        }
    }

    /// Operator-chosen icon for one object of one session.
    pub(super) fn set_icon(
        &mut self,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        self.write_gate(now)?;
        let session_id = text(v, "sessionId")?;
        let ple_id = text(v, "pleId")?;
        let bytes: Vec<u8> = decode(v, "png")?;
        let c = self.config.as_ref().ok_or("configuration required")?;
        let limits = c.png_limits;
        let capacity = self.connection.as_ref().ok_or("BLE disconnected")?.1;
        let Some(index) = self.display_index(session_id) else {
            return Ok(json!({"accepted":false,"reason":"no such active session"}));
        };
        match self.displays[index]
            .sync
            .enqueue_icon(ple_id, &bytes, capacity, limits, now)
        {
            Ok(id) => {
                self.pump(now, actions)?;
                Ok(json!({"accepted":true,"sessionId":session_id,"operationId":id}))
            }
            // A mistyped object leaves every session untouched.
            Err(DomainError::NotSelected) => {
                Ok(json!({"accepted":false,"reason":"object not selected in that session"}))
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub(super) fn confirm_display(&mut self, v: &Value) -> Result<Value, String> {
        let session_id = text(v, "sessionId")?;
        let id = integer(v, "operationId")?;
        let evidence = text(v, "evidenceId")?.to_owned();
        let Some(index) = self.display_index(session_id) else {
            return Ok(json!({"accepted":false,"reason":"no such active session"}));
        };
        match self.displays[index].sync.confirm_display(id, evidence) {
            Ok(report) => Ok(json!({"accepted":true,"sessionId":session_id,"report":report})),
            Err(error) => Ok(json!({"accepted":false,"reason":error.to_string()})),
        }
    }
}
