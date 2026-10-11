mod control;
mod host;
mod observations;
use crate::{AdapterConfig, RchMarker};
use atn_domain::{
    MarkerStyle, ObservationRecord, ObserverFix, SelectedPle, SessionConfig, SyncSession,
    derive_draft, marker_for_ple,
};
use atn_protocol::{DECODER_VERSION, InboundFrame, parse_inbound};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

pub const NATIVE_LIMIT: usize = 393_216; // Bounded u16 PNG encoded as a JSON byte array.
pub const ENVELOPE_LIMIT: usize = 65_536; // Real REM SDK protocol limit.
const OPERATIONS: [&str; 6] = [
    "map.adapter.describe",
    "map.observer.get",
    "map.ple.get",
    "map.ple.subscribe",
    "map.observation.draft.put",
    "map.ple.unsubscribe",
];

enum Purpose {
    Describe,
    Observer,
    Measurement(u64),
    Marker(String),
    Subscribe,
    Draft,
    Unsubscribe,
}
struct Pending {
    purpose: Purpose,
    deadline_ms: u64,
}

/// Owns one ephemeral connection and authorization session per plugin process.
/// No hardware identity, resume token, or write queue is persisted.
pub struct Adapter {
    config: Option<AdapterConfig>,
    host_started: bool,
    host_ready: bool,
    subscribed: bool,
    connection: Option<(String, usize)>,
    observer: Option<ObserverFix>,
    session: Option<SyncSession>,
    markers: BTreeMap<String, RchMarker>,
    last_encoded: BTreeMap<String, Vec<u8>>,
    observations: VecDeque<(u64, ObservationRecord)>,
    pending: BTreeMap<String, Pending>,
    sequence: u64,
}

