mod control;
mod host;
mod observations;
mod sessions;
use crate::{AdapterConfig, RemMapObject};
use atn_domain::{
    MarkerStyle, ObservationRecord, ObserverFix, SelectedPle, SessionConfig, SessionSnapshot,
    SyncSession, derive_draft, marker_for_ple,
};
use atn_protocol::{DECODER_VERSION, InboundFrame, parse_inbound};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    hash::{BuildHasher, Hasher},
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
/// REM serves at most four active selection sessions per plugin.
pub const MAX_DISPLAY_SESSIONS: usize = 4;
const RELEASED_SLOT_BOUND: usize = 32;
const ENDED_SESSION_BOUND: usize = 8;

pub const OP_SYMBOLS_LIST: &str = "map.symbols.list";
pub const OP_MARKERS_CREATE: &str = "map.markers.create";
pub const OP_OBSERVER_SNAPSHOT: &str = "map.observer.snapshot";
pub const OP_SESSION_LIST: &str = "map.session.list";
pub const OP_SESSION_OBJECTS: &str = "map.session.objects.get";
pub const OP_SESSION_STOP: &str = "map.session.stop";
pub const OP_NOTIFY: &str = "notifications.raise";

#[derive(Clone)]
enum Purpose {
    Symbols,
    /// Observer refresh for the active display sessions.
    Observer,
    /// Observer snapshot captured for one retained observation.
    Measurement(u64),
    Create(u64),
    /// Session discovery/reconciliation, with the time the request was sent:
    /// only sessions accepted before then can be judged absent from the list.
    SessionList(u64),
    SessionObjects(String),
    SessionStop(String),
    /// Operator-visible rejection reason; best effort, never cancels work.
    Notify,
}
struct Pending {
    purpose: Purpose,
    deadline_ms: u64,
}

/// Whether an indeterminate creation may still be resolved by REM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    /// One replay of the same key and payload is due on a later tick.
    ReplayScheduled,
    /// The outcome stays unknown; nothing is retried.
    Final,
}

/// What REM did with one observation. Timeouts and Binder failures stay
/// distinct from a host rejection; they allow one bounded same-key replay
/// within the same Binder host session and capture session, never more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum HostOutcome {
    #[serde(rename_all = "camelCase")]
    Retained {
        reason: String,
    },
    ObserverRequested,
    CreateRequested,
    /// The single replay of an indeterminate creation is outstanding.
    ReplayRequested,
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
        resolution: Resolution,
    },
}

/// The exact creation request sent for one observation, kept in memory only
/// so an indeterminate outcome can be replayed byte-for-byte once.
struct CreateAttempt {
    payload: Value,
    replayed: bool,
    replay_due_ms: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredObservation {
    id: u64,
    record: ObservationRecord,
    host: HostOutcome,
    /// Capture session nonce in force when the measurement arrived.
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_session: Option<String>,
    #[serde(skip)]
    create: Option<CreateAttempt>,
}

/// Operator-started authorization to turn this connection's measurements
/// into REM markers. Process-local; never persisted or restored.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Capture {
    nonce: String,
    connection_id: String,
    started_at_ms: u64,
}

/// One accepted REM selection session: its own domain queue and slots, and
/// the host objects it was last given. Writes from all sessions share one
/// device-wide scheduler in [`Adapter`].
struct Display {
    sync: SyncSession,
    markers: BTreeMap<String, RemMapObject>,
    last_encoded: BTreeMap<String, Vec<u8>>,
    accepted_at_ms: u64,
    /// Last authoritative object snapshot (`started` or `objects.get`).
    snapshot_at_ms: u64,
    /// An event arrived since the last authoritative snapshot.
    dirty: bool,
}

/// A slot whose authorization ended. The device still shows whatever was
/// last written there: releasing never sends a clear command.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleasedSlot {
    slot_id: u16,
    session_id: String,
    ple_id: String,
    reason: String,
    device_display_cleared: bool,
}

