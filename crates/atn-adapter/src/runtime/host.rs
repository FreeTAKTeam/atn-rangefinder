use super::{
    Adapter, MarkerStyle, OPERATIONS, ObserverFix, Purpose, RchMarker, Value, decode, json,
    marker_for_ple, text,
};

impl Adapter {
    pub(super) fn host_response(
        &mut self,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let envelope = v.get("envelope").ok_or("response envelope required")?;
        if envelope.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
            return Err("unsupported host envelope version".into());
        }
        let id = text(envelope, "requestId")?;
        // Ignore old-session or duplicate callbacks; never interpret as new work.
        let Some(pending) = self.pending.remove(id) else {
            return Ok(json!({"ignored":"unmatched host response"}));
        };
        if now >= pending.deadline_ms {
            return Err("expired host response".into());
        }
        if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
            self.host_ready = false;
            return Err("host rejected map request; adapter remains read-only".into());
        }
        let result = envelope.get("result").ok_or("host result required")?;
        match pending.purpose {
            Purpose::Describe => {
                if result.get("contractVersion").and_then(Value::as_u64) != Some(1) {
                    return Err("unsupported map contract".into());
                }
                let operations: Vec<String> = decode(result, "operations")?;
                if !OPERATIONS
                    .iter()
                    .all(|required| operations.iter().any(|item| item == required))
                {
                    return Err("incomplete map operations; adapter remains read-only".into());
                }
                self.host_ready = true;
            }
            Purpose::Observer | Purpose::Measurement(_) => {
                let fix: ObserverFix = decode(result, "observer")?;
                self.accept_observer(fix, now)?;
                if let Purpose::Measurement(id) = pending.purpose {
                    self.publish_draft(id, now, actions)?;
                } else {
                    self.refresh_selected(now, actions)?;
                }
            }
            Purpose::Marker(expected) => {
                let marker: RchMarker = decode(result, "marker")?;
                if marker.object_destination_hash != expected {
                    return Err("host returned a different marker".into());
                }
                self.update_marker(marker, now, actions)?;
            }
            Purpose::Subscribe => {
                if result.get("accepted").and_then(Value::as_bool) != Some(true) {
                    return Err("selected PLE subscription not accepted".into());
                }
                self.subscribed = true;
                self.refresh_selected(now, actions)?;
            }
            Purpose::Draft | Purpose::Unsubscribe => {
                if result.get("accepted").and_then(Value::as_bool) != Some(true) {
                    return Err("draft not retained by host".into());
                }
            }
        }
        Ok(self.status())
    }

    pub(super) fn host_event(
        &mut self,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        if !self.host_ready {
            return Ok(json!({"ignored":"map host unavailable"}));
        }
        let envelope = v.get("envelope").ok_or("event envelope required")?;
        if envelope.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
            return Err("unsupported host event version".into());
        }
        let event = text(envelope, "event")?;
        let payload = envelope.get("payload").ok_or("event payload required")?;
        match event {
            "map.ple.updated" => {
                // The event is session-scoped, so an old subscription cannot add
                // points to a later authorization of an overlapping selected set.
                if !self.event_session_matches(payload) {
                    return Ok(json!({"ignored":"different session"}));
                }
                self.update_marker(decode(payload, "marker")?, now, actions)?;
            }
            "map.ple.removed" => {
                if !self.event_session_matches(payload) {
                    return Ok(json!({"ignored":"different session"}));
                }
                let id = text(payload, "pleId")?;
                self.session
                    .as_mut()
                    .ok_or("session required")?
                    .remove_selected(id)
                    .map_err(|e| e.to_string())?;
                self.markers.remove(id);
                self.last_encoded.remove(id);
                if self.session.as_ref().is_some_and(|s| {
                    !matches!(s.snapshot().state, atn_domain::SessionState::Active)
                }) {
                    self.subscribed = false;
                    actions.push(json!({"kind":"cancelWrites"}));
                }
            }
            "map.observer.updated" => {
                if !self.event_session_matches(payload) {
                    return Ok(json!({"ignored":"different session"}));
                }
                self.accept_observer(decode(payload, "observer")?, now)?;
                self.refresh_selected(now, actions)?;
            }
            _ => return Ok(json!({"ignored":"unrelated event"})),
        }
        Ok(self.status())
    }

    fn accept_observer(&mut self, fix: ObserverFix, now: u64) -> Result<bool, String> {
        fix.position.validate().map_err(|e| e.to_string())?;
        if let Some(previous) = &self.observer {
            if fix.captured_at_ms < previous.captured_at_ms {
                return Ok(false);
            }
            if fix.captured_at_ms == previous.captured_at_ms && fix != *previous {
                return Err("conflicting observer fix timestamp".into());
            }
        }
        let c = self.config.as_ref().ok_or("configuration required")?;
        if fix.validate(&c.eligibility, now).is_err() {
            return Ok(false);
        }
        self.observer = Some(fix);
        Ok(true)
    }

    fn event_session_matches(&self, payload: &Value) -> bool {
        self.subscribed
            && self.session.as_ref().is_some_and(|session| {
                payload.get("sessionId").and_then(Value::as_str)
                    == Some(session.snapshot().session_id.as_str())
            })
    }

    fn update_marker(
        &mut self,
        marker: RchMarker,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let id = &marker.object_destination_hash;
        if !self
            .session
            .as_ref()
            .is_some_and(|s| s.snapshot().selected.iter().any(|p| &p.ple_id == id))
        {
            return Ok(());
        }
        if self
            .markers
            .get(id)
            .is_some_and(|old| old.updated_ts_ms > marker.updated_ts_ms)
        {
            return Err("stale RCH marker event".into());
        }
        marker
            .as_ple()
            .map_err(|e| e.to_string())?
            .position
            .validate()
            .map_err(|e| e.to_string())?;
        self.markers.insert(id.clone(), marker);
        self.refresh_selected(now, actions)
    }

    pub(super) fn refresh_selected(
        &mut self,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        if !self.subscribed || self.observer.is_none() {
            return Ok(());
        }
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };
        if !matches!(session.snapshot().state, atn_domain::SessionState::Active) {
            return Ok(());
        }
        self.write_gate(now)?;
        let selected = session.snapshot().selected;
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
