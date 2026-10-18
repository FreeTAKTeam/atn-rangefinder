//! Synthetic exchange against the REM plugin map API 1.3 envelopes inspected at
//! REM `91124cffd8f7fa7df94684b672c2228d97f0522d`. These tests prove adapter
//! behaviour, not real REM map visibility or device rendering.
use atn_adapter::Adapter;
use serde_json::{Value, json};

const SERVICE: &str = "d973f2e0-b19e-11e2-9e96-0800200c9a66";
const NOTIFY: &str = "d973f2e1-b19e-11e2-9e96-0800200c9a66";
const WRITE: &str = "d973f2e2-b19e-11e2-9e96-0800200c9a66";
/// Dispatches `v`, filling in the platform's GATT identity when a test does
/// not state it, as Android always does.
fn call(a: &mut Adapter, mut v: Value) -> Value {
    if v.get("nowMs").is_none() {
        v["nowMs"] = json!(10000);
    }
    let defaults: &[(&str, &str)] = match v["cmd"].as_str() {
        Some("connected") => &[
            ("serviceUuid", SERVICE),
            ("notifyCharacteristic", NOTIFY),
            ("writeCharacteristic", WRITE),
        ],
        Some("notification") => &[("characteristic", NOTIFY)],
        Some("transportResult") => &[("characteristic", WRITE)],
        _ => &[],
    };
    for (key, uuid) in defaults {
        if v.get(*key).is_none() {
            v[*key] = json!(uuid);
        }
    }
    serde_json::from_str(&a.dispatch(&v.to_string())).expect("JSON response")
}
fn config() -> Value {
    let mut c: Value =
        serde_json::from_str(include_str!("../../../examples/synthetic-config.json")).unwrap();
    c["deviceProfile"]["protocolVerified"] = json!(true);
    c["semantics"]["calibrationConfirmed"] = json!(true);
    c["semantics"]["bearingReference"] = json!({"kind":"trueNorth"});
    c["semantics"]["datum"] = json!("wgs84");
    c
}
fn requests(v: &Value, op: &str) -> Vec<Value> {
    v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["kind"] == "hostRequest" && a["envelope"]["operation"] == op)
        .map(|a| a["envelope"].clone())
        .collect()
}
fn request(v: &Value, op: &str) -> Value {
    requests(v, op)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("host request {op} in {v}"))
}
fn has_action(v: &Value, kind: &str) -> bool {
    v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["kind"] == kind)
}
#[allow(clippy::needless_pass_by_value)] // Fixture builders hand over complete results.
fn respond(a: &mut Adapter, request: &Value, result: Value) -> Value {
    respond_at(a, request, result, 10000)
}
#[allow(clippy::needless_pass_by_value)]
fn respond_at(a: &mut Adapter, request: &Value, result: Value, now: u64) -> Value {
    call(
        a,
        json!({"cmd":"hostResponse","nowMs":now,"envelope":{"protocolVersion":1,"requestId":request["requestId"],"ok":true,"result":result}}),
    )
}
#[allow(clippy::needless_pass_by_value)]
fn fail(a: &mut Adapter, request: &Value, code: &str) -> Value {
    call(
        a,
        json!({"cmd":"hostResponse","envelope":{"protocolVersion":1,"requestId":request["requestId"],"ok":false,"error":{"code":code,"message":"synthetic"}}}),
    )
}
fn snapshot(lon: f64, updated_at_ms: u64) -> Value {
    json!({"available":true,"observer":{"kind":"localDevice","identityHex":"00","lxmfDestinationHex":"00"},
        "source":"device","datum":"WGS84","lat":0.0,"lon":lon,"alt":null,"horizontalAccuracyM":1.0,
        "course":null,"speed":null,"updatedAtMs":updated_at_ms,"ageMs":0})
}
fn object(id: &str, lon: f64, updated_at: &str) -> Value {
    json!({"object_destination_hash":id,"origin_rch":"synthetic-origin","type":"marker","name":"Synthetic point",
        "category":"marker","symbol":"marker","notes":null,"position":{"lat":0.0,"lon":lon},
        "time":updated_at,"stale_at":updated_at,"created_at":"2026-10-16T12:00:00Z","updated_at":updated_at,"textTruncated":false})
}
const T1: &str = "2026-10-16T12:34:56Z";
const T2: &str = "2026-10-16T12:35:56Z";
fn session(id: &str, ids: &[&str]) -> Value {
    json!({"sessionId":id,"pluginId":"org.freetak.atn","objectDestinationHashes":ids,"startedAtMs":9000,"startedBy":"operator"})
}
fn started(a: &mut Adapter, id: &str, objects: &[Value]) -> Value {
    let ids: Vec<String> = objects
        .iter()
        .map(|o| o["object_destination_hash"].as_str().unwrap().to_owned())
        .collect();
    let mut payload = session(id, &ids.iter().map(String::as_str).collect::<Vec<_>>());
    payload["objects"] = json!(objects);
    call(
        a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.started","payload":payload}}),
    )
}
fn connected() -> Adapter {
    let mut a = Adapter::new();
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":config()}))["ok"],
        true
    );
    assert_eq!(
        call(
            &mut a,
            json!({"cmd":"connected","connectionId":"synthetic-connection","writeCapacity":509})
        )["ok"],
        true
    );
    a
}
fn ready(a: &mut Adapter) {
    let start = call(a, json!({"cmd":"hostStarted","apiMinor":3}));
    let symbols = request(&start, "map.symbols.list");
    let v = respond(
        a,
        &symbols,
        json!({"items":[{"id":"marker","set":"rch","description":"Generic marker","category":"marker","aliases":["pin"]}]}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["hostReady"], true);
    let list = request(&v, "map.session.list");
    let v = respond(a, &list, json!({"items":[]}));
    assert_eq!(v["ok"], true);
}
fn colocate(a: &mut Adapter) {
    let v = call(
        a,
        json!({"cmd":"confirmColocation","evidenceId":"operator at binocular"}),
    );
    assert_eq!(v["ok"], true, "{v}");
    if let Some(list) = requests(&v, "map.session.list").first() {
        respond(a, list, json!({"items":[]}));
    }
}
fn start_capture(a: &mut Adapter) -> Value {
    let v = call(a, json!({"cmd":"startCapture"}));
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["captureStarted"], true, "{v}");
    v
}
fn capture_status(a: &mut Adapter) -> Value {
    call(a, json!({"cmd":"status"}))["result"]["capture"].clone()
}
/// Ready, co-located and capturing on `synthetic-connection`.
fn capturing() -> Adapter {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    start_capture(&mut a);
    a
}
/// Takes one observation through to a `map.markers.create` request.
fn create_request(a: &mut Adapter) -> Value {
    let v = placed(a);
    request(&v, "map.markers.create")
}
/// Observes once; a still-fresh position places it at once, otherwise REM's
/// snapshot is answered. Returns the response holding `map.markers.create`.
fn placed(a: &mut Adapter) -> Value {
    let v = observe(a);
    if requests(&v, "map.markers.create").is_empty() {
        respond(
            a,
            &request(&v, "map.observer.snapshot"),
            snapshot(0.0, 10000),
        )
    } else {
        v
    }
}
fn host_state(a: &mut Adapter, index: usize) -> Value {
    call(a, json!({"cmd":"status"}))["result"]["observations"][index]["host"].clone()
}
/// Operator shares one point; the adapter authorizes slot 2 and emits the first write.
fn sync(a: &mut Adapter) -> Value {
    colocate(a);
    let v = started(
        a,
        "synthetic-session",
        &[object("synthetic-ple", 0.001, T1)],
    );
    assert_eq!(v["ok"], true, "{v}");
    assert!(has_action(&v, "sessionAuthorized"));
    respond(
        a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    )
}
/// An accepted session in a status-bearing result.
fn session_in(v: &Value, id: &str) -> Value {
    v["result"]["sessions"]
        .as_array()
        .unwrap_or_else(|| panic!("sessions in {v}"))
        .iter()
        .find(|s| s["sessionId"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}
/// The latest snapshot of an ended session.
fn ended_in(v: &Value, id: &str) -> Value {
    v["result"]["endedSessions"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|s| s["sessionId"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("ended session {id} in {v}"))
}
fn write(v: &Value) -> Value {
    v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "write")
        .unwrap_or_else(|| panic!("write action in {v}"))
        .clone()
}
fn transport_ok(a: &mut Adapter, write: &Value, now: u64) -> Value {
    call(
        a,
        json!({"cmd":"transportResult","nowMs":now,"connectionId":write["connectionId"],"sessionId":write["sessionId"],"operationId":write["operationId"],"frameNumber":1,"completed":true}),
    )
}
fn observe(a: &mut Adapter) -> Value {
    call(
        a,
        json!({"cmd":"notification","bytes":[1,1,0,120,0,0,0,90],"observedAtMs":10000}),
    )
}

#[test]
fn raw_notification_survives_unavailable_host() {
    let mut a = connected();
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,1,0,120,255,251,0,210],"observedAtMs":10000}),
    );
    assert_eq!(v["result"]["observation"]["raw"]["pitch"], -5);
    assert_eq!(
        v["result"]["observation"]["sourceSession"],
        "synthetic-connection"
    );
    assert_eq!(v["result"]["host"]["state"], "retained");
    assert_eq!(v["actions"], json!([]));
}

#[test]
fn older_host_api_never_negotiates_map_exchange() {
    let mut a = connected();
    let v = call(&mut a, json!({"cmd":"hostStarted","apiMinor":1}));
    assert_eq!(v["ok"], true);
    assert_eq!(v["result"]["apiSupported"], false);
    assert!(!has_action(&v, "hostRequest"));
    assert!(has_action(&v, "status"));
    let v = started(&mut a, "s", &[object("p", 0.001, T1)]);
    assert_eq!(v["result"]["ignored"], "map host unavailable");
    let v = observe(&mut a);
    assert_eq!(v["result"]["host"]["state"], "retained");
}

