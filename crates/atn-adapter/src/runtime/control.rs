use super::{Adapter, AdapterConfig, OPERATIONS, Purpose, Value, decode, integer, json, text};

impl Adapter {
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
                self.pending.clear();
                self.cancel_host_actions(actions);
                self.observer = None;
                self.host_ready = false;
                self.config = Some(config);
                actions.push(json!({"kind":"cancelWrites"}));
                if self.host_started {
                    self.describe(now, actions)?;
                }
                Ok(json!({"configured":true}))
            }
            "hostStarted" => {
                self.cancel_session("host restarted");
                self.pending.clear();
                self.cancel_host_actions(actions);
                self.host_started = true;
                self.host_ready = false;
                self.observer = None;
                actions.push(json!({"kind":"cancelWrites"}));
                if self.config.is_some() {
                    self.describe(now, actions)?;
                }
                Ok(json!({"hostReady":false}))
            }
            "hostStopped" => {
                self.host_started = false;
                self.host_ready = false;
                self.connection = None;
                self.observer = None;
                self.pending.clear();
                self.cancel_host_actions(actions);
                self.cancel_session("host stopped");
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
                self.observer = None;
                self.pending
                    .retain(|_, p| matches!(p.purpose, Purpose::Describe));
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
                self.pending
                    .retain(|_, p| matches!(p.purpose, Purpose::Describe));
                self.cancel_host_actions(actions);
                let old = self
                    .session
                    .as_ref()
                    .filter(|s| matches!(s.snapshot().state, atn_domain::SessionState::Active))
                    .map(|s| s.snapshot().session_id);
                self.cancel_session("BLE disconnected");
                if self.host_ready && self.host_started {
                    if let Some(id) = old {
                        self.request(
                            Purpose::Unsubscribe,
                            OPERATIONS[5],
                            json!({"sessionId":id}),
                            now,
                            actions,
                        )?;
                    }
                }
                actions.push(json!({"kind":"cancelWrites"}));
                Ok(json!({"connected":false}))
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
                let old = self.session.as_ref().map(|s| s.snapshot().session_id);
                self.cancel_session("user stopped session");
                self.pending.clear();
                self.cancel_host_actions(actions);
                actions.push(json!({"kind":"cancelWrites"}));
                if self.host_ready && self.host_started {
                    if let Some(id) = old {
                        self.request(
                            Purpose::Unsubscribe,
                            OPERATIONS[5],
                            json!({"sessionId":id}),
                            now,
                            actions,
                        )?;
                    }
                }
                Ok(self.status())
            }
            "tick" => {
                if self.pending.values().any(|p| now >= p.deadline_ms) {
                    self.host_ready = false;
                    return Err("host request timeout; map contract unavailable".into());
                }
                if let Some(session) = self.session.as_mut() {
                    if matches!(session.snapshot().state, atn_domain::SessionState::Active) {
                        session.tick(now).map_err(|e| e.to_string())?;
                    }
                }
                Ok(self.status())
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }
}
