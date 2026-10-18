use super::{
    Adapter, HostOutcome, ObserverFix, Purpose, RemMapObject, Value, decode, integer, json, text,
};

/// Phone movement after which the Binox display is re-aimed.
const DISPLAY_REAIM_M: f64 = 10.0;

/// Equirectangular distance; adequate for a 10 m movement threshold.
fn approx_distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_MEAN_RADIUS_M: f64 = 6_371_008.8;
    let x = (lon2 - lon1).to_radians() * f64::midpoint(lat1, lat2).to_radians().cos();
    let y = (lat2 - lat1).to_radians();
    EARTH_MEAN_RADIUS_M * x.hypot(y)
}
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
                now,
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
            return self.host_failure(pending.purpose, &code, &message, now, actions);
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
                self.confirmed_symbol = Some(wanted.to_owned());
                self.host_ready = true;
                self.request_session_list(now, actions)?;
            }
            Purpose::Observer => self.observer_answer(result, now, actions)?,
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
            Purpose::SessionList(sent_at_ms) => {
                let items = result
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or("session list items required")?;
                self.session_list_result(&items, sent_at_ms, now, actions)?;
            }
            Purpose::SessionObjects(expected) => {
                let session = result.get("session").ok_or("session required")?;
                if text(session, "sessionId")? != expected {
                    return Err("host returned a different session".into());
                }
                let objects: Vec<RemMapObject> = decode(result, "objects")?;
                self.apply_objects(&expected, objects, now, actions)?;
            }
            Purpose::Nearby => self.nearby_result(result, now, actions)?,
            Purpose::SessionStop(_) | Purpose::Notify => {}
        }
        Ok(())
    }

    /// Typed failure handling: a marker creation keeps its own outcome, session
    /// lookups that answer `NotFound` end the session, permission loss makes
    /// the adapter read-only and ends capture, anything else cancels
    /// authorized work.
    fn host_failure(
        &mut self,
        purpose: Purpose,
        code: &str,
        message: &str,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let transient = code == "HostError" || code == "expired" || code.starts_with("binder.");
        match purpose {
            Purpose::Create(id) => {
                if transient {
                    self.creation_unknown(id, format!("{code}: {message}"), now);
                } else {
                    self.set_outcome(
                        id,
                        HostOutcome::Rejected {
                            code: code.to_owned(),
                            message: message.to_owned(),
                        },
                    );
                }
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
            // Notifications are best effort, even when REM denies the capability.
            Purpose::Nearby => {
                self.nearby_failed(code, message, actions);
                return Ok(self.status());
            }
            Purpose::Notify => {
                actions.push(json!({"kind":"status","message":format!("REM notification not shown: {code} {message}")}));
                return Ok(self.status());
            }
            Purpose::SessionObjects(id) if code == "NotFound" => {
                self.session_stopped(&id, "unknown to REM", now, actions);
                return Ok(self.status());
            }
            Purpose::SessionObjects(id) if transient => {
                self.end_display(
                    &id,
                    &format!("REM session objects failed: {code}"),
                    true,
                    false,
                    now,
                    actions,
                );
                return Ok(self.status());
            }
            Purpose::SessionList(_) if transient => {
                actions.push(json!({"kind":"status","message":format!("REM session list failed: {code} {message}")}));
                return Ok(self.status());
            }
            Purpose::Observer if transient => {
                actions.push(json!({"kind":"status","message":format!("observer snapshot failed: {code} {message}")}));
                return Ok(self.status());
            }
            _ => {}
        }
        if code == "PermissionDenied" {
            // A missing grant for marker creation or the phone position only
            // stops measurements going to REM; displays and the host stay up.
            if let Some(grant) = match purpose {
                Purpose::Create(_) => Some("Add and edit its own map markers"),
                Purpose::Measurement(_) => Some("Read this device's location"),
                _ => None,
            } {
                let message = format!(
                    "Measurements stay on this phone: allow \"{grant}\" for ATN Rangefinder in REM's plugin settings"
                );
                self.end_capture(&message, actions);
                actions.push(json!({"kind":"status","message":message}));
                return Ok(self.status());
            }
            self.end_capture("map capability revoked (PermissionDenied)", actions);
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
                let session = text(payload, "sessionId")?.to_owned();
                if !self.mark_dirty(&session) {
                    return Ok(json!({"ignored":"different session"}));
                }
                self.update_marker(&session, decode(payload, "object")?, now, actions)?;
            }
            "map.session.object.removed" => {
                let session = text(payload, "sessionId")?.to_owned();
                if !self.mark_dirty(&session) {
                    return Ok(json!({"ignored":"different session"}));
                }
                let id = text(payload, "objectDestinationHash")?.to_owned();
                self.remove_object(&session, &id, now, actions)?;
            }
            "map.session.stopped" => {
                let id = text(payload, "sessionId")?.to_owned();
                let reason = payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unspecified");
                self.session_stopped(&id, reason, now, actions);
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

    /// REM's answer to an observer refresh: use it, or report why not and
    /// wait for the next interval.
    fn observer_answer(
        &mut self,
        result: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        let problem = match self.observer_from_snapshot(result) {
            Ok(fix) => {
                let description = self.describe_fix(&fix, now);
                if self.accept_observer(fix, now)? || self.observer_valid(now).is_ok() {
                    // An older REM fix next to a fresh phone fix is no problem.
                    None
                } else {
                    Some(description)
                }
            }
            Err(reason) => Some(reason),
        };
        match problem {
            None => {
                self.observer_retry_after_ms = None;
                self.observer_problem = None;
                self.refresh_selected(None, now, actions)?;
            }
            Some(problem) => {
                let interval = self
                    .config
                    .as_ref()
                    .map_or(5000, |c| c.reconcile_interval_ms);
                self.observer_retry_after_ms = Some(now.saturating_add(interval));
                self.observer_report(&problem, actions);
            }
        }
        Ok(())
    }

    /// A fix from this phone's own GPS, which the platform keeps running
    /// while the Binox is connected. It is the same device REM reports as the
    /// observer, only fresher; the same age and accuracy limits apply, and an
    /// older fix never replaces a newer one. The display is re-aimed only after
    /// the phone moved more than [`DISPLAY_REAIM_M`], not on GPS jitter.
    pub(super) fn phone_fix(
        &mut self,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let number = |key: &str| {
            v.get(key)
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("phone fix {key} required"))
        };
        let (lat, lon, accuracy) = (number("lat")?, number("lon")?, number("accuracyM")?);
        let captured_at_ms = integer(v, "fixTimeMs")?;
        let (source, evidence_id) = match &self.colocation {
            Some(evidence) => (ObserverSource::VerifiedColocated, evidence.clone()),
            None => (ObserverSource::Unverified, String::new()),
        };
        let fix = ObserverFix {
            position: GeographicPosition {
                latitude: lat,
                longitude: lon,
                altitude_m: None,
            },
            captured_at_ms,
            horizontal_accuracy_m: accuracy,
            source,
            evidence_id,
        };
        let description = self
            .describe_fix(&fix, now)
            .replace("REM's", "The phone GPS");
        if !self.accept_observer(fix, now)? {
            if self.observer_valid(now).is_err() {
                self.observer_report(&description, actions);
            }
            return Ok(json!({"accepted":false}));
        }
        self.observer_retry_after_ms = None;
        self.observer_problem = None;
        let moved = self.display_origin.is_none_or(|(old_lat, old_lon)| {
            approx_distance_m(old_lat, old_lon, lat, lon) > DISPLAY_REAIM_M
        });
        if moved {
            self.display_origin = Some((lat, lon));
            self.refresh_selected(None, now, actions)?;
        }
        Ok(json!({"accepted":true}))
    }

    /// Plain-language reason a fix REM sent cannot be used.
    fn describe_fix(&self, fix: &ObserverFix, now: u64) -> String {
        let limits = self.config.as_ref().map(|c| &c.eligibility);
        format!(
            "REM's position for this phone is {} s old with {:.0} m accuracy; it must be under {} s and {:.0} m. Check REM telemetry",
            now.saturating_sub(fix.captured_at_ms) / 1000,
            fix.horizontal_accuracy_m,
            limits.map_or(0, |l| l.max_observer_age_ms / 1000),
            limits.map_or(0.0, |l| l.max_horizontal_accuracy_m),
        )
    }

    /// Reports an observer problem once per distinct reason.
    pub(super) fn observer_report(&mut self, text: &str, actions: &mut Vec<Value>) {
        if self.observer_problem.as_deref() != Some(text) {
            self.observer_problem = Some(text.to_owned());
            actions.push(json!({"kind":"status","message":text}));
        }
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
}