#[test]
fn unknown_catalog_symbol_keeps_adapter_read_only() {
    let mut a = connected();
    let start = call(&mut a, json!({"cmd":"hostStarted","apiMinor":3}));
    let v = respond(
        &mut a,
        &request(&start, "map.symbols.list"),
        json!({"items":[{"id":"fire","aliases":[]}]}),
    );
    assert_eq!(v["ok"], false);
    assert_eq!(
        call(&mut a, json!({"cmd":"status"}))["result"]["hostReady"],
        false
    );
    let v = started(&mut a, "s", &[object("p", 0.001, T1)]);
    assert_eq!(v["result"]["ignored"], "map host unavailable");
}

#[test]
fn permission_denial_on_catalog_fails_closed() {
    let mut a = connected();
    let start = call(&mut a, json!({"cmd":"hostStarted","apiMinor":3}));
    let v = fail(
        &mut a,
        &request(&start, "map.symbols.list"),
        "PermissionDenied",
    );
    assert_eq!(v["ok"], false);
    assert_eq!(
        call(&mut a, json!({"cmd":"status"}))["result"]["hostReady"],
        false
    );
}

#[test]
fn observation_creates_one_idempotent_shared_marker() {
    let mut a = capturing();
    let v = observe(&mut a);
    assert_eq!(v["result"]["host"]["state"], "observerRequested");
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let create = request(&v, "map.markers.create");
    let payload = &create["payload"];
    let key = payload["idempotencyKey"].as_str().unwrap();
    assert!(key.contains("/synthetic-connection/"));
    assert_eq!(payload["symbol"], "marker");
    assert!(
        payload["name"]
            .as_str()
            .unwrap()
            .starts_with("ATN observation")
    );
    assert!(payload["lon"].as_f64().unwrap() > 0.0);
    assert!(payload["lat"].as_f64().unwrap().abs() < 1e-9);
    assert!(payload.get("category").is_none());
    let notes = payload["notes"].as_str().unwrap();
    assert!(notes.chars().count() <= 1024);
    assert!(notes.contains("operator at binocular"));
    assert!(!notes.contains("identityHex"));
    let v = respond(
        &mut a,
        &create,
        json!({"marker":object("host-minted",0.0,T1),"created":true}),
    );
    let host = &v["result"]["observations"][0]["host"];
    assert_eq!(host["state"], "created");
    assert_eq!(host["objectDestinationHash"], "host-minted");
    // A replayed key returns the original marker without a second creation.
    let v = placed(&mut a);
    let second = request(&v, "map.markers.create");
    assert_ne!(second["payload"]["idempotencyKey"], key);
    let v = respond(
        &mut a,
        &second,
        json!({"marker":object("host-minted",0.0,T1),"created":false}),
    );
    assert_eq!(
        v["result"]["observations"][1]["host"]["state"],
        "alreadyExisted"
    );
}

#[test]
fn ineligible_observer_retains_raw_without_marker_creation() {
    let mut a = capturing();
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 0),
    );
    assert_eq!(v["ok"], true);
    assert!(requests(&v, "map.markers.create").is_empty());
    assert_eq!(v["result"]["observations"][0]["host"]["state"], "retained");
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        json!({"available":false,"observer":{"kind":"localDevice"}}),
    );
    assert_eq!(v["result"]["observations"][1]["host"]["state"], "retained");
    let v = observe(&mut a);
    let mut poor = snapshot(0.0, 10000);
    poor["horizontalAccuracyM"] = Value::Null;
    let v = respond(&mut a, &request(&v, "map.observer.snapshot"), poor);
    let host = &v["result"]["observations"][2]["host"];
    assert_eq!(host["state"], "retained");
    assert!(host["reason"].as_str().unwrap().contains("accuracy"));
}

#[test]
fn unconfirmed_colocation_blocks_creation_and_rejects_sessions() {
    let mut a = connected();
    ready(&mut a);
    let v = call(&mut a, json!({"cmd":"startCapture"}));
    assert_eq!(v["ok"], true);
    assert_eq!(v["result"]["captureStarted"], false);
    assert!(
        v["result"]["reason"]
            .as_str()
            .unwrap()
            .contains("binocular")
    );
    let v = observe(&mut a);
    assert!(!has_action(&v, "hostRequest"));
    let host = &v["result"]["host"];
    assert_eq!(host["state"], "retained");
    assert_eq!(host["reason"], "capture not started");
    let v = started(&mut a, "shared-early", &[object("p", 0.001, T1)]);
    assert_eq!(v["ok"], true);
    assert!(!has_action(&v, "sessionAuthorized"));
    let stop = request(&v, "map.session.stop");
    assert_eq!(stop["payload"]["sessionId"], "shared-early");
    assert_eq!(v["result"]["sessions"], json!([]));
    // The rejected ID is terminal even if REM re-reports it.
    let v = started(&mut a, "shared-early", &[object("p", 0.001, T1)]);
    assert!(requests(&v, "map.session.stop").is_empty());
    assert!(!has_action(&v, "sessionAuthorized"));
}

#[test]
fn create_rejection_timeout_and_binder_failure_stay_distinct() {
    let mut a = capturing();
    let v = placed(&mut a);
    let v = fail(&mut a, &request(&v, "map.markers.create"), "InvalidRequest");
    assert_eq!(v["ok"], true);
    let host = &v["result"]["observations"][0]["host"];
    assert_eq!(host["state"], "rejected");
    assert_eq!(host["code"], "InvalidRequest");
    let v = placed(&mut a);
    let create = request(&v, "map.markers.create");
    let v = call(&mut a, json!({"cmd":"tick","nowMs":12001}));
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["observations"][1]["host"]["state"], "unknown");
    // A late answer to an expired request is ignored, not applied.
    let v = respond_at(
        &mut a,
        &create,
        json!({"marker":object("late",0.0,T1),"created":true}),
        12001,
    );
    assert_eq!(v["result"]["ignored"], "unmatched host response");
    let v = placed(&mut a);
    let v = fail(&mut a, &request(&v, "map.markers.create"), "binder.timeout");
    let host = &v["result"]["observations"][2]["host"];
    assert_eq!(host["state"], "unknown");
    assert_eq!(host["resolution"], "replayScheduled");
    let v = placed(&mut a);
    let v = fail(
        &mut a,
        &request(&v, "map.markers.create"),
        "PermissionDenied",
    );
    // A missing marker grant only stops measurements; REM stays usable
    // (it used to make the whole adapter read-only).
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["hostReady"], true);
    assert!(
        v["result"]["capture"]["lastEnded"]
            .as_str()
            .unwrap()
            .contains("Add and edit its own map markers")
    );
    // Revocation ended capture, so the scheduled replay can never run.
    let host = host_state(&mut a, 2);
    assert_eq!(host["state"], "unknown");
    assert_eq!(host["resolution"], "final");
    assert_eq!(capture_status(&mut a)["active"], false);
}

#[test]
fn session_allocates_slots_in_selection_order_and_writes() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = started(
        &mut a,
        "two",
        &[object("b", 0.001, T1), object("a", 0.002, T1)],
    );
    let selected = session_in(&v, "two")["selected"]
        .as_array()
        .unwrap()
        .clone();
    let slot = |id: &str| {
        selected
            .iter()
            .find(|s| s["pleId"] == id)
            .map(|s| s["slotId"].as_u64().unwrap())
            .unwrap()
    };
    assert_eq!(slot("b"), 2);
    assert_eq!(slot("a"), 3);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let w = write(&v);
    assert!([4, 6].contains(&w["bytes"][2].as_u64().unwrap())); // slot 2 or 3, type 0
    assert_eq!(w["sessionId"], "two");
    assert_eq!(w["connectionId"], "synthetic-connection");
}

#[test]
// Stage 3: a second session is now served alongside the first (it was
// rejected while only one session was supported).
fn second_session_gets_its_own_slots_and_existing_session_is_untouched() {
    let mut a = connected();
    ready(&mut a);
    let first = sync(&mut a);
    let w = write(&first);
    let v = started(&mut a, "second", &[object("other", 0.001, T1)]);
    assert!(requests(&v, "map.session.stop").is_empty());
    assert!(has_action(&v, "sessionAuthorized"));
    assert!(!has_action(&v, "cancelWrites"));
    assert!(!has_action(&v, "write")); // the first session's frame is in flight
    assert_eq!(session_in(&v, "second")["selected"][0]["slotId"], 3);
    assert_eq!(
        session_in(&v, "synthetic-session")["selected"][0]["slotId"],
        2
    );
    let v = started(&mut a, "second", &[object("other", 0.001, T1)]);
    assert!(requests(&v, "map.session.stop").is_empty());
    let v = transport_ok(&mut a, &w, 10000);
    assert_eq!(v["result"]["report"]["transportComplete"], true);
}

#[test]
fn exhausted_slot_pool_rejects_whole_session() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let objects: Vec<Value> = (0..5)
        .map(|i| object(&format!("p{i}"), 0.001, T1))
        .collect();
    let v = started(&mut a, "five", &objects);
    assert_eq!(v["ok"], true);
    assert_eq!(v["result"]["sessions"], json!([]));
    assert_eq!(
        request(&v, "notifications.raise")["payload"]["body"],
        "Share rejected: configured slot pool exhausted"
    );
    assert_eq!(
        request(&v, "map.session.stop")["payload"]["sessionId"],
        "five"
    );
    assert!(!has_action(&v, "sessionAuthorized"));
}