impl Default for Adapter {
    fn default() -> Self {
        Self::new()
    }
}
impl Adapter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: None,
            host_started: false,
            host_ready: false,
            subscribed: false,
            connection: None,
            observer: None,
            session: None,
            markers: BTreeMap::new(),
            last_encoded: BTreeMap::new(),
            observations: VecDeque::new(),
            pending: BTreeMap::new(),
            sequence: 0,
        }
    }

    pub fn dispatch(&mut self, request: &str) -> String {
        let mut actions = Vec::new();
        let result = if request.len() > NATIVE_LIMIT {
            Err("native envelope too large".into())
        } else {
            serde_json::from_str::<Value>(request)
                .map_err(|e| e.to_string())
                .and_then(|value| self.handle(&value, &mut actions))
        };
        match result {
            Ok(result) => json!({"ok":true,"result":result,"actions":actions}).to_string(),
            Err(message) => {
                // Errors cannot retain partially authorized write work.
                self.cancel_session(&message);
                self.pending.clear();
                json!({"ok":false,"error":{"code":"adapter","message":message},
                    "actions":[{"kind":"cancelWrites"},{"kind":"cancelHostRequests","keepRequestIds":[]}]})
                .to_string()
            }
        }
    }

    fn handle(&mut self, v: &Value, actions: &mut Vec<Value>) -> Result<Value, String> {
        let now = if let Some(value) = v.get("nowMs") {
            value.as_u64().ok_or("unsigned nowMs required")?
        } else {
            clock_ms()?
        };
        let cmd = text(v, "cmd")?;
        match cmd {
            "configure" | "hostStarted" | "hostStopped" => self.lifecycle(cmd, v, now, actions),
            "connected" | "disconnected" => self.ble_state(cmd, v, now, actions),
            "notification" => self.notification(v, now, actions),
            "hostResponse" => self.host_response(v, now, actions),
            "hostEvent" => self.host_event(v, now, actions),
            "startSync" => self.start_sync(v, now, actions),
            "stopSync" | "tick" => self.session_control(cmd, v, now, actions),
            "transportResult" | "confirmDisplay" | "setIcon" | "setStyle" => {
                self.transfer(cmd, v, now, actions)
            }
            "status" => Ok(self.status()),
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    fn describe(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        self.request(
            Purpose::Describe,
            OPERATIONS[0],
            json!({"contractVersion":1}),
            now,
            actions,
        )
    }

    #[allow(clippy::needless_pass_by_value)] // Actions take ownership of the serialized payload.
    fn request(
        &mut self,
        purpose: Purpose,
        operation: &str,
        payload: Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<(), String> {
        if !self.host_started {
            return Err("host not started".into());
        }
        let c = self.config.as_ref().ok_or("configuration required")?;
        if self.pending.len() >= c.transport.max_pending_host_requests {
            return Err("host request bound reached".into());
        }
        let deadline_ms = now
            .checked_add(c.transport.host_request_timeout_ms)
            .ok_or("deadline overflow")?;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("request identity exhausted")?;
        let id = format!("atn/{}", self.sequence);
        let envelope =
            json!({"protocolVersion":1,"requestId":id,"operation":operation,"payload":payload});
        if envelope.to_string().len() > ENVELOPE_LIMIT {
            return Err("host envelope too large".into());
        }
        self.pending.insert(
            id,
            Pending {
                purpose,
                deadline_ms,
            },
        );
        actions.push(json!({"kind":"hostRequest","envelope":envelope}));
        Ok(())
    }

    fn cancel_host_actions(&self, actions: &mut Vec<Value>) {
        actions.push(json!({"kind":"cancelHostRequests","keepRequestIds":self.pending.keys().collect::<Vec<_>>()}));
    }

    fn cancel_session(&mut self, reason: &str) {
        if let Some(session) = self.session.as_mut() {
            session.stop(reason.into());
        }
        self.subscribed = false;
        self.markers.clear();
        self.last_encoded.clear();
    }

    fn write_gate(&self, now: u64) -> Result<(), String> {
        if !self.host_ready {
            return Err("REM map contract not negotiated".into());
        }
        self.connection.as_ref().ok_or("BLE disconnected")?;
        let c = self.config.as_ref().ok_or("configuration required")?;
        if !c.device_profile.protocol_verified {
            return Err("device model/firmware protocol not verified".into());
        }
        c.semantics
            .validate(&c.eligibility, now)
            .map_err(|e| e.to_string())
    }

    fn start_sync(
        &mut self,
        v: &Value,
        now: u64,
        actions: &mut Vec<Value>,
    ) -> Result<Value, String> {
        self.write_gate(now)?;
        self.cancel_session("new user authorization");
        self.pending.clear();
        self.cancel_host_actions(actions);
        let c = self.config.as_ref().ok_or("configuration required")?;
        let selected: Vec<SelectedPle> = decode(v, "selected")?;
        let config = SessionConfig {
            session_id: text(v, "sessionId")?.into(),
            selected: selected.clone(),
            queue_capacity: c.queue_capacity,
            max_queued_bytes: c.max_queued_bytes,
            queue_ttl_ms: c.queue_ttl_ms,
            operation_timeout_ms: c.operation_timeout_ms,
        };
        self.session = Some(SyncSession::start(config, now).map_err(|e| e.to_string())?);
        actions.push(json!({"kind":"cancelWrites"}));
        self.request(Purpose::Subscribe, OPERATIONS[3], json!({"sessionId":text(v,"sessionId")?,"pleIds":selected.iter().map(|s|&s.ple_id).collect::<Vec<_>>()}), now, actions)?;
        self.request(Purpose::Observer, OPERATIONS[1], json!({}), now, actions)?;
        for item in selected {
            self.request(
                Purpose::Marker(item.ple_id.clone()),
                OPERATIONS[2],
                json!({"pleId":item.ple_id}),
                now,
                actions,
            )?;
        }
        Ok(self.status())
    }

    fn pump(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        if !self.subscribed {
            return Ok(());
        }
        self.write_gate(now)?;
        let c = self.config.as_ref().ok_or("configuration required")?;
        self.observer
            .as_ref()
            .ok_or("observer required before writing")?
            .validate(&c.eligibility, now)
            .map_err(|e| e.to_string())?;
        if let Some(frame) = self
            .session
            .as_mut()
            .ok_or("session required")?
            .next_write(now)
            .map_err(|e| e.to_string())?
        {
            actions.push(json!({"kind":"write","operationId":frame.operation_id,
                "frameNumber":frame.frame_number,"totalFrames":frame.total_frames,"bytes":frame.bytes,
                "sessionId":self.session.as_ref().map(|s|s.snapshot().session_id),
                "connectionId":self.connection.as_ref().map(|(id,_)|id)}));
        }
        Ok(())
    }

    fn status(&self) -> Value {
        json!({"configured":self.config.is_some(),"connected":self.connection.is_some(),
            "hostReady":self.host_ready,"session":self.session.as_ref().map(SyncSession::snapshot),
            "pendingHostRequests":self.pending.len(),"observations":self.observations.iter().map(|(id,record)|json!({"id":id,"record":record})).collect::<Vec<_>>()})
    }
}

fn decode<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(v.get(key).ok_or_else(|| format!("missing {key}"))?.clone())
        .map_err(|e| e.to_string())
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("nonempty {key} required"))
}
fn integer(v: &Value, key: &str) -> Result<u64, String> {
    v.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("unsigned {key} required"))
}
fn clock_ms() -> Result<u64, String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    u64::try_from(elapsed.as_millis()).map_err(|e| e.to_string())
}
