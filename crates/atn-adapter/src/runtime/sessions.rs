//! REM operator-selected sessions (`map.session.*`) drive display authorization.
//! REM owns session identity and membership; this side owns slot allocation,
//! relative geometry and the serialized ATN write queue.
use super::{
    Adapter, MarkerStyle, OP_SESSION_OBJECTS, OP_SESSION_STOP, Purpose, RemMapObject, SelectedPle,
    SessionConfig, SyncSession, TERMINAL_SESSION_BOUND, Value, decode, json, marker_for_ple, text,
};

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

    /// Ends the active local session on REM too, after the local cancellation.
    pub(super) fn end_session_on_host(&mut self, now: u64, actions: &mut Vec<Value>) {
        if let Some(id) = self.session.as_ref().map(|s| s.snapshot().session_id) {
            if !self.is_terminal(&id) {
                self.stop_on_host(&id, now, actions);
            }
        }
    }

    fn reject_session(
        &mut self,
        session_id: &str,
        reason: &str,
        now: u64,
        actions: &mut Vec<Value>,
    ) {
        actions.push(json!({"kind":"status","message":format!("REM session {session_id} rejected: {reason}")}));
        self.stop_on_host(session_id, now, actions);
    }

    /// Considers a session REM reports as active, from `map.session.started`
    /// or `map.session.list`. Allocation is atomic: either every selected
    /// object receives a slot or the whole session is rejected and stopped.
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
        if let Some(active) = self.active_session_id() {
            if active == id {
                if let Some(objects) = objects {
                    self.apply_objects(&id, objects, now, actions)?;
                }
                return Ok(());
            }
            self.reject_session(&id, "another display session is active", now, actions);
            return Ok(());
        }
        if let Err(reason) = self.write_gate(now) {
            self.reject_session(&id, &reason, now, actions);
            return Ok(());
        }
        let c = self.config.as_ref().ok_or("configuration required")?;
        if ids.is_empty() || ids.iter().any(|i| i.trim().is_empty()) {
            self.reject_session(&id, "selection is empty", now, actions);
            return Ok(());
        }
        if ids.len() > c.slot_pool.len() {
            self.reject_session(&id, "configured slot pool exhausted", now, actions);
            return Ok(());
        }
        let selected = ids
            .iter()
            .zip(&c.slot_pool)
            .map(|(ple_id, slot)| SelectedPle {
                ple_id: ple_id.clone(),
                slot_id: *slot,
            })
            .collect();
        let config = SessionConfig {
            session_id: id.clone(),
            selected,
            queue_capacity: c.queue_capacity,
            max_queued_bytes: c.max_queued_bytes,
            queue_ttl_ms: c.queue_ttl_ms,
            operation_timeout_ms: c.operation_timeout_ms,
        };
        let session = match SyncSession::start(config, now) {
            Ok(session) => session,
            Err(error) => {
                self.reject_session(&id, &error.to_string(), now, actions);
                return Ok(());
            }
        };
        self.session = Some(session);
        self.markers.clear();
        self.last_encoded.clear();
        actions.push(json!({"kind":"cancelWrites"}));
        actions.push(json!({"kind":"sessionAuthorized","sessionId":id,
            "connectionId":self.connection.as_ref().map(|(c,_)|c)}));
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

    /// Applies an authoritative snapshot of the session's objects. Objects REM
    /// omits no longer exist and leave the selection.
    pub(super) fn apply_objects(
        &mut self,
        session_id: &str,
        objects: Vec<RemMapObject>,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        if self.active_session_id().as_deref() != Some(session_id) {
            return Ok(());
        }
        let selected = self
            .session
            .as_ref()
            .map(|s| s.snapshot().selected)
            .unwrap_or_default();
        for item in selected {
            if !objects
                .iter()
                .any(|o| o.object_destination_hash == item.ple_id)
            {
                self.remove_object(&item.ple_id, actions)?;
            }
        }
        for object in objects {
            self.update_marker(object, now, actions)?;
        }
        Ok(())
    }

    pub(super) fn remove_object(
        &mut self,
        id: &str,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        if !session.snapshot().selected.iter().any(|p| p.ple_id == id) {
            return Ok(());
        }
        session.remove_selected(id).map_err(|e| e.to_string())?;
        self.markers.remove(id);
        self.last_encoded.remove(id);
        if self.active_session_id().is_none() {
            actions.push(json!({"kind":"cancelWrites"}));
        }
        Ok(())
    }

    /// Handles `map.session.stopped` or a host answer proving the session is gone.
    pub(super) fn session_stopped(
        &mut self,
        session_id: &str,
        reason: &str,
        actions: &mut Vec<Value>,
    ) {
        self.mark_terminal(session_id);
        if self
            .session
            .as_ref()
            .map(|s| s.snapshot().session_id)
            .as_deref()
            == Some(session_id)
        {
            self.cancel_session(&format!("REM session stopped: {reason}"));
            actions.push(json!({"kind":"cancelWrites"}));
        }
    }

    pub(super) fn update_marker(
        &mut self,
        marker: RemMapObject,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let id = marker.object_destination_hash.clone();
        if !self
            .session
            .as_ref()
            .is_some_and(|s| s.snapshot().selected.iter().any(|p| p.ple_id == id))
        {
            return Ok(());
        }
        let updated = marker.updated_at_ms().map_err(|e| e.to_string())?;
        if let Some(old) = self.markers.get(&id) {
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
        self.markers.insert(id, marker);
        self.refresh_selected(now, actions)
    }

    pub(super) fn refresh_selected(
        &mut self,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        if self.active_session_id().is_none() {
            return Ok(());
        }
        self.write_gate(now)?;
        if self.observer_valid(now).is_err() {
            return self.pump(now, actions);
        }
        let selected = self
            .session
            .as_ref()
            .map(|s| s.snapshot().selected)
            .unwrap_or_default();
        let c = self.config.as_ref().ok_or("configuration required")?;
        for item in selected {
            let Some(record) = self.markers.get(&item.ple_id) else {
                continue;
            };
            let ple = record.as_ple().map_err(|e| e.to_string())?;
            let style = MarkerStyle {
                slot_id: item.slot_id,
                marker_type: c.marker_type,
                color: c.color,
                enabled: true,
            };
            let marker = marker_for_ple(
                &ple,
                self.observer.as_ref(),
                &c.semantics,
                &c.eligibility,
                &style,
                c.pitch_policy,
                now,
            )
            .map_err(|e| e.to_string())?;
            let bytes = marker.encode().to_vec();
            if self.last_encoded.get(&item.ple_id) == Some(&bytes) {
                continue;
            }
            self.session
                .as_mut()
                .ok_or("session required")?
                .enqueue_position(&item.ple_id, &marker, now)
                .map_err(|e| e.to_string())?;
            self.last_encoded.insert(item.ple_id, bytes);
        }
        self.pump(now, actions)
    }
}