#[test]
fn session_events_update_remove_and_stop_within_scope() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let w = write(&v);
    transport_ok(&mut a, &w, 10000);
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,2,3,0,0,0],"observedAtMs":10000}),
    );
    assert_eq!(v["result"]["report"]["displayConfirmation"], Value::Null);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"synthetic-session","reason":"moved","object":object("unselected",0.002,T2)}}}),
    );
    assert_eq!(v["actions"], json!([]));
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"other-session","reason":"moved","object":object("synthetic-ple",0.002,T2)}}}),
    );
    assert_eq!(v["result"]["ignored"], "different session");
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"synthetic-session","reason":"moved","object":object("synthetic-ple",0.002,T2)}}}),
    );
    let w = write(&v);
    transport_ok(&mut a, &w, 10000);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.removed","payload":{"sessionId":"synthetic-session","objectDestinationHash":"synthetic-ple"}}}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(session_in(&v, "synthetic-session"), Value::Null);
    assert_eq!(
        ended_in(&v, "synthetic-session")["state"]["state"],
        "stopped"
    );
    // Stage 3: one session ending no longer cancels other sessions' writes.
    assert!(has_action(&v, "sessionEnded"));
    assert!(requests(&v, "map.session.stop").is_empty()); // REM already owns the removal
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.stopped","payload":{"sessionId":"synthetic-session","reason":"operator"}}}),
    );
    assert_eq!(v["ok"], true);
    let v = started(
        &mut a,
        "synthetic-session",
        &[object("synthetic-ple", 0.001, T1)],
    );
    assert!(!has_action(&v, "sessionAuthorized"));
    assert!(requests(&v, "map.session.stop").is_empty());
}

#[test]
fn rem_stop_event_cancels_unsent_work_without_device_clear() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.stopped","payload":{"sessionId":"synthetic-session","reason":"capability-revoked"}}}),
    );
    // Stage 3: the per-session end replaces the global cancellation.
    assert!(has_action(&v, "sessionEnded"));
    assert!(!has_action(&v, "write"));
    assert!(requests(&v, "map.session.stop").is_empty());
    let ended = ended_in(&v, "synthetic-session");
    let state = &ended["state"];
    assert_eq!(state["state"], "stopped");
    assert!(
        state["reason"]
            .as_str()
            .unwrap()
            .contains("capability-revoked")
    );
    assert!(ended["completed"][0]["cancelled"].is_string());
    let released = v["result"]["releasedSlots"].as_array().unwrap();
    assert_eq!(released[0]["slotId"], 2);
    assert_eq!(released[0]["deviceDisplayCleared"], false);
}

#[test]
fn dropped_started_event_is_recovered_from_session_list() {
    let mut a = connected();
    ready(&mut a);
    let v = call(
        &mut a,
        json!({"cmd":"confirmColocation","evidenceId":"operator at binocular"}),
    );
    let list = request(&v, "map.session.list");
    let v = respond(
        &mut a,
        &list,
        json!({"items":[session("listed",&["synthetic-ple","gone"])]}),
    );
    assert!(has_action(&v, "sessionAuthorized"));
    let objects = request(&v, "map.session.objects.get");
    assert_eq!(objects["payload"]["sessionId"], "listed");
    respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    // REM omits objects that no longer exist; they leave the selection.
    let v = respond(
        &mut a,
        &objects,
        json!({"session":session("listed",&["synthetic-ple","gone"]),"objects":[object("synthetic-ple",0.001,T1)]}),
    );
    assert_eq!(v["ok"], true, "{v}");
    let session = session_in(&v, "listed");
    let selected = session["selected"].as_array().unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["pleId"], "synthetic-ple");
    assert_eq!(write(&v)["bytes"][2], 4);
}

#[test]
fn unknown_session_answer_ends_local_session() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let list = call(&mut a, json!({"cmd":"disconnected"}));
    assert!(requests(&list, "map.session.stop").is_empty());
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"second","writeCapacity":509}),
    );
    let v = call(
        &mut a,
        json!({"cmd":"confirmColocation","evidenceId":"operator at binocular"}),
    );
    let v = respond(
        &mut a,
        &request(&v, "map.session.list"),
        json!({"items":[session("listed",&["p"])]}),
    );
    let v = fail(&mut a, &request(&v, "map.session.objects.get"), "NotFound");
    assert_eq!(v["ok"], true);
    assert_eq!(ended_in(&v, "listed")["state"]["state"], "stopped");
    assert!(has_action(&v, "sessionEnded"));
}

#[test]
fn disconnect_stops_rem_session_and_old_callbacks_never_replay() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let old = write(&v)["operationId"].clone();
    let v = call(&mut a, json!({"cmd":"disconnected"}));
    assert_eq!(
        request(&v, "map.session.stop")["payload"]["sessionId"],
        "synthetic-session"
    );
    assert_eq!(
        call(&mut a, json!({"cmd":"status"}))["result"]["colocationConfirmed"],
        false
    );
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"new-synthetic-connection","writeCapacity":509}),
    );
    let v = call(
        &mut a,
        json!({"cmd":"transportResult","sessionId":"synthetic-session","connectionId":"synthetic-connection","operationId":old,"frameNumber":1,"completed":true}),
    );
    assert_eq!(v["ok"], true);
    assert_eq!(v["result"]["ignored"], "stale transport callback");
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"synthetic-session","reason":"moved","object":object("synthetic-ple",0.003,T2)}}}),
    );
    assert!(!has_action(&v, "write"));
    // The old session is terminal: REM re-reporting it cannot re-authorize writes.
    colocate(&mut a);
    let v = started(
        &mut a,
        "synthetic-session",
        &[object("synthetic-ple", 0.001, T1)],
    );
    assert!(!has_action(&v, "sessionAuthorized"));
}

#[test]
fn operator_stop_asks_rem_to_stop_without_device_clear() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let v = call(&mut a, json!({"cmd":"stopSync"}));
    let r = request(&v, "map.session.stop");
    assert_eq!(r["payload"]["sessionId"], "synthetic-session");
    assert!(!has_action(&v, "write"));
    let v = respond(&mut a, &r, json!({"stopped":true}));
    assert_eq!(v["ok"], true);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":13000}));
    assert_eq!(v["ok"], true);
    assert_eq!(v["actions"], json!([]));
}

#[test]
fn stale_observer_holds_writes_and_requests_a_fresh_snapshot() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let w = write(&v);
    let v = transport_ok(&mut a, &w, 11001);
    assert_eq!(v["ok"], true, "{v}");
    assert!(!has_action(&v, "write"));
    assert!(!has_action(&v, "cancelWrites"));
    let refresh = request(&v, "map.observer.snapshot");
    let v = call(&mut a, json!({"cmd":"tick","nowMs":11002}));
    assert!(requests(&v, "map.observer.snapshot").is_empty()); // one outstanding at a time
    let v = respond_at(&mut a, &refresh, snapshot(0.0, 11000), 11002);
    assert_eq!(v["ok"], true);
}

#[test]
fn tick_refreshes_observer_during_an_active_session() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10600}));
    assert!(!requests(&v, "map.observer.snapshot").is_empty());
}

#[test]
fn native_input_size_and_malformed_config_fail_closed() {
    let mut a = Adapter::new();
    let result: Value = serde_json::from_str(&a.dispatch(&"x".repeat(393_217))).unwrap();
    assert_eq!(result["ok"], false);
    let mut c = config();
    c["transport"]["maxPendingHostRequests"] = json!(0);
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        false
    );
    let mut c = config();
    c["slotPool"] = json!([2, 2]);
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        false
    );
    let mut c = config();
    c["slotPool"] = json!([128]);
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        false
    );
    let mut c = config();
    c["hostMarker"]["symbol"] = json!("");
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        false
    );
}

#[test]
fn pending_negotiation_deadline_fails_closed() {
    let mut a = connected();
    let v = call(&mut a, json!({"cmd":"hostStarted","apiMinor":3}));
    assert_eq!(v["ok"], true);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":12000}));
    assert_eq!(v["ok"], false);
    assert!(has_action(&v, "cancelWrites"));
}

#[test]
fn early_reply_and_platform_completion_remain_separate() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let w = write(&v);
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,2,3,0,0,0],"observedAtMs":10000}),
    );
    assert_eq!(v["result"]["report"]["transportComplete"], false);
    assert_eq!(v["result"]["report"]["reply"]["answer_signed"], 3);
    let v = transport_ok(&mut a, &w, 10000);
    assert_eq!(v["result"]["report"]["transportComplete"], true);
    assert_eq!(v["result"]["report"]["displayConfirmation"], Value::Null);
}

#[test]
fn style_changes_queue_position_and_icon_is_selected_only() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let w = write(&v);
    transport_ok(&mut a, &w, 10000);
    call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,2,3,0,0,0],"observedAtMs":10000}),
    );
    let v = call(&mut a, json!({"cmd":"setStyle","markerType":1,"color":3}));
    let w = write(&v);
    assert_eq!(w["bytes"][2], 5);
    assert_eq!(w["bytes"][3], 19);
    let png = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png").to_vec();
    let v = call(
        &mut a,
        json!({"cmd":"setIcon","sessionId":"synthetic-session","pleId":"synthetic-ple","png":png}),
    );
    assert_eq!(v["ok"], true);
    assert_eq!(v["result"]["accepted"], true);
    assert!(!has_action(&v, "write")); // waits for position reply, separate operation
    let v = call(
        &mut a,
        json!({"cmd":"setIcon","sessionId":"synthetic-session","pleId":"not-selected","png":png}),
    );
    // Stage 3: a mistyped object is refused without ending every session.
    assert_eq!(v["ok"], true);
    assert_eq!(v["result"]["accepted"], false);
    assert!(!has_action(&v, "cancelWrites"));
}

#[test]
fn removing_a_queued_selected_point_preserves_another_active_write() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = started(
        &mut a,
        "two-selected",
        &[object("a", 0.001, T1), object("b", 0.002, T1)],
    );
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    assert_eq!(write(&v)["bytes"][2], 4);
    assert_eq!(session_in(&v, "two-selected")["queuedOperations"], 1);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.removed","payload":{"sessionId":"two-selected","objectDestinationHash":"b"}}}),
    );
    assert!(!has_action(&v, "write"));
    assert!(!has_action(&v, "cancelWrites"));
    let session = session_in(&v, "two-selected");
    assert_eq!(session["active"]["pleId"], "a");
    assert_eq!(session["queuedOperations"], 0);
}

