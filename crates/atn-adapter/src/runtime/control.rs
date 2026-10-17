use super::{
    Adapter, AdapterConfig, OP_SESSION_LIST, OP_SYMBOLS_LIST, Purpose, REQUIRED_API_MINOR, Value,
    decode, integer, json, text,
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
                    super::HostOutcome::Unknown {
                        reason: reason.to_owned(),
                    },
                ),
                Purpose::Measurement(id) => self.set_outcome(
                    id,
                    super::HostOutcome::Retained {
                        reason: reason.to_owned(),
                    },
                ),
                _ => {}
            }
        }
    }

    fn negotiate(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        self.host_ready = false;
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
                self.cancel_session("configuration changed");
                self.end_session_on_host(now, actions);
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
                self.cancel_session("host restarted");
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
                self.host_started = false;
                self.api_minor = None;
                self.host_ready = false;
                self.connection = None;
                self.colocation = None;
                self.observer = None;
                self.drop_observation_requests("host stopped before answering");
                self.pending.clear();
                self.cancel_host_actions(actions);
                self.cancel_session("host stopped");
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
                self.cancel_session("new BLE connection");
                self.end_session_on_host(now, actions);
                self.observer = None;
                self.colocation = None;
                self.drop_observation_requests("connection changed before host answered");
                self.pending.retain(|_, p| {
                    matches!(
                        p.purpose,
                        Purpose::Symbols | Purpose::SessionList | Purpose::SessionStop(_)
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
                self.connection = None;
                self.observer = None;
                self.colocation = None;
                self.cancel_session("BLE disconnected");
                self.end_session_on_host(now, actions);
                self.drop_observation_requests("disconnected before host answered");
                self.pending.retain(|_, p| {
                    matches!(
                        p.purpose,
                        Purpose::Symbols | Purpose::SessionList | Purpose::SessionStop(_)
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
                if self.host_ready && !self.pending_purpose(|p| matches!(p, Purpose::SessionList)) {
                    self.request(
                        Purpose::SessionList,
                        OP_SESSION_LIST,
                        json!({}),
                        now,
                        actions,
                    )?;
                }
                Ok(self.status())
            }
            "clearColocation" => {
                self.colocation = None;
                self.observer = None;
                self.cancel_session("co-location confirmation withdrawn");
                self.end_session_on_host(now, actions);
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
            "transportResult" => {
                let context_matches = self.connection.as_ref().is_some_and(|(id, _)| {
                    v.get("connectionId").and_then(Value::as_str) == Some(id.as_str())
                }) && self.session.as_ref().is_some_and(|s| {
                    v.get("sessionId").and_then(Value::as_str)
                        == Some(s.snapshot().session_id.as_str())
                });
                if !context_matches {
                    return Ok(json!({"ignored":"stale transport callback"}));
                }
                let id = integer(v, "operationId")?;
                let number =
                    usize::try_from(integer(v, "frameNumber")?).map_err(|e| e.to_string())?;
                if !v
                    .get("completed")
                    .and_then(Value::as_bool)
                    .ok_or("completed boolean required")?
                {
                    return Err("transport failed; explicit new session required".into());
                }
                let report = self
                    .session
                    .as_mut()
                    .ok_or("no session")?
                    .transport_completed(id, number, now)
                    .map_err(|e| e.to_string())?;
                self.pump(now, actions)?;
                Ok(json!(report))
            }
            "confirmDisplay" => {
                let id = integer(v, "operationId")?;
                let evidence = text(v, "evidenceId")?.into();
                let report = self
                    .session
                    .as_mut()
                    .ok_or("no session")?
                    .confirm_display(id, evidence)
                    .map_err(|e| e.to_string())?;
                Ok(json!(report))
            }
            "setIcon" => {
                self.write_gate(now)?;
                let bytes: Vec<u8> = decode(v, "png")?;
                let c = self.config.as_ref().ok_or("configuration required")?;
                let capacity = self.connection.as_ref().ok_or("BLE disconnected")?.1;
                let id = self
                    .session
                    .as_mut()
                    .ok_or("no session")?
                    .enqueue_icon(text(v, "pleId")?, &bytes, capacity, c.png_limits, now)
                    .map_err(|e| e.to_string())?;
                self.pump(now, actions)?;
                Ok(json!({"operationId":id}))
            }
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
                self.refresh_selected(now, actions)?;
                Ok(self.status())
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    pub(super) fn session_control(
        &mut self,
        cmd: &str,
        _v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        match cmd {
            "stopSync" => {
                self.cancel_session("operator stopped display session");
                self.end_session_on_host(now, actions);
                self.pending.retain(|_, p| {
                    !matches!(p.purpose, Purpose::Observer | Purpose::SessionObjects(_))
                });
                self.cancel_host_actions(actions);
                actions.push(json!({"kind":"cancelWrites"}));
                Ok(self.status())
            }
            "tick" => {
                self.expire_pending(now, actions)?;
                if let Some(session) = self.session.as_mut() {
                    if matches!(session.snapshot().state, atn_domain::SessionState::Active) {
                        session.tick(now).map_err(|e| e.to_string())?;
                    }
                }
                if self.active_session_id().is_some() {
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
                Purpose::Create(id) => self.set_outcome(
                    id,
                    super::HostOutcome::Unknown {
                        reason: "no host response before deadline".into(),
                    },
                ),
                Purpose::Measurement(id) => self.set_outcome(
                    id,
                    super::HostOutcome::Retained {
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
                Purpose::Symbols | Purpose::SessionList | Purpose::SessionObjects(_) => {
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
