use super::{
    Adapter, HostOutcome, OP_SESSION_LIST, ObserverFix, Purpose, RemMapObject, Value, decode, json,
    text,
};
use atn_domain::{GeographicPosition, ObserverSource};

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
            return self.host_failure(
                pending.purpose,
                "expired",
                "host response after deadline",
                actions,
            );
        }
        if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = envelope.get("error").cloned().unwrap_or(Value::Null);
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            return self.host_failure(pending.purpose, &code, &message, actions);
        }
        let result = envelope.get("result").ok_or("host result required")?;
        self.host_success(pending.purpose, result, now, actions)?;
        Ok(self.status())
    }

    fn host_success(
        &mut self,
        purpose: Purpose,
        result: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        match purpose {
            Purpose::Symbols => {
                let c = self.config.as_ref().ok_or("configuration required")?;
                let wanted = c.host_marker.symbol.as_str();
                let items = result
                    .get("items")
                    .and_then(Value::as_array)
                    .ok_or("symbol catalog items required")?;
                let known = items.iter().any(|item| {
                    item.get("id").and_then(Value::as_str) == Some(wanted)
                        || item
                            .get("aliases")
                            .and_then(Value::as_array)
                            .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(wanted)))
                });
                if !known {
                    return Err("configured marker symbol is not in the REM catalog".into());
                }
                self.host_ready = true;
                self.request(
                    Purpose::SessionList,
                    OP_SESSION_LIST,
                    json!({}),
                    now,
                    actions,
                )?;
            }
            Purpose::Observer => match self.observer_from_snapshot(result) {
                Ok(fix) => {
                    self.accept_observer(fix, now)?;
                    self.refresh_selected(now, actions)?;
                }
                Err(reason) => {
                    actions.push(json!({"kind":"status","message":format!("observer fix ineligible: {reason}")}));
                }
            },
            Purpose::Measurement(id) => {
                let fix = match self.observer_from_snapshot(result) {
                    Ok(fix) => fix,
                    Err(reason) => {
                        self.set_outcome(id, HostOutcome::Retained { reason });
                        return Ok(());
                    }
                };
                let c = self.config.as_ref().ok_or("configuration required")?;
                if let Err(error) = fix.validate(&c.eligibility, now) {
                    self.set_outcome(
                        id,
                        HostOutcome::Retained {
                            reason: format!("observer ineligible: {error}"),
                        },
                    );
                    return Ok(());
                }
                self.accept_observer(fix, now)?;
                self.create_marker(id, now, actions)?;
            }
            Purpose::Create(id) => {
                let marker: RemMapObject = decode(result, "marker")?;
                let hash = marker.object_destination_hash;
                if hash.trim().is_empty() {
                    return Err("host marker without identity".into());
                }
                let created = result.get("created").and_then(Value::as_bool) == Some(true);
                self.set_outcome(
                    id,
                    if created {
                        HostOutcome::Created {
                            object_destination_hash: hash,
                        }
                    } else {
                        HostOutcome::AlreadyExisted {
                            object_destination_hash: hash,
                        }
                    },
                );
            }
            Purpose::SessionList => {
                let items = result
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or("session list items required")?;
                for session in &items {
                    self.consider_session(session, None, now, actions)?;
                }
            }
            Purpose::SessionObjects(expected) => {
                let session = result.get("session").ok_or("session required")?;
                if text(session, "sessionId")? != expected {
                    return Err("host returned a different session".into());
                }
                let objects: Vec<RemMapObject> = decode(result, "objects")?;
                self.apply_objects(&expected, objects, now, actions)?;
            }
            Purpose::SessionStop(_) => {}
        }
        Ok(())
    }

    /// Typed failure handling: a marker creation keeps its own outcome, session
    /// lookups that answer `NotFound` end the session, permission loss makes
    /// the adapter read-only, anything else cancels authorized work.
    fn host_failure(
        &mut self,
        purpose: Purpose,
        code: &str,
        message: &str,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let transient = code == "HostError" || code == "expired" || code.starts_with("binder.");
        match purpose {
            Purpose::Create(id) => {
                let outcome = if transient {
                    HostOutcome::Unknown {
                        reason: format!("{code}: {message}"),
                    }
                } else {
                    HostOutcome::Rejected {
                        code: code.to_owned(),
                        message: message.to_owned(),
                    }
                };
                self.set_outcome(id, outcome);
                if code != "PermissionDenied" {
                    return Ok(self.status());
                }
            }
            Purpose::Measurement(id) => {
                self.set_outcome(
                    id,
                    HostOutcome::Retained {
                        reason: format!("observer snapshot failed: {code} {message}"),
                    },
                );
                if code != "PermissionDenied" {
                    return Ok(self.status());
                }
            }
            Purpose::SessionStop(_) => return Ok(self.status()),
            Purpose::SessionObjects(id) if code == "NotFound" => {
                self.session_stopped(&id, "unknown to REM", actions);
                return Ok(self.status());
            }
            Purpose::Observer if transient => {
                actions.push(json!({"kind":"status","message":format!("observer snapshot failed: {code} {message}")}));
                return Ok(self.status());
            }
            _ => {}
        }
        if code == "PermissionDenied" {
            self.host_ready = false;
        }
        Err(format!("REM rejected map request ({code}): {message}"))
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
            "map.session.started" => {
                let objects: Vec<RemMapObject> = decode(payload, "objects")?;
                self.consider_session(payload, Some(objects), now, actions)?;
            }
            "map.session.object.updated" => {
                if !self.event_session_matches(payload) {
                    return Ok(json!({"ignored":"different session"}));
                }
                self.update_marker(decode(payload, "object")?, now, actions)?;
            }
            "map.session.object.removed" => {
                if !self.event_session_matches(payload) {
                    return Ok(json!({"ignored":"different session"}));
                }
                let id = text(payload, "objectDestinationHash")?.to_owned();
                self.remove_object(&id, actions)?;
            }
            "map.session.stopped" => {
                let id = text(payload, "sessionId")?.to_owned();
                let reason = payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unspecified");
                self.session_stopped(&id, reason, actions);
            }
            _ => return Ok(json!({"ignored":"unrelated event"})),
        }
        Ok(self.status())
    }

    /// REM's snapshot is this phone's own fix in WGS84 with no vertical datum.
    /// It becomes an eligible observer only with the operator's co-location
    /// confirmation; `alt`, `course` and `speed` are not used.
    fn observer_from_snapshot(&self, result: &Value) -> Result<ObserverFix, String> {
        if result.get("available").and_then(Value::as_bool) != Some(true) {
            return Err("REM has no observer fix".into());
        }
        if result.get("source").and_then(Value::as_str) != Some("device") {
            return Err("unexpected observer source".into());
        }
        if result.get("datum").and_then(Value::as_str) != Some("WGS84") {
            return Err("observer datum is not WGS84".into());
        }
        let number = |key: &str| {
            result
                .get(key)
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("observer {key} required"))
        };
        let (lat, lon) = (number("lat")?, number("lon")?);
        let horizontal_accuracy_m = number("horizontalAccuracyM")
            .map_err(|_| "observer horizontal accuracy unknown".to_owned())?;
        let captured_at_ms = result
            .get("updatedAtMs")
            .and_then(Value::as_u64)
            .ok_or("observer updatedAtMs required")?;
        let (source, evidence_id) = match &self.colocation {
            Some(evidence) => (ObserverSource::VerifiedColocated, evidence.clone()),
            None => (ObserverSource::Unverified, String::new()),
        };
        Ok(ObserverFix {
            position: GeographicPosition {
                latitude: lat,
                longitude: lon,
                altitude_m: None,
            },
            captured_at_ms,
            horizontal_accuracy_m,
            source,
            evidence_id,
        })
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
        self.active_session_id().is_some_and(|active| {
            payload.get("sessionId").and_then(Value::as_str) == Some(active.as_str())
        })
    }
}