#[test]
fn stale_object_update_is_refused_and_same_time_content_is_compared() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    transport_ok(&mut a, &write(&v), 10000);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"synthetic-session","reason":"moved","object":object("synthetic-ple",0.002,T2)}}}),
    );
    assert_eq!(v["ok"], true);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"synthetic-session","reason":"moved","object":object("synthetic-ple",0.003,T1)}}}),
    );
    assert_eq!(v["ok"], false);
    assert!(has_action(&v, "cancelWrites"));
    assert_eq!(
        request(&v, "map.session.stop")["payload"]["sessionId"],
        "synthetic-session"
    );
}

#[test]
fn unparseable_host_timestamp_is_rejected() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = started(&mut a, "bad-time", &[object("p", 0.001, "yesterday")]);
    assert_eq!(v["ok"], false);
}

#[test]
fn configured_limits_reject_unbounded_allocation_and_inconsistent_png_dimensions() {
    let mut a = Adapter::new();
    let mut c = config();
    c["pngLimits"]["maxDecodedBytes"] = json!(usize::MAX);
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        false
    );
    let mut c = config();
    c["pngLimits"]["expectedDimensions"] = json!([41, 40]);
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        false
    );
}

#[test]
fn connecting_during_negotiation_preserves_host_lifetime_request() {
    let mut a = Adapter::new();
    call(&mut a, json!({"cmd":"configure","config":config()}));
    let v = call(&mut a, json!({"cmd":"hostStarted","apiMinor":3}));
    let symbols = request(&v, "map.symbols.list");
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"fresh","writeCapacity":509}),
    );
    let v = respond(
        &mut a,
        &symbols,
        json!({"items":[{"id":"pin","aliases":["marker"]}]}),
    );
    assert_eq!(v["result"]["hostReady"], true);
}

#[test]
fn configuration_change_ends_session_and_renegotiates() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let v = call(&mut a, json!({"cmd":"configure","config":config()}));
    assert_eq!(v["ok"], true);
    assert!(has_action(&v, "cancelWrites"));
    assert_eq!(
        request(&v, "map.session.stop")["payload"]["sessionId"],
        "synthetic-session"
    );
    request(&v, "map.symbols.list");
    let v = call(&mut a, json!({"cmd":"status"}));
    assert_eq!(v["result"]["hostReady"], false);
    assert_eq!(v["result"]["colocationConfirmed"], false);
}

#[test]
fn moving_observer_replaces_unsent_geometry_before_next_transfer() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let first = write(&v);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","nowMs":10500,"envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"synthetic-session","reason":"moved","object":object("synthetic-ple",0.002,T2)}}}),
    );
    assert_eq!(session_in(&v, "synthetic-session")["queuedOperations"], 1);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10500}));
    let refresh = request(&v, "map.observer.snapshot");
    let v = respond_at(&mut a, &refresh, snapshot(0.0005, 10500), 10500);
    assert_eq!(session_in(&v, "synthetic-session")["queuedOperations"], 1);
    transport_ok(&mut a, &first, 10500);
    let v = call(
        &mut a,
        json!({"cmd":"notification","nowMs":10500,"observedAtMs":10500,"bytes":[1,2,3,0,0,0]}),
    );
    let bytes = &write(&v)["bytes"];
    let distance = i16::from_le_bytes([
        u8::try_from(bytes[4].as_u64().unwrap()).unwrap(),
        u8::try_from(bytes[5].as_u64().unwrap()).unwrap(),
    ]);
    assert_eq!(distance, 167); // fresh observer at 0.0005deg -> selected point 0.002deg
}

#[test]
fn host_stop_and_restart_clear_everything_without_replay() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let v = call(&mut a, json!({"cmd":"hostStopped"}));
    assert!(has_action(&v, "disconnect"));
    assert!(requests(&v, "map.session.stop").is_empty()); // the host is gone
    let v = call(&mut a, json!({"cmd":"status"}))["result"].clone();
    assert_eq!(v["hostReady"], false);
    assert_eq!(v["connected"], false);
    assert_eq!(v["apiMinor"], Value::Null);
    let v = call(&mut a, json!({"cmd":"hostStarted","apiMinor":3}));
    request(&v, "map.symbols.list");
    assert!(!has_action(&v, "write"));
}

#[test]
fn measurements_stay_raw_until_capture_starts() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = observe(&mut a);
    assert_eq!(v["actions"], json!([]));
    assert_eq!(v["result"]["host"]["state"], "retained");
    assert_eq!(v["result"]["host"]["reason"], "capture not started");
    assert_eq!(v["result"]["observation"]["raw"]["distance"], 120);
    let status = capture_status(&mut a);
    assert_eq!(status["active"], false);
    assert_eq!(status["session"], Value::Null);
    let v = start_capture(&mut a);
    let session = &v["result"]["status"]["capture"]["session"];
    assert_eq!(session["connectionId"], "synthetic-connection");
    assert_eq!(session["startedAtMs"], 10000);
    // Starting capture never offers the earlier measurement to REM.
    assert!(!has_action(&v, "hostRequest"));
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10600}));
    assert!(requests(&v, "map.observer.snapshot").is_empty());
    assert!(requests(&v, "map.markers.create").is_empty());
    assert_eq!(host_state(&mut a, 0)["reason"], "capture not started");
    let v = observe(&mut a);
    assert_eq!(v["result"]["host"]["state"], "observerRequested");
}

#[test]
fn start_capture_requires_host_connection_profile_semantics_and_colocation() {
    let refused = |a: &mut Adapter, needle: &str| {
        let v = call(a, json!({"cmd":"startCapture"}));
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["result"]["captureStarted"], false, "{v}");
        assert!(!has_action(&v, "cancelWrites"));
        let reason = v["result"]["reason"].as_str().unwrap();
        assert!(reason.contains(needle), "{reason}");
    };
    let mut a = connected();
    refused(&mut a, "REM map API");
    ready(&mut a);
    refused(&mut a, "binocular");
    call(&mut a, json!({"cmd":"disconnected"}));
    refused(&mut a, "disconnected");
    let mut a = Adapter::new();
    let mut c = config();
    c["deviceProfile"]["protocolVerified"] = json!(false);
    call(&mut a, json!({"cmd":"configure","config":c}));
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"synthetic-connection","writeCapacity":509}),
    );
    ready(&mut a);
    colocate(&mut a);
    refused(&mut a, "not verified");
    let mut a = Adapter::new();
    let mut c = config();
    c["semantics"]["calibrationConfirmed"] = json!(false);
    call(&mut a, json!({"cmd":"configure","config":c}));
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"synthetic-connection","writeCapacity":509}),
    );
    ready(&mut a);
    colocate(&mut a);
    let v = call(&mut a, json!({"cmd":"startCapture"}));
    assert_eq!(v["result"]["captureStarted"], false, "{v}");
    // A second start keeps the active capture and its nonce.
    let mut a = capturing();
    let nonce = capture_status(&mut a)["session"]["nonce"].clone();
    refused(&mut a, "already active");
    assert_eq!(capture_status(&mut a)["session"]["nonce"], nonce);
}

#[test]
fn capture_ends_on_every_authorization_change() {
    let triggers: Vec<(&str, Value)> = vec![
        ("operator stopped capture", json!({"cmd":"stopCapture"})),
        ("BLE disconnected", json!({"cmd":"disconnected"})),
        (
            "new BLE connection",
            json!({"cmd":"connected","connectionId":"other","writeCapacity":509}),
        ),
        ("host stopped", json!({"cmd":"hostStopped"})),
        ("host restarted", json!({"cmd":"hostStarted","apiMinor":3})),
        (
            "configuration changed",
            json!({"cmd":"configure","config":config()}),
        ),
        (
            "co-location confirmation withdrawn",
            json!({"cmd":"clearColocation"}),
        ),
    ];
    for (reason, command) in triggers {
        let mut a = capturing();
        let v = call(&mut a, command);
        assert_eq!(v["ok"], true, "{reason}: {v}");
        let status = capture_status(&mut a);
        assert_eq!(status["active"], false, "{reason}");
        assert_eq!(status["lastEnded"], reason);
        let v = observe(&mut a);
        if v["ok"] == true {
            assert_eq!(v["result"]["host"]["reason"], "capture not started");
            assert!(requests(&v, "map.observer.snapshot").is_empty());
        }
    }
    // A refused observer snapshot ends capture too, naming the REM grant,
    // without making REM unusable.
    let mut a = capturing();
    let v = observe(&mut a);
    let v = fail(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        "PermissionDenied",
    );
    assert_eq!(v["ok"], true, "{v}");
    let status = capture_status(&mut a);
    assert_eq!(status["active"], false);
    assert!(
        status["lastEnded"]
            .as_str()
            .unwrap()
            .contains("Read this device's location")
    );
}

