mod control;
mod host;
mod observations;
mod sessions;
use crate::{AdapterConfig, RemMapObject};
use atn_domain::{
    MarkerStyle, ObservationRecord, ObserverFix, SelectedPle, SessionConfig, SyncSession,
    derive_draft, marker_for_ple,
};
use atn_protocol::{DECODER_VERSION, InboundFrame, parse_inbound};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

pub const NATIVE_LIMIT: usize = 393_216; // Bounded u16 PNG encoded as a JSON byte array.
pub const ENVELOPE_LIMIT: usize = 65_536; // Real REM SDK protocol limit.
/// REM plugin API minor carrying marker creation, observer snapshot and
/// operator-selected sessions. Older hosts never bind a plugin declaring it.
pub const REQUIRED_API_MINOR: u64 = 3;
/// REM bounds for plugin marker payload text.
pub const HOST_NAME_LIMIT: usize = 128;
pub const HOST_NOTES_LIMIT: usize = 1024;
const TERMINAL_SESSION_BOUND: usize = 64;

pub const OP_SYMBOLS_LIST: &str = "map.symbols.list";
pub const OP_MARKERS_CREATE: &str = "map.markers.create";
pub const OP_OBSERVER_SNAPSHOT: &str = "map.observer.snapshot";
pub const OP_SESSION_LIST: &str = "map.session.list";
pub const OP_SESSION_OBJECTS: &str = "map.session.objects.get";
pub const OP_SESSION_STOP: &str = "map.session.stop";

#[derive(Clone)]
enum Purpose {
    Symbols,
    /// Observer refresh for the active display session.
    Observer,
    /// Observer snapshot captured for one retained observation.
    Measurement(u64),
    Create(u64),
    SessionList,
    SessionObjects(String),
    SessionStop(String),
}
struct Pending {
    purpose: Purpose,
    deadline_ms: u64,
}