/// The one frame handed to the platform, device-wide.
#[derive(Clone, PartialEq, Eq)]
struct InFlight {
    session_id: String,
    operation_id: u64,
    frame_number: usize,
}

/// Owns one ephemeral connection and up to four REM-authorized display
/// sessions per plugin process. No hardware identity, resume token, or write
/// queue is persisted.
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
    displays: Vec<Display>,
    ended_sessions: VecDeque<SessionSnapshot>,
    released_slots: VecDeque<ReleasedSlot>,
    in_flight: Option<InFlight>,
    /// Round-robin cursor over `displays` for starting the next operation.
    next_writer: usize,
    last_reconcile_ms: Option<u64>,
    terminal_sessions: VecDeque<String>,
    observations: VecDeque<StoredObservation>,
    pending: BTreeMap<String, Pending>,
    sequence: u64,
    /// Symbol `map.symbols.list` confirmed for the current host session.
    confirmed_symbol: Option<String>,
    capture: Option<Capture>,
    capture_ended: Option<String>,
    /// Random per process; with `capture_count` it makes capture nonces, and
    /// so idempotency keys, distinct from those of any earlier process.
    process_nonce: u64,
    capture_count: u64,
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
            displays: Vec::new(),
            ended_sessions: VecDeque::new(),
            released_slots: VecDeque::new(),
            in_flight: None,
            next_writer: 0,
            last_reconcile_ms: None,
            terminal_sessions: VecDeque::new(),
            observations: VecDeque::new(),
            pending: BTreeMap::new(),
            sequence: 0,
            confirmed_symbol: None,
            capture: None,
            capture_ended: None,
            // std seeds `RandomState` from operating-system randomness. This is
            // a collision guard, not a secret or an authorization token.
            process_nonce: std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish(),
            capture_count: 0,
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
            Ok(result) => {
                self.enforce_capture_gate(&mut actions);
                json!({"ok":true,"result":result,"actions":actions}).to_string()
            }
            Err(message) => {
                // Errors cannot retain partially authorized write work.
                let mut actions = vec![json!({"kind":"cancelWrites"})];
                self.drop_observation_requests("adapter error before host answered");
                self.pending.clear();
                // REM is told once per session when a clock is known.
                let tell_host = now.is_some();
                self.end_all_displays(&message, tell_host, now.unwrap_or_default(), &mut actions);
                self.enforce_capture_gate(&mut actions);
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
            "startCapture" | "stopCapture" => self.capture_control(cmd, now, actions),
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

    /// Next per-process capture nonce: `c<process>-<count>`, at most 38
    /// ASCII-graphic characters.
    fn mint_capture_nonce(&mut self) -> Result<String, String> {
        self.capture_count = self
            .capture_count
            .checked_add(1)
            .ok_or("capture identity exhausted")?;
        Ok(format!(
            "c{:016x}-{}",
            self.process_nonce, self.capture_count
        ))
    }

    fn status(&self) -> Value {
        json!({"configured":self.config.is_some(),"connected":self.connection.is_some(),
            "hostStarted":self.host_started,"apiMinor":self.api_minor,"requiredApiMinor":REQUIRED_API_MINOR,
            "hostReady":self.host_ready,"colocationConfirmed":self.colocation.is_some(),
            "capture":{"active":self.capture.is_some(),"session":self.capture,"lastEnded":self.capture_ended},
            "observerFresh":self.observer.is_some(),
            "maxSessions":MAX_DISPLAY_SESSIONS,"freeSlots":self.free_slots(),
            "sessions":self.displays.iter().map(|d| d.sync.snapshot()).collect::<Vec<_>>(),
            "endedSessions":self.ended_sessions,"releasedSlots":self.released_slots,
            "terminalSessions":self.terminal_sessions,
            "inFlight":self.in_flight.as_ref().map(|f| json!({"sessionId":f.session_id,
                "operationId":f.operation_id,"frameNumber":f.frame_number})),
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