#[test]
fn idempotency_key_carries_a_fresh_capture_nonce() {
    let mut a = capturing();
    let first_nonce = capture_status(&mut a)["session"]["nonce"]
        .as_str()
        .unwrap()
        .to_owned();
    let (process, count) = first_nonce[1..].split_once('-').unwrap();
    assert!(first_nonce.starts_with('c'));
    assert_eq!(process.len(), 16);
    assert!(process.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(count, "1");
    let create = create_request(&mut a);
    let key = create["payload"]["idempotencyKey"].as_str().unwrap();
    let parts: Vec<&str> = key.split('/').collect();
    assert_eq!(parts[0], first_nonce);
    assert_eq!(parts[1], "synthetic-connection");
    assert!(parts[2].parse::<u64>().is_ok());
    assert!(key.len() <= 128 && key.bytes().all(|b| b.is_ascii_graphic()));
    call(&mut a, json!({"cmd":"stopCapture"}));
    start_capture(&mut a);
    let second_nonce = capture_status(&mut a)["session"]["nonce"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(second_nonce, first_nonce);
    assert_eq!(second_nonce, format!("c{process}-2"));
    let second = create_request(&mut a);
    assert!(
        second["payload"]["idempotencyKey"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{second_nonce}/"))
    );
    // Another process draws a different random component.
    let mut b = capturing();
    let other = capture_status(&mut b)["session"]["nonce"].clone();
    assert_ne!(other, json!(first_nonce));
}

#[test]
fn unknown_creation_is_replayed_once_with_the_same_key_and_payload() {
    let mut a = capturing();
    let create = create_request(&mut a);
    let v = fail(&mut a, &create, "HostError");
    assert_eq!(v["ok"], true);
    let host = &v["result"]["observations"][0]["host"];
    assert_eq!(host["state"], "unknown");
    assert_eq!(host["resolution"], "replayScheduled");
    // Nothing before the configured delay (synthetic 500 ms).
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10499}));
    assert!(requests(&v, "map.markers.create").is_empty());
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10500}));
    let replay = request(&v, "map.markers.create");
    assert_eq!(replay["payload"], create["payload"]);
    assert_ne!(replay["requestId"], create["requestId"]);
    assert_eq!(host_state(&mut a, 0)["state"], "replayRequested");
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10600}));
    assert!(requests(&v, "map.markers.create").is_empty());
    // REM recognises the key and returns the original marker.
    let v = respond_at(
        &mut a,
        &replay,
        json!({"marker":object("host-minted",0.0,T1),"created":false}),
        10600,
    );
    let host = &v["result"]["observations"][0]["host"];
    assert_eq!(host["state"], "alreadyExisted");
    assert_eq!(host["objectDestinationHash"], "host-minted");
}

#[test]
fn a_failed_replay_stays_unknown_and_is_never_repeated() {
    let mut a = capturing();
    let create = create_request(&mut a);
    fail(&mut a, &create, "binder.timeout");
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10500}));
    let replay = request(&v, "map.markers.create");
    // The replay times out as well (synthetic hostRequestTimeoutMs 2000).
    let v = call(&mut a, json!({"cmd":"tick","nowMs":12500}));
    assert_eq!(v["ok"], true, "{v}");
    assert!(requests(&v, "map.markers.create").is_empty());
    let host = host_state(&mut a, 0);
    assert_eq!(host["state"], "unknown");
    assert_eq!(host["resolution"], "final");
    let v = call(&mut a, json!({"cmd":"tick","nowMs":20000}));
    assert!(requests(&v, "map.markers.create").is_empty());
    let v = respond_at(
        &mut a,
        &replay,
        json!({"marker":object("late",0.0,T1),"created":true}),
        20000,
    );
    assert_eq!(v["result"]["ignored"], "unmatched host response");
}

#[test]
fn replay_is_withdrawn_by_host_restart() {
    let mut a = capturing();
    let create = create_request(&mut a);
    fail(&mut a, &create, "HostError");
    ready(&mut a); // hostStarted: a new Binder host session
    let host = host_state(&mut a, 0);
    assert_eq!(host["state"], "unknown");
    assert_eq!(host["resolution"], "final");
    assert!(host["reason"].as_str().unwrap().contains("host restarted"));
    start_capture(&mut a);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":11000}));
    assert!(requests(&v, "map.markers.create").is_empty());
}

#[test]
fn replay_is_withdrawn_by_disconnect_and_capture_stop() {
    let mut a = capturing();
    let create = create_request(&mut a);
    fail(&mut a, &create, "HostError");
    call(&mut a, json!({"cmd":"disconnected"}));
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"synthetic-connection","writeCapacity":509}),
    );
    colocate(&mut a);
    start_capture(&mut a);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":11000}));
    assert!(requests(&v, "map.markers.create").is_empty());
    assert_eq!(host_state(&mut a, 0)["resolution"], "final");
    // Stopping capture withdraws a replay as well.
    let create = create_request(&mut a);
    fail(&mut a, &create, "HostError");
    assert_eq!(host_state(&mut a, 1)["resolution"], "replayScheduled");
    call(&mut a, json!({"cmd":"stopCapture"}));
    start_capture(&mut a);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":11000}));
    assert!(requests(&v, "map.markers.create").is_empty());
    assert_eq!(host_state(&mut a, 1)["resolution"], "final");
}

#[test]
fn stopping_capture_drops_unanswered_snapshots_and_restart_publishes_nothing_old() {
    let mut a = capturing();
    let v = observe(&mut a);
    let snap = request(&v, "map.observer.snapshot");
    let v = call(&mut a, json!({"cmd":"stopCapture"}));
    let cancel = v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["kind"] == "cancelHostRequests")
        .unwrap_or_else(|| panic!("cancelHostRequests in {v}"))
        .clone();
    assert!(
        !cancel["keepRequestIds"]
            .as_array()
            .unwrap()
            .contains(&snap["requestId"])
    );
    let host = host_state(&mut a, 0);
    assert_eq!(host["state"], "retained");
    assert!(host["reason"].as_str().unwrap().contains("capture ended"));
    let v = respond(&mut a, &snap, snapshot(0.0, 10000));
    assert_eq!(v["result"]["ignored"], "unmatched host response");
    // A creation already sent keeps REM's answer after capture stops.
    start_capture(&mut a);
    let create = create_request(&mut a);
    call(&mut a, json!({"cmd":"stopCapture"}));
    let v = respond(
        &mut a,
        &create,
        json!({"marker":object("host-minted",0.0,T1),"created":true}),
    );
    assert_eq!(v["result"]["observations"][1]["host"]["state"], "created");
    let v = start_capture(&mut a);
    assert!(!has_action(&v, "hostRequest"));
    let v = call(&mut a, json!({"cmd":"tick","nowMs":11000}));
    // The tick may ask REM for nearby markers to display; it never publishes
    // the earlier observations.
    assert!(requests(&v, "map.observer.snapshot").is_empty());
    assert!(requests(&v, "map.markers.create").is_empty());
    assert_eq!(host_state(&mut a, 0)["state"], "retained");
}

#[test]
// Convention over configuration: an omitted delay takes the built-in default;
// an explicit zero is still rejected.
fn retry_delay_defaults_when_omitted_and_zero_is_rejected() {
    let mut a = Adapter::new();
    let mut c = config();
    c.as_object_mut().unwrap().remove("createRetryDelayMs");
    let v = call(&mut a, json!({"cmd":"configure","config":c}));
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["config"]["createRetryDelayMs"], 500);
    let mut c = config();
    c["createRetryDelayMs"] = json!(0);
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        false
    );
}

fn connected_with_pool(pool: &[u16]) -> Adapter {
    let mut a = Adapter::new();
    let mut c = config();
    c["slotPool"] = json!(pool);
    assert_eq!(
        call(&mut a, json!({"cmd":"configure","config":c}))["ok"],
        true
    );
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"synthetic-connection","writeCapacity":509}),
    );
    ready(&mut a);
    colocate(&mut a);
    a
}
fn reply(a: &mut Adapter, now: u64) -> Value {
    call(
        a,
        json!({"cmd":"notification","nowMs":now,"bytes":[1,2,3,0,0,0],"observedAtMs":now}),
    )
}
/// Completes the position write found in `v` (transport, then reply).
fn complete(a: &mut Adapter, v: &Value) -> Value {
    let w = write(v);
    let t = transport_ok(a, &w, 10000);
    assert_eq!(t["ok"], true, "{t}");
    reply(a, 10000)
}
fn slot_byte(w: &Value) -> u64 {
    w["bytes"][2].as_u64().unwrap()
}
fn stopped_event(a: &mut Adapter, id: &str, reason: &str) -> Value {
    call(
        a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.stopped","payload":{"sessionId":id,"reason":reason}}}),
    )
}