/// What REM did with one observation. Timeouts and Binder failures are never
/// retried automatically; they stay distinct from a host rejection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum HostOutcome {
    #[serde(rename_all = "camelCase")]
    Retained {
        reason: String,
    },
    ObserverRequested,
    CreateRequested,
    #[serde(rename_all = "camelCase")]
    Created {
        object_destination_hash: String,
    },
    #[serde(rename_all = "camelCase")]
    AlreadyExisted {
        object_destination_hash: String,
    },
    #[serde(rename_all = "camelCase")]
    Rejected {
        code: String,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Unknown {
        reason: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredObservation {
    id: u64,
    record: ObservationRecord,
    host: HostOutcome,
}

/// Owns one ephemeral connection and at most one REM-authorized display session
/// per plugin process. No hardware identity, resume token, or write queue is persisted.
pub struct Adapter {
    config: Option<AdapterConfig>,
    host_started: bool,
    api_minor: Option<u64>,
    host_ready: bool,
    connection: Option<(String, usize)>,
    /// Operator evidence that the phone (REM's observer) is at the binocular.
    /// Connection-scoped; never persisted.
    colocation: Option<String>,
    observer: Option<ObserverFix>,
    session: Option<SyncSession>,
    terminal_sessions: VecDeque<String>,
    markers: BTreeMap<String, RemMapObject>,
    last_encoded: BTreeMap<String, Vec<u8>>,
    observations: VecDeque<StoredObservation>,
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
            api_minor: None,
            host_ready: false,
            connection: None,
            colocation: None,
            observer: None,
            session: None,
            terminal_sessions: VecDeque::new(),
            markers: BTreeMap::new(),
            last_encoded: BTreeMap::new(),
            observations: VecDeque::new(),
            pending: BTreeMap::new(),
            sequence: 0,
        }
    }

    pub fn dispatch(&mut self, request: &str) -> String {
        let mut actions = Vec::new();
        let parsed = if request.len() > NATIVE_LIMIT {
            Err("native envelope too large".to_owned())
        } else {
            serde_json::from_str::<Value>(request).map_err(|e| e.to_string())
        };
        let now = parsed
            .as_ref()
            .ok()
            .and_then(|v| v.get("nowMs"))
            .and_then(Value::as_u64);
        let result = parsed.and_then(|value| self.handle(&value, &mut actions));
        match result {
            Ok(result) => json!({"ok":true,"result":result,"actions":actions}).to_string(),
            Err(message) => {
                // Errors cannot retain partially authorized write work.
                let mut actions = vec![json!({"kind":"cancelWrites"})];
                self.drop_observation_requests("adapter error before host answered");
                self.pending.clear();
                self.cancel_session(&message);
                if let Some(now) = now {
                    self.end_session_on_host(now, &mut actions);
                }
                self.cancel_host_actions(&mut actions);
                json!({"ok":false,"error":{"code":"adapter","message":message},"actions":actions})
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
            "confirmColocation" | "clearColocation" => self.colocation(cmd, v, now, actions),
            "notification" => self.notification(v, now, actions),
            "hostResponse" => self.host_response(v, now, actions),
            "hostEvent" => self.host_event(v, now, actions),
            "stopSync" | "tick" => self.session_control(cmd, v, now, actions),
            "transportResult" | "confirmDisplay" | "setIcon" | "setStyle" => {
                self.transfer(cmd, v, now, actions)
            }
            "status" => Ok(self.status()),
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    fn api_supported(&self) -> bool {
        self.host_started && self.api_minor.is_some_and(|m| m >= REQUIRED_API_MINOR)
    }

    fn pending_purpose(&self, matches: impl Fn(&Purpose) -> bool) -> bool {
        self.pending.values().any(|p| matches(&p.purpose))
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
        if !self.api_supported() {
            return Err("REM host with plugin API 1.3 required".into());
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

    /// Ends the local display session; REM is told separately by
    /// [`Self::end_session_on_host`] because that needs a live host request.
    fn cancel_session(&mut self, reason: &str) {
        if let Some(session) = self.session.as_mut() {
            if matches!(session.snapshot().state, atn_domain::SessionState::Active) {
                session.stop(reason.into());
            }
        }
        self.markers.clear();
        self.last_encoded.clear();
    }

    fn active_session_id(&self) -> Option<String> {
        self.session
            .as_ref()
            .filter(|s| matches!(s.snapshot().state, atn_domain::SessionState::Active))
            .map(|s| s.snapshot().session_id)
    }

    fn write_gate(&self, now: u64) -> Result<(), String> {
        if !self.host_ready {
            return Err("REM map API unavailable; adapter remains read-only".into());
        }
        self.connection.as_ref().ok_or("BLE disconnected")?;
        let c = self.config.as_ref().ok_or("configuration required")?;
        if !c.device_profile.protocol_verified {
            return Err("device model/firmware protocol not verified".into());
        }
        if self.colocation.is_none() {
            return Err("operator has not confirmed the phone is at the binocular".into());
        }
        c.semantics
            .validate(&c.eligibility, now)
            .map_err(|e| e.to_string())
    }

    fn observer_valid(&self, now: u64) -> Result<(), String> {
        let c = self.config.as_ref().ok_or("configuration required")?;
        self.observer
            .as_ref()
            .ok_or("observer fix required")?
            .validate(&c.eligibility, now)
            .map_err(|e| e.to_string())
    }

    /// Requests a fresh REM observer snapshot for the display session unless one
    /// is already outstanding.
    fn refresh_observer(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        if self.pending_purpose(|p| matches!(p, Purpose::Observer)) {
            return Ok(());
        }
        self.request(
            Purpose::Observer,
            OP_OBSERVER_SNAPSHOT,
            json!({}),
            now,
            actions,
        )
    }

    fn pump(&mut self, now: u64, actions: &mut Vec<Value>) -> Result<(), String> {
        if self.active_session_id().is_none() {
            return Ok(());
        }
        self.write_gate(now)?;
        if let Err(reason) = self.observer_valid(now) {
            // Hold rather than send relative geometry from an ineligible fix.
            actions.push(
                json!({"kind":"status","message":format!("holding display writes: {reason}")}),
            );
            return self.refresh_observer(now, actions);
        }
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
            "hostStarted":self.host_started,"apiMinor":self.api_minor,"requiredApiMinor":REQUIRED_API_MINOR,
            "hostReady":self.host_ready,"colocationConfirmed":self.colocation.is_some(),
            "observerFresh":self.observer.is_some(),
            "session":self.session.as_ref().map(SyncSession::snapshot),
            "pendingHostRequests":self.pending.len(),"observations":self.observations})
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
