use super::{
    Adapter, AdapterConfig, Capture, HostOutcome, OP_SYMBOLS_LIST, Purpose, REQUIRED_API_MINOR,
    Resolution, Value, decode, integer, json, text,
};

impl Adapter {
    /// Observation requests that can no longer be answered keep an explicit
    /// indeterminate or retained outcome instead of "requested" forever.
    pub(super) fn drop_observation_requests(&mut self, reason: &str) {
        let purposes: Vec<Purpose> = self.pending.values().map(|p| p.purpose.clone()).collect();
        for purpose in purposes {
            match purpose {
                Purpose::Create(id) => self.set_outcome(
                    id,
                    HostOutcome::Unknown {
                        reason: reason.to_owned(),
                        resolution: Resolution::Final,
                    },
                ),
                Purpose::Measurement(id) => self.set_outcome(
                    id,
                    HostOutcome::Retained {
                        reason: reason.to_owned(),
                    },
                ),
                _ => {}
            }
        }
    }

    /// Operator-started capture authorization for the current connection.
    /// Refusals are reported without side effects on other work.
    pub(super) fn capture_control(
        &mut self,
        cmd: &str,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        match cmd {
            "startCapture" => {
                if let Some(reason) = self.capture_refusal(now) {
                    actions.push(
                        json!({"kind":"status","message":format!("capture not started: {reason}")}),
                    );
                    return Ok(
                        json!({"captureStarted":false,"reason":reason,"status":self.status()}),
                    );
                }
                let connection_id = self
                    .connection
                    .as_ref()
                    .map(|(id, _)| id.clone())
                    .ok_or("BLE disconnected")?;
                let nonce = self.mint_capture_nonce()?;
                self.capture = Some(Capture {
                    nonce,
                    connection_id,
                    started_at_ms: now,
                });
                self.capture_ended = None;
                Ok(json!({"captureStarted":true,"status":self.status()}))
            }
            "stopCapture" => {
                self.end_capture("operator stopped capture", actions);
                Ok(self.status())
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    fn capture_refusal(&self, now: u64) -> Option<String> {
        if self.capture.is_some() {
            return Some("capture already active".into());
        }
        if let Err(reason) = self.write_gate(now) {
            return Some(reason);
        }
        let symbol = self.config.as_ref().map(|c| c.host_marker.symbol.as_str());
        if self.confirmed_symbol.as_deref() != symbol {
            return Some("configured marker symbol not confirmed by map.symbols.list".into());
        }
        None
    }

    /// Ends capture. Unanswered observer snapshots of the ended capture keep a
    /// retained outcome and a scheduled replay becomes final; a creation
    /// already sent keeps waiting for REM's answer, which is still recorded.
    pub(super) fn end_capture(&mut self, reason: &str, actions: &mut Vec<Value>) {
        if self.capture.take().is_none() {
            return;
        }
        self.capture_ended = Some(reason.to_owned());
        let measuring: Vec<(String, u64)> = self
            .pending
            .iter()
            .filter_map(|(request, p)| match p.purpose {
                Purpose::Measurement(id) => Some((request.clone(), id)),
                _ => None,
            })
            .collect();
        for (request, id) in &measuring {
            self.pending.remove(request);
            self.set_outcome(
                *id,
                HostOutcome::Retained {
                    reason: format!("capture ended: {reason}"),
                },
            );
        }
        self.finalize_replays(reason);
        if !measuring.is_empty() {
            self.cancel_host_actions(actions);
        }
        actions.push(json!({"kind":"status","message":format!("capture ended: {reason}")}));
    }

    /// Capture never outlives the authorization it was started under.
    pub(super) fn enforce_capture_gate(&mut self, actions: &mut Vec<Value>) {
        let Some(capture) = &self.capture else {
            return;
        };
        let reason = if !self.host_ready {
            Some("REM map API unavailable")
        } else if self.connection.as_ref().map(|(id, _)| id) != Some(&capture.connection_id) {
            Some("BLE connection changed")
        } else if self.colocation.is_none() {
            Some("co-location confirmation withdrawn")
        } else if !self
            .config
            .as_ref()
            .is_some_and(|c| c.device_profile.protocol_verified)
        {
            Some("device profile unverified")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.end_capture(reason, actions);
        }
    }

    fn negotiate(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        self.host_ready = false;
        self.confirmed_symbol = None;
        if !self.api_supported() {
            actions.push(json!({"kind":"status","message":format!("REM plugin API 1.{REQUIRED_API_MINOR} required; map exchange unavailable")}));
            return Ok(());
        }
        if self.config.is_none() {
            return Ok(());
        }
        self.request(Purpose::Symbols, OP_SYMBOLS_LIST, json!({}), now, actions)
    }

    pub(super) fn lifecycle(
        &mut self,
        cmd: &str,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        match cmd {
            "configure" => {
                let config: AdapterConfig = decode(v, "config")?;
                config.validate()?;
                self.end_capture("configuration changed", actions);
                self.end_all_displays("configuration changed", true, now, actions);
                self.drop_observation_requests("configuration changed before host answered");
                self.pending
                    .retain(|_, p| matches!(p.purpose, Purpose::SessionStop(_)));
                self.cancel_host_actions(actions);
                self.observer = None;
                self.colocation = None;
                self.config = Some(config);
                actions.push(json!({"kind":"cancelWrites"}));
                self.negotiate(now, actions)?;
                Ok(json!({"configured":true}))
            }
            "hostStarted" => {
                self.end_capture("host restarted", actions);
                // The previous Binder host session is gone; it cannot be told.
                self.end_all_displays("host restarted", false, now, actions);
                self.last_reconcile_ms = None;
                self.drop_observation_requests("host restarted before answering");
                self.pending.clear();
                self.cancel_host_actions(actions);
                self.host_started = true;
                self.api_minor = Some(integer(v, "apiMinor")?);
                self.observer = None;
                self.terminal_sessions.clear();
                actions.push(json!({"kind":"cancelWrites"}));
                self.negotiate(now, actions)?;
                Ok(json!({"hostReady":false,"apiSupported":self.api_supported()}))
            }
            "hostStopped" => {
                self.end_capture("host stopped", actions);
                self.host_started = false;
                self.api_minor = None;
                self.host_ready = false;
                self.confirmed_symbol = None;
                self.connection = None;
                self.colocation = None;
                self.observer = None;
                self.drop_observation_requests("host stopped before answering");
                self.pending.clear();
                self.cancel_host_actions(actions);
                self.end_all_displays("host stopped", false, now, actions);
                self.in_flight = None;
                self.last_reconcile_ms = None;
                self.terminal_sessions.clear();
                actions.push(json!({"kind":"cancelWrites"}));
                actions.push(json!({"kind":"disconnect"}));
                Ok(json!({"hostReady":false}))
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    pub(super) fn ble_state(
        &mut self,
        cmd: &str,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        match cmd {
            "connected" => {
                self.end_capture("new BLE connection", actions);
                self.end_all_displays("new BLE connection", true, now, actions);
                self.in_flight = None;
                self.observer = None;
                self.colocation = None;
                self.drop_observation_requests("connection changed before host answered");
                self.pending.retain(|_, p| {
                    matches!(
                        p.purpose,
                        Purpose::Symbols
                            | Purpose::SessionList(_)
                            | Purpose::SessionStop(_)
                            | Purpose::Notify
                    )
                });
                self.cancel_host_actions(actions);
                let capacity = integer(v, "writeCapacity")?;
                let capacity = usize::try_from(capacity).map_err(|e| e.to_string())?;
                if !(20..=512).contains(&capacity) {
                    return Err("invalid negotiated ATT capacity".into());
                }
                self.connection = Some((text(v, "connectionId")?.into(), capacity));
                actions.push(json!({"kind":"cancelWrites"}));
                Ok(json!({"readOnly":true,"writeCapacity":capacity}))
            }
            "disconnected" => {
                self.end_capture("BLE disconnected", actions);
                self.connection = None;
                self.observer = None;
                self.colocation = None;
                self.end_all_displays("BLE disconnected", true, now, actions);
                self.in_flight = None;
                self.drop_observation_requests("disconnected before host answered");
                self.pending.retain(|_, p| {
                    matches!(
                        p.purpose,
                        Purpose::Symbols
                            | Purpose::SessionList(_)
                            | Purpose::SessionStop(_)
                            | Purpose::Notify
                    )
                });
                self.cancel_host_actions(actions);
                actions.push(json!({"kind":"cancelWrites"}));
                Ok(json!({"connected":false}))
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    /// The operator attests that this phone, whose fix REM reports, is at the
    /// binocular. The attestation is connection-scoped and never persisted.
    pub(super) fn colocation(
        &mut self,
        cmd: &str,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        match cmd {
            "confirmColocation" => {
                self.connection.as_ref().ok_or("connect the device first")?;
                let evidence = text(v, "evidenceId")?;
                if evidence.chars().count() > 64 {
                    return Err("co-location evidence is too long".into());
                }
                self.colocation = Some(evidence.to_owned());
                self.observer = None;
                if self.host_ready {
                    self.request_session_list(now, actions)?;
                }
                Ok(self.status())
            }
            "clearColocation" => {
                self.end_capture("co-location confirmation withdrawn", actions);
                self.colocation = None;
                self.observer = None;
                self.end_all_displays("co-location confirmation withdrawn", true, now, actions);
                actions.push(json!({"kind":"cancelWrites"}));
                Ok(self.status())
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    pub(super) fn transfer(
        &mut self,
        cmd: &str,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        match cmd {
            "transportResult" => self.transport_result(v, now, actions),
            "confirmDisplay" => self.confirm_display(v),
            "setIcon" => self.set_icon(v, now, actions),
            "setStyle" => {
                self.write_gate(now)?;
                let marker_type =
                    u8::try_from(integer(v, "markerType")?).map_err(|e| e.to_string())?;
                let color = u8::try_from(integer(v, "color")?).map_err(|e| e.to_string())?;
                if marker_type > 1 || color > 7 {
                    return Err("unrepresentable style".into());
                }
                let c = self.config.as_mut().ok_or("configuration required")?;
                c.marker_type = marker_type;
                c.color = color;
                self.refresh_selected(None, now, actions)?;
                Ok(self.status())
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    pub(super) fn session_control(
        &mut self,
        cmd: &str,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        match cmd {
            "stopSync" => {
                // With a `sessionId` only that session ends; without, all do.
                if let Some(id) = v.get("sessionId").and_then(Value::as_str) {
                    let id = id.to_owned();
                    self.end_display(
                        &id,
                        "operator stopped display session",
                        true,
                        false,
                        now,
                        actions,
                    );
                    self.pending.retain(
                        |_, p| !matches!(&p.purpose, Purpose::SessionObjects(s) if *s == id),
                    );
                } else {
                    self.end_all_displays("operator stopped display session", true, now, actions);
                    self.pending.retain(|_, p| {
                        !matches!(p.purpose, Purpose::Observer | Purpose::SessionObjects(_))
                    });
                    actions.push(json!({"kind":"cancelWrites"}));
                }
                self.cancel_host_actions(actions);
                self.pump(now, actions)?;
                Ok(self.status())
            }
            "tick" => {
                self.expire_pending(now, actions)?;
                self.replay_due(now, actions);
                self.sessions_tick(now, actions)?;
                if !self.displays.is_empty() {
                    let c = self.config.as_ref().ok_or("configuration required")?;
                    let half_life = c.eligibility.max_observer_age_ms / 2;
                    let refresh = self
                        .observer
                        .as_ref()
                        .is_none_or(|fix| now.saturating_sub(fix.captured_at_ms) >= half_life);
                    if refresh {
                        self.refresh_observer(now, actions)?;
                    }
                }
                Ok(self.status())
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    /// Pending host requests past their deadline. Observation work records an
    /// indeterminate outcome; session discovery/authorization work fails closed.
    fn expire_pending(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        let expired: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, p)| now >= p.deadline_ms)
            .map(|(id, _)| id.clone())
            .collect();
        let mut hard = None;
        for id in expired {
            let Some(pending) = self.pending.remove(&id) else {
                continue;
            };
            match pending.purpose {
                Purpose::Create(id) => {
                    self.creation_unknown(id, "no host response before deadline".into(), now);
                }
                Purpose::Measurement(id) => self.set_outcome(
                    id,
                    HostOutcome::Retained {
                        reason: "observer snapshot timed out".into(),
                    },
                ),
                Purpose::SessionStop(_) => {
                    actions.push(
                        json!({"kind":"status","message":"REM session stop request timed out"}),
                    );
                }
                Purpose::Observer => {
                    actions.push(json!({"kind":"status","message":"observer snapshot timed out; display writes held"}));
                }
                Purpose::Notify => {
                    actions.push(json!({"kind":"status","message":"REM notification timed out"}));
                }
                // The next reconciliation lists again; nothing ends meanwhile.
                Purpose::SessionList(_) => {
                    actions.push(json!({"kind":"status","message":"REM session list timed out"}));
                }
                // A session whose membership cannot be confirmed is ended.
                Purpose::SessionObjects(session) => {
                    self.end_display(
                        &session,
                        "REM session objects timed out",
                        true,
                        false,
                        now,
                        actions,
                    );
                }
                Purpose::Symbols => {
                    hard = Some("host request timeout; map exchange unavailable");
                }
            }
        }
        if let Some(reason) = hard {
            self.host_ready = false;
            return Err(reason.into());
        }
        Ok(())
    }
}