#[test]
fn four_concurrent_sessions_share_one_serialized_queue() {
    let mut a = connected_with_pool(&[2, 3, 4, 5, 6, 7]);
    let v = started(&mut a, "s1", &[object("p1", 0.001, T1)]);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let first = write(&v);
    assert_eq!(slot_byte(&first), 4);
    for (id, point) in [("s2", "p2"), ("s3", "p3"), ("s4", "p4")] {
        let v = started(&mut a, id, &[object(point, 0.001, T1)]);
        assert!(has_action(&v, "sessionAuthorized"), "{v}");
        assert!(!has_action(&v, "write")); // one frame in flight device-wide
        assert!(!has_action(&v, "cancelWrites"));
    }
    let v = started(&mut a, "s5", &[object("p5", 0.001, T1)]);
    assert!(!has_action(&v, "sessionAuthorized"));
    assert_eq!(
        request(&v, "map.session.stop")["payload"]["sessionId"],
        "s5"
    );
    assert!(
        request(&v, "notifications.raise")["payload"]["body"]
            .as_str()
            .unwrap()
            .contains("session limit")
    );
    let status = call(&mut a, json!({"cmd":"status"}));
    let sessions = status["result"]["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 4);
    assert_eq!(status["result"]["freeSlots"], json!([6, 7]));
    // The reply belongs to s1; only then may the next session write.
    let v = transport_ok(&mut a, &first, 10000);
    assert!(!has_action(&v, "write"));
    let mut seen = vec![slot_byte(&first)];
    let mut v = reply(&mut a, 10000);
    for _ in 0..3 {
        let w = write(&v);
        seen.push(slot_byte(&w));
        transport_ok(&mut a, &w, 10000);
        v = reply(&mut a, 10000);
    }
    assert_eq!(seen, vec![4, 6, 8, 10]);
    assert!(!has_action(&v, "write"));
}

#[test]
fn overlapping_selection_gets_separate_slots_per_session() {
    let mut a = connected_with_pool(&[2, 3, 4, 5]);
    let v = started(
        &mut a,
        "first",
        &[object("x", 0.001, T1), object("y", 0.002, T1)],
    );
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let v = complete(&mut a, &v);
    let v = complete(&mut a, &v);
    assert!(!has_action(&v, "write"));
    let v = started(&mut a, "second", &[object("y", 0.002, T1)]);
    let second = session_in(&v, "second");
    assert_eq!(second["selected"][0]["pleId"], "y");
    assert_eq!(second["selected"][0]["slotId"], 4);
    let first = session_in(&v, "first");
    assert_eq!(first["selected"].as_array().unwrap().len(), 2);
    // The same object is written again for the second session's own slot.
    assert_eq!(slot_byte(&write(&v)), 8);
    assert_eq!(write(&v)["sessionId"], "second");
}

#[test]
fn exhausted_pool_rejects_only_the_new_session_and_released_slots_are_reused() {
    let mut a = connected_with_pool(&[2, 3, 4]);
    let objects: Vec<Value> = (0..3)
        .map(|i| object(&format!("p{i}"), 0.001, T1))
        .collect();
    let v = started(&mut a, "big", &objects);
    assert!(has_action(&v, "sessionAuthorized"));
    respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let v = started(
        &mut a,
        "late",
        &[object("q0", 0.001, T1), object("q1", 0.001, T1)],
    );
    assert!(!has_action(&v, "sessionAuthorized"));
    assert!(!has_action(&v, "sessionEnded"));
    assert!(!has_action(&v, "cancelWrites"));
    assert_eq!(requests(&v, "map.session.stop").len(), 1);
    assert_eq!(requests(&v, "notifications.raise").len(), 1);
    assert_eq!(
        session_in(&v, "big")["selected"].as_array().unwrap().len(),
        3
    );
    assert_eq!(session_in(&v, "late"), Value::Null);
    // A denied notification is status only.
    let v = fail(
        &mut a,
        &request(&v, "notifications.raise"),
        "PermissionDenied",
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["hostReady"], true);
    assert!(!has_action(&v, "cancelWrites"));
    let v = started(&mut a, "late", &[object("q0", 0.001, T1)]);
    assert!(requests(&v, "map.session.stop").is_empty()); // terminal
    let v = stopped_event(&mut a, "big", "operator");
    let released: Vec<u64> = v["result"]["releasedSlots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            assert_eq!(r["deviceDisplayCleared"], false);
            r["slotId"].as_u64().unwrap()
        })
        .collect();
    assert_eq!(released, vec![2, 3, 4]);
    let v = started(
        &mut a,
        "next",
        &[object("q0", 0.001, T1), object("q1", 0.001, T1)],
    );
    assert!(has_action(&v, "sessionAuthorized"));
    let slots: Vec<u64> = session_in(&v, "next")["selected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["slotId"].as_u64().unwrap())
        .collect();
    assert_eq!(slots.len(), 2);
    assert!(slots.iter().all(|s| [2, 3].contains(s)));
}

#[test]
fn reconciliation_discovers_dropped_started_event() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":14999}));
    assert!(requests(&v, "map.session.list").is_empty());
    let v = call(&mut a, json!({"cmd":"tick","nowMs":15000}));
    let list = request(&v, "map.session.list");
    let v = respond_at(
        &mut a,
        &list,
        json!({"items":[session("dropped",&["p"])]}),
        15000,
    );
    assert!(has_action(&v, "sessionAuthorized"));
    assert_eq!(
        request(&v, "map.session.objects.get")["payload"]["sessionId"],
        "dropped"
    );
}

#[test]
fn reordered_and_duplicate_events_never_reauthorize_or_rewrite() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"early","reason":"moved","object":object("p",0.002,T2)}}}),
    );
    assert_eq!(v["result"]["ignored"], "different session");
    stopped_event(&mut a, "early", "operator");
    let v = started(&mut a, "early", &[object("p", 0.001, T1)]);
    assert!(!has_action(&v, "sessionAuthorized"));
    assert!(requests(&v, "map.session.stop").is_empty());
    let v = sync(&mut a);
    let v = complete(&mut a, &v);
    assert!(!has_action(&v, "write"));
    let v = started(
        &mut a,
        "synthetic-session",
        &[object("synthetic-ple", 0.001, T1)],
    );
    assert_eq!(v["ok"], true);
    assert!(!has_action(&v, "sessionAuthorized"));
    assert!(!has_action(&v, "write"));
}

/// Session with every write completed; observer captured at 10000.
fn settled(a: &mut Adapter, objects: &[Value]) {
    let v = started(a, "settled", objects);
    let mut v = respond(
        a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    while has_action(&v, "write") {
        v = complete(a, &v);
    }
}
/// Tick at 15000: fresh observer, then the session list answer.
fn reconcile(a: &mut Adapter, items: &Value) -> Value {
    let v = call(a, json!({"cmd":"tick","nowMs":15000}));
    let list = request(&v, "map.session.list");
    let observer = request(&v, "map.observer.snapshot");
    let o = respond_at(a, &observer, snapshot(0.0, 15000), 15000);
    assert!(!has_action(&o, "write"), "{o}"); // same geometry
    respond_at(a, &list, items.clone(), 15000)
}

#[test]
fn unchanged_reconciliation_produces_no_write() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    settled(&mut a, &[object("p", 0.001, T1)]);
    let v = reconcile(&mut a, &json!({"items":[session("settled",&["p"])]}));
    let get = request(&v, "map.session.objects.get");
    let v = respond_at(
        &mut a,
        &get,
        json!({"session":session("settled",&["p"]),"objects":[object("p",0.001,T1)]}),
        15000,
    );
    assert_eq!(v["ok"], true, "{v}");
    assert!(!has_action(&v, "write"));
    assert_eq!(session_in(&v, "settled")["selected"][0]["pleId"], "p");
    // Fresh snapshot: the next list inside the interval fetches nothing.
    let v = call(&mut a, json!({"cmd":"tick","nowMs":19999}));
    assert!(requests(&v, "map.session.list").is_empty());
}

#[test]
fn objects_get_omission_removes_only_the_missing_object() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    settled(
        &mut a,
        &[object("keep", 0.001, T1), object("gone", 0.002, T1)],
    );
    let v = reconcile(
        &mut a,
        &json!({"items":[session("settled",&["keep","gone"])]}),
    );
    let get = request(&v, "map.session.objects.get");
    let v = respond_at(
        &mut a,
        &get,
        json!({"session":session("settled",&["keep","gone"]),"objects":[object("keep",0.001,T1)]}),
        15000,
    );
    let session = session_in(&v, "settled");
    assert_eq!(session["selected"].as_array().unwrap().len(), 1);
    assert_eq!(session["selected"][0]["pleId"], "keep");
    let released = v["result"]["releasedSlots"].as_array().unwrap();
    assert_eq!(released.last().unwrap()["pleId"], "gone");
    assert!(!has_action(&v, "write"));
}

#[test]
fn session_missing_from_list_ends_without_stop_and_newer_session_survives() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    settled(&mut a, &[object("p", 0.001, T1)]);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":15000}));
    let list = request(&v, "map.session.list");
    // Accepted after the list request was sent: not judged by that answer.
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","nowMs":15001,"envelope":{"protocolVersion":1,"event":"map.session.started",
            "payload":{"sessionId":"newer","pluginId":"org.freetak.atn","objectDestinationHashes":["q"],"startedAtMs":15001,"startedBy":"operator","objects":[object("q",0.001,T1)]}}}),
    );
    assert!(has_action(&v, "sessionAuthorized"), "{v}");
    let v = respond_at(&mut a, &list, json!({"items":[]}), 15001);
    assert_eq!(v["ok"], true, "{v}");
    assert!(requests(&v, "map.session.stop").is_empty());
    assert!(has_action(&v, "sessionEnded"));
    assert_eq!(session_in(&v, "settled"), Value::Null);
    assert_ne!(session_in(&v, "newer"), Value::Null);
    assert!(
        v["result"]["terminalSessions"]
            .as_array()
            .unwrap()
            .contains(&json!("settled"))
    );
}

#[test]
fn echo_for_an_ended_session_frees_the_device_but_advances_nothing() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let old = write(&v);
    let v = started(&mut a, "other", &[object("q", 0.001, T1)]);
    assert!(!has_action(&v, "write"));
    let v = stopped_event(&mut a, "synthetic-session", "operator");
    assert!(has_action(&v, "sessionEnded"));
    assert!(!has_action(&v, "write")); // its frame is still on the radio
    // A forged echo naming the live session cannot complete anything.
    let v = call(
        &mut a,
        json!({"cmd":"transportResult","sessionId":"other","connectionId":"synthetic-connection","operationId":old["operationId"],"frameNumber":1,"completed":true}),
    );
    assert_eq!(v["result"]["ignored"], "stale transport callback");
    assert!(!has_action(&v, "write"));
    let v = transport_ok(&mut a, &old, 10000);
    assert_eq!(
        v["result"]["ignored"],
        "transport callback for ended session"
    );
    let next = write(&v);
    assert_eq!(next["sessionId"], "other");
    let other = session_in(&call(&mut a, json!({"cmd":"status"})), "other");
    assert_eq!(other["active"]["transportComplete"], false);
    // After reconnecting, an echo from the old connection is ignored.
    call(&mut a, json!({"cmd":"disconnected"}));
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"fresh","writeCapacity":509}),
    );
    let v = transport_ok(&mut a, &next, 10000);
    assert_eq!(v["result"]["ignored"], "stale transport callback");
}

/// One settled session plus a second one, with an icon frame on the radio.
fn partial_icon(a: &mut Adapter) -> Value {
    let v = sync(a);
    let v = complete(a, &v);
    assert!(!has_action(&v, "write"));
    let v = started(a, "bystander", &[object("q", 0.001, T1)]);
    let v = complete(a, &v);
    assert!(!has_action(&v, "write"));
    let png = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png").to_vec();
    let v = call(
        a,
        json!({"cmd":"setIcon","sessionId":"synthetic-session","pleId":"synthetic-ple","png":png}),
    );
    assert_eq!(v["result"]["accepted"], true, "{v}");
    let w = write(&v);
    assert_eq!(w["sessionId"], "synthetic-session");
    w
}
fn assert_invalidated(a: &mut Adapter, v: &Value, frame: &Value) {
    assert!(has_action(v, "disconnect"), "{v}");
    assert!(has_action(v, "cancelWrites"));
    let status = call(a, json!({"cmd":"status"}))["result"].clone();
    assert_eq!(status["sessions"], json!([]));
    assert_eq!(status["connected"], false);
    // No further frames, no replay of the icon.
    let v = transport_ok(a, frame, 10000);
    assert_eq!(v["result"]["ignored"], "stale transport callback");
    assert!(!has_action(&v, "write"));
    // No slot is reused before an explicit reconnect and a new Share.
    let v = started(a, "too-early", &[object("r", 0.001, T1)]);
    assert!(!has_action(&v, "sessionAuthorized"));
    call(
        a,
        json!({"cmd":"connected","connectionId":"after","writeCapacity":509}),
    );
    colocate(a);
    let v = started(a, "bystander", &[object("q", 0.001, T1)]);
    assert!(!has_action(&v, "sessionAuthorized")); // terminal
    let v = started(a, "fresh-share", &[object("q", 0.001, T1)]);
    assert!(has_action(&v, "sessionAuthorized"));
}

#[test]
fn partial_icon_then_session_stop_invalidates_every_session() {
    let mut a = connected();
    ready(&mut a);
    let frame = partial_icon(&mut a);
    let v = stopped_event(&mut a, "synthetic-session", "operator");
    let stops: Vec<Value> = requests(&v, "map.session.stop")
        .into_iter()
        .map(|r| r["payload"]["sessionId"].clone())
        .collect();
    assert_eq!(stops, vec![json!("bystander")]);
    assert_invalidated(&mut a, &v, &frame);
}

#[test]
fn partial_icon_then_revocation_invalidates_every_session() {
    let mut a = connected();
    ready(&mut a);
    let frame = partial_icon(&mut a);
    let v = stopped_event(&mut a, "bystander", "capability-revoked");
    assert!(!has_action(&v, "disconnect")); // bystander had no transfer
    // Revocation surfacing as a typed denial on the next host request.
    let v = call(
        &mut a,
        json!({"cmd":"confirmColocation","evidenceId":"operator at binocular"}),
    );
    let v = fail(&mut a, &request(&v, "map.session.list"), "PermissionDenied");
    assert_eq!(v["ok"], false);
    assert_invalidated_after_error(&mut a, &v, &frame);
}
fn assert_invalidated_after_error(a: &mut Adapter, v: &Value, frame: &Value) {
    assert!(has_action(v, "disconnect"), "{v}");
    let status = call(a, json!({"cmd":"status"}))["result"].clone();
    assert_eq!(status["sessions"], json!([]));
    assert_eq!(status["connected"], false);
    let v = transport_ok(a, frame, 10000);
    assert_eq!(v["result"]["ignored"], "stale transport callback");
}

#[test]
fn partial_icon_then_disconnect_needs_new_share_after_reconnect() {
    let mut a = connected();
    ready(&mut a);
    let frame = partial_icon(&mut a);
    let v = call(&mut a, json!({"cmd":"disconnected"}));
    assert_eq!(requests(&v, "map.session.stop").len(), 2);
    let status = call(&mut a, json!({"cmd":"status"}))["result"].clone();
    assert_eq!(status["sessions"], json!([]));
    let v = transport_ok(&mut a, &frame, 10000);
    assert_eq!(v["result"]["ignored"], "stale transport callback");
    let v = started(&mut a, "too-early", &[object("r", 0.001, T1)]);
    assert!(!has_action(&v, "sessionAuthorized"));
}

#[test]
fn partial_icon_timing_out_closes_the_transport() {
    let mut a = connected();
    ready(&mut a);
    let frame = partial_icon(&mut a);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":12999}));
    assert!(!has_action(&v, "disconnect"));
    let v = call(&mut a, json!({"cmd":"tick","nowMs":13000}));
    assert_eq!(v["ok"], true, "{v}");
    assert_invalidated(&mut a, &v, &frame);
}

#[test]
fn connection_requires_the_atn_gatt_identity() {
    let mut a = Adapter::new();
    call(&mut a, json!({"cmd":"configure","config":config()}));
    // Without characteristic identity the platform boundary is refused.
    let v: Value = serde_json::from_str(
        &a.dispatch(
            &json!({"cmd":"connected","nowMs":10000,"connectionId":"c","writeCapacity":509})
                .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(v["ok"], false);
    let v = call(
        &mut a,
        json!({"cmd":"connected","connectionId":"c","writeCapacity":509,"writeCharacteristic":NOTIFY}),
    );
    assert_eq!(v["ok"], false);
    assert_eq!(
        call(&mut a, json!({"cmd":"status"}))["result"]["connected"],
        false
    );
    let v = call(
        &mut a,
        json!({"cmd":"connected","connectionId":"c","writeCapacity":509,"serviceUuid":SERVICE.to_uppercase()}),
    );
    assert_eq!(v["ok"], true, "{v}");
}

#[test]
fn measurements_decode_only_from_the_notification_characteristic() {
    let mut a = capturing();
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,1,0,120,0,0,0,90],"observedAtMs":10000,"characteristic":WRITE}),
    );
    assert_eq!(v["ok"], true);
    assert_eq!(
        v["result"]["ignored"],
        "not the ATN notification characteristic"
    );
    assert!(!has_action(&v, "hostRequest"));
    let status = call(&mut a, json!({"cmd":"status"}));
    assert_eq!(status["result"]["observations"], json!([]));
    let v = observe(&mut a);
    assert_eq!(v["result"]["host"]["state"], "observerRequested");
}

#[test]
fn transport_echo_must_name_the_write_characteristic_and_failures_are_typed() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let w = write(&v);
    let v = call(
        &mut a,
        json!({"cmd":"transportResult","connectionId":w["connectionId"],"sessionId":w["sessionId"],"operationId":w["operationId"],"frameNumber":1,"completed":true,"characteristic":NOTIFY}),
    );
    assert_eq!(v["result"]["ignored"], "stale transport callback");
    let v = call(
        &mut a,
        json!({"cmd":"transportResult","connectionId":w["connectionId"],"sessionId":w["sessionId"],"operationId":w["operationId"],"frameNumber":1,"completed":false,"error":{"code":"gattStatus","gattStatus":133}}),
    );
    assert_eq!(v["ok"], false);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("gattStatus")
    );
    assert!(has_action(&v, "cancelWrites"));
}

#[test]
fn built_in_binox_4k_defaults_need_no_configuration() {
    let mut a = Adapter::new();
    let v = call(&mut a, json!({"cmd":"configure"}));
    assert_eq!(v["ok"], true, "{v}");
    let c = &v["result"]["config"];
    assert_eq!(c["deviceProfile"]["model"], "Binox 4K");
    assert_eq!(c["deviceProfile"]["protocolVerified"], true);
    assert_eq!(c["semantics"]["distanceMetersPerUnit"], 1.0);
    assert_eq!(c["transport"]["scanTimeoutMs"], 5000);
    // With defaults alone a co-located connection can capture straight away.
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"c","writeCapacity":509}),
    );
    ready(&mut a);
    colocate(&mut a);
    start_capture(&mut a);
}

#[test]
fn partial_overrides_merge_onto_defaults_and_unknown_keys_fail() {
    let mut a = Adapter::new();
    let v = call(
        &mut a,
        json!({"cmd":"resolveConfig","config":{"transport":{"scanTimeoutMs":8000},"color":2}}),
    );
    assert_eq!(v["ok"], true, "{v}");
    let c = &v["result"]["config"];
    assert_eq!(c["transport"]["scanTimeoutMs"], 8000);
    assert_eq!(c["transport"]["connectTimeoutMs"], 10000);
    assert_eq!(c["color"], 2);
    assert_eq!(c["deviceProfile"]["model"], "Binox 4K");
    let v = call(
        &mut a,
        json!({"cmd":"configure","config":{"transport":{"scanTimeoutMS":8000}}}),
    );
    assert_eq!(v["ok"], false);
    let v = call(&mut a, json!({"cmd":"configure","config":[1]}));
    assert_eq!(v["ok"], false);
    // The resolve command never applies the configuration.
    assert_eq!(
        call(&mut a, json!({"cmd":"status"}))["result"]["configured"],
        false
    );
}

fn nearby_marker(id: &str, lon: f64, updated_at: &str) -> Value {
    let mut o = object(id, lon, updated_at);
    o["notes"] = Value::Null;
    o["distanceM"] = json!(lon * 111_000.0);
    o
}
/// Connected, co-located and ready; returns the first nearby request.
fn nearby_ready(a: &mut Adapter) -> Value {
    ready(a);
    colocate(a);
    let v = call(a, json!({"cmd":"tick","nowMs":10000}));
    request(&v, "map.markers.nearby")
}

#[test]
fn nearby_rem_markers_are_shown_automatically_nearest_first() {
    let mut a = connected_with_pool(&[2, 3]);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10000}));
    let query = request(&v, "map.markers.nearby");
    assert_eq!(query["payload"]["radiusM"], 10000.0);
    assert_eq!(query["payload"]["limit"], 2);
    let v = respond(
        &mut a,
        &query,
        json!({"available":true,"radiusM":10000.0,"items":[nearby_marker("near",0.001,T1),nearby_marker("far",0.002,T1)]}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert!(has_action(&v, "sessionAuthorized"));
    let shown = v["result"]["sessions"][0].clone();
    assert_eq!(shown["selected"].as_array().unwrap().len(), 2);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let w = write(&v);
    assert!(w["sessionId"].as_str().unwrap().starts_with("rem-nearby-"));
    // No map.session.stop is ever sent for the automatic set.
    assert!(requests(&v, "map.session.stop").is_empty());
}

#[test]
fn unchanged_nearby_markers_write_nothing_and_changes_replace_the_set() {
    let mut a = connected_with_pool(&[2, 3]);
    let query = request(
        &call(&mut a, json!({"cmd":"tick","nowMs":10000})),
        "map.markers.nearby",
    );
    let items = json!({"available":true,"radiusM":10000.0,"items":[nearby_marker("p",0.001,T1)]});
    let v = respond(&mut a, &query, items.clone());
    let mut v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    while has_action(&v, "write") {
        v = complete(&mut a, &v);
    }
    let first_id = session_in(&call(&mut a, json!({"cmd":"status"})), "rem-nearby-1");
    assert_ne!(first_id, Value::Null);
    // Same membership and content: nothing is written.
    let v = call(&mut a, json!({"cmd":"tick","nowMs":15000}));
    let query = request(&v, "map.markers.nearby");
    let o = respond_at(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 15000),
        15000,
    );
    assert!(!has_action(&o, "write"));
    let v = respond_at(&mut a, &query, items, 15000);
    assert!(!has_action(&v, "write"));
    assert!(!has_action(&v, "sessionEnded"));
    // A new marker nearer than the old one replaces the set; old slots are
    // released without a device clear.
    let v = call(&mut a, json!({"cmd":"tick","nowMs":20000}));
    let query = request(&v, "map.markers.nearby");
    respond_at(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 20000),
        20000,
    );
    let v = respond_at(
        &mut a,
        &query,
        json!({"available":true,"radiusM":10000.0,"items":[nearby_marker("new",0.0005,T1),nearby_marker("p",0.001,T1)]}),
        20000,
    );
    assert!(has_action(&v, "sessionEnded"));
    assert_ne!(session_in(&v, "rem-nearby-2"), Value::Null);
    let released = v["result"]["releasedSlots"].as_array().unwrap();
    assert!(released.iter().all(|r| r["deviceDisplayCleared"] == false));
}

#[test]
fn nearby_grant_missing_is_reported_once_and_breaks_nothing() {
    let mut a = connected();
    let query = nearby_ready(&mut a);
    let v = fail(&mut a, &query, "PermissionDenied");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["hostReady"], true);
    let message = v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["kind"] == "status")
        .unwrap()["message"]
        .clone();
    assert!(
        message
            .as_str()
            .unwrap()
            .contains("Read all map markers near this device")
    );
    let v = call(&mut a, json!({"cmd":"tick","nowMs":15000}));
    let v = fail(
        &mut a,
        &request(&v, "map.markers.nearby"),
        "PermissionDenied",
    );
    assert!(!has_action(&v, "status")); // not repeated
    // REM shares keep working without the grant.
    let v = started(&mut a, "shared", &[object("s", 0.001, T1)]);
    assert!(has_action(&v, "sessionAuthorized"));
}

#[test]
fn stop_pauses_nearby_markers_until_reconnect_and_rem_list_never_ends_them() {
    let mut a = connected_with_pool(&[2, 3]);
    let query = request(
        &call(&mut a, json!({"cmd":"tick","nowMs":10000})),
        "map.markers.nearby",
    );
    let v = respond(
        &mut a,
        &query,
        json!({"available":true,"radiusM":10000.0,"items":[nearby_marker("p",0.001,T1)]}),
    );
    assert!(has_action(&v, "sessionAuthorized"));
    // REM's session list does not know the automatic set and must not end it.
    let v = call(&mut a, json!({"cmd":"tick","nowMs":15000}));
    let v = respond_at(
        &mut a,
        &request(&v, "map.session.list"),
        json!({"items":[]}),
        15000,
    );
    assert_ne!(session_in(&v, "rem-nearby-1"), Value::Null);
    let v = call(&mut a, json!({"cmd":"stopSync","nowMs":15000}));
    assert_eq!(v["result"]["sessions"], json!([]));
    let v = call(&mut a, json!({"cmd":"tick","nowMs":30000}));
    assert!(requests(&v, "map.markers.nearby").is_empty());
    call(
        &mut a,
        json!({"cmd":"connected","nowMs":30000,"connectionId":"again","writeCapacity":509}),
    );
    colocate(&mut a);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":30000}));
    request(&v, "map.markers.nearby");
}

#[test]
fn an_unusable_rem_position_is_reported_once_and_not_requested_in_a_loop() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = started(&mut a, "shared", &[object("p", 0.001, T1)]);
    // REM's fix is far older than the synthetic 1 s limit.
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 1000),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert!(requests(&v, "map.observer.snapshot").is_empty(), "{v}");
    let problem = v["result"]["observerProblem"].as_str().unwrap().to_owned();
    assert!(problem.contains("9 s old"), "{problem}");
    // Anything that pumps writes neither re-asks nor repeats the message.
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"shared","reason":"moved","object":object("p",0.002,T2)}}}),
    );
    assert!(requests(&v, "map.observer.snapshot").is_empty());
    assert!(!has_action(&v, "status"));
    // The next interval asks again.
    let v = call(&mut a, json!({"cmd":"tick","nowMs":15000}));
    request(&v, "map.observer.snapshot");
}

fn phone_fix(a: &mut Adapter, lon: f64, fix_time_ms: u64, now: u64) -> Value {
    call(
        a,
        json!({"cmd":"phoneFix","nowMs":now,"lat":0.0,"lon":lon,"accuracyM":3.0,"fixTimeMs":fix_time_ms}),
    )
}

#[test]
fn phone_gps_places_measurements_without_asking_rem() {
    let mut a = capturing();
    let v = phone_fix(&mut a, 0.0, 10000, 10000);
    assert_eq!(v["result"]["accepted"], true, "{v}");
    let v = observe(&mut a);
    assert!(requests(&v, "map.observer.snapshot").is_empty());
    let create = request(&v, "map.markers.create");
    assert!(create["payload"]["lon"].as_f64().unwrap() > 0.0);
    assert_eq!(v["result"]["host"]["state"], "createRequested");
}

#[test]
fn phone_gps_aims_shared_markers_and_ignores_jitter() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    phone_fix(&mut a, 0.0, 10000, 10000);
    let v = started(&mut a, "shared", &[object("p", 0.001, T1)]);
    let mut v = v;
    while has_action(&v, "write") {
        v = complete(&mut a, &v);
    }
    // Under 10 m of movement: nothing is rewritten.
    let v = phone_fix(&mut a, 0.000_05, 10500, 10500);
    assert_eq!(v["result"]["accepted"], true);
    assert!(!has_action(&v, "write"), "{v}");
    // About 55 m: the marker is re-aimed.
    let v = phone_fix(&mut a, 0.000_5, 10600, 10600);
    assert!(has_action(&v, "write"), "{v}");
}

#[test]
fn a_stale_phone_fix_is_rejected_with_a_plain_reason() {
    let mut a = capturing();
    let v = phone_fix(&mut a, 0.0, 1000, 10000);
    assert_eq!(v["result"]["accepted"], false);
    let status = call(&mut a, json!({"cmd":"status"}));
    assert!(
        status["result"]["observerProblem"]
            .as_str()
            .unwrap()
            .starts_with("The phone GPS")
    );
    // The measurement falls back to asking REM.
    let v = observe(&mut a);
    request(&v, "map.observer.snapshot");
}

fn icon_config() -> Value {
    let mut c = config();
    c["sendIcons"] = json!(true);
    c["queueCapacity"] = json!(16);
    c
}
fn color_of(w: &Value) -> u64 {
    w["bytes"][3].as_u64().unwrap() & 7
}

#[test]
fn markers_get_nato_colours_and_one_icon_each() {
    let mut a = Adapter::new();
    call(&mut a, json!({"cmd":"configure","config":icon_config()}));
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"synthetic-connection","writeCapacity":509}),
    );
    ready(&mut a);
    colocate(&mut a);
    let mut hostile = object("h", 0.001, T1);
    hostile["symbol"] = json!("hostile");
    let mut unknown = object("u", 0.002, T1);
    unknown["symbol"] = json!("unknown");
    let v = started(&mut a, "shared", &[hostile, unknown]);
    let mut v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let mut writes = Vec::new();
    while has_action(&v, "write") {
        let w = write(&v);
        writes.push(w.clone());
        let t = transport_ok(&mut a, &w, 10000);
        v = if w["totalFrames"] == w["frameNumber"] {
            let command = w["bytes"][1].as_u64().unwrap();
            call(
                &mut a,
                json!({"cmd":"notification","bytes":[1,command,3,0,0,0],"observedAtMs":10000}),
            )
        } else {
            t
        };
    }
    let positions: Vec<&Value> = writes.iter().filter(|w| w["bytes"][1] == 2).collect();
    let icons: Vec<&Value> = writes.iter().filter(|w| w["bytes"][1] == 7).collect();
    assert_eq!(positions.len(), 2);
    assert_eq!(color_of(positions[0]), 0); // hostile red
    assert_eq!(color_of(positions[1]), 2); // unknown yellow
    assert_eq!(icons.len(), 2, "one single-frame icon per marker");
    // Same symbol again: no new icon. Changed symbol: the icon is resent.
    let mut moved = object("u", 0.003, T2);
    moved["symbol"] = json!("unknown");
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"shared","reason":"moved","object":moved}}}),
    );
    assert_eq!(write(&v)["bytes"][1], 2);
    assert_eq!(
        session_in(&call(&mut a, json!({"cmd":"status"})), "shared")["queuedOperations"],
        0
    );
    let mut renamed = object("u", 0.003, "2026-10-16T12:36:56Z");
    renamed["symbol"] = json!("friendly");
    call(
        &mut a,
        json!({"cmd":"transportResult","connectionId":"synthetic-connection","sessionId":"shared","operationId":write(&v)["operationId"],"frameNumber":1,"completed":true}),
    );
    call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,2,3,0,0,0],"observedAtMs":10000}),
    );
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.updated","payload":{"sessionId":"shared","reason":"symbol","object":renamed}}}),
    );
    assert_eq!(color_of(&write(&v)), 4); // friendly blue
    assert_eq!(session_in(&v, "shared")["queuedOperations"], 1); // its new icon
}

#[test]
fn generic_symbols_keep_the_configured_blue() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let mut vehicle = object("v", 0.001, T1);
    vehicle["symbol"] = json!("vehicle");
    let v = started(&mut a, "shared", &[vehicle]);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    assert_eq!(color_of(&write(&v)), 4);
}
