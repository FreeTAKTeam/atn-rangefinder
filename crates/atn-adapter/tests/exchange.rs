//! Synthetic exchange against the REM plugin map API 1.3 envelopes inspected at
//! REM `91124cffd8f7fa7df94684b672c2228d97f0522d`. These tests prove adapter
//! behaviour, not real REM map visibility or device rendering.
use atn_adapter::Adapter;
use serde_json::{Value, json};

fn call(a: &mut Adapter, mut v: Value) -> Value {
    if v.get("nowMs").is_none() {
        v["nowMs"] = json!(10000);
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
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
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
    assert!(key.starts_with("synthetic-connection/"));
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
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
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
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
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
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let host = &v["result"]["observations"][0]["host"];
    assert_eq!(host["state"], "retained");
    assert!(host["reason"].as_str().unwrap().contains("co-location"));
    let v = started(&mut a, "shared-early", &[object("p", 0.001, T1)]);
    assert_eq!(v["ok"], true);
    assert!(!has_action(&v, "sessionAuthorized"));
    let stop = request(&v, "map.session.stop");
    assert_eq!(stop["payload"]["sessionId"], "shared-early");
    assert_eq!(v["result"]["session"], Value::Null);
    // The rejected ID is terminal even if REM re-reports it.
    let v = started(&mut a, "shared-early", &[object("p", 0.001, T1)]);
    assert!(requests(&v, "map.session.stop").is_empty());
    assert!(!has_action(&v, "sessionAuthorized"));
}

#[test]
fn create_rejection_timeout_and_binder_failure_stay_distinct() {
    let mut a = connected();
    ready(&mut a);
    colocate(&mut a);
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let v = fail(&mut a, &request(&v, "map.markers.create"), "InvalidRequest");
    assert_eq!(v["ok"], true);
    let host = &v["result"]["observations"][0]["host"];
    assert_eq!(host["state"], "rejected");
    assert_eq!(host["code"], "InvalidRequest");
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
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
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let v = fail(&mut a, &request(&v, "map.markers.create"), "binder.timeout");
    assert_eq!(v["result"]["observations"][2]["host"]["state"], "unknown");
    let v = observe(&mut a);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.snapshot"),
        snapshot(0.0, 10000),
    );
    let v = fail(
        &mut a,
        &request(&v, "map.markers.create"),
        "PermissionDenied",
    );
    assert_eq!(v["ok"], false);
    assert_eq!(
        call(&mut a, json!({"cmd":"status"}))["result"]["hostReady"],
        false
    );
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
    let selected = v["result"]["session"]["selected"]
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
fn second_session_is_rejected_once_and_existing_session_is_untouched() {
    let mut a = connected();
    ready(&mut a);
    let first = sync(&mut a);
    let w = write(&first);
    let v = started(&mut a, "second", &[object("other", 0.001, T1)]);
    assert_eq!(requests(&v, "map.session.stop").len(), 1);
    assert_eq!(v["result"]["session"]["sessionId"], "synthetic-session");
    assert!(!has_action(&v, "cancelWrites"));
    let v = started(&mut a, "second", &[object("other", 0.001, T1)]);
    assert!(requests(&v, "map.session.stop").is_empty());
    let v = transport_ok(&mut a, &w, 10000);
    assert_eq!(v["result"]["transportComplete"], true);
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
    assert_eq!(v["result"]["session"], Value::Null);
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
    assert_eq!(v["result"]["session"]["state"]["state"], "stopped");
    assert!(has_action(&v, "cancelWrites"));
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
    assert!(has_action(&v, "cancelWrites"));
    assert!(!has_action(&v, "write"));
    assert!(requests(&v, "map.session.stop").is_empty());
    let state = &v["result"]["session"]["state"];
    assert_eq!(state["state"], "stopped");
    assert!(
        state["reason"]
            .as_str()
            .unwrap()
            .contains("capability-revoked")
    );
    assert!(v["result"]["session"]["completed"][0]["cancelled"].is_string());
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
    let selected = v["result"]["session"]["selected"].as_array().unwrap();
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
    assert_eq!(v["result"]["session"]["state"]["state"], "stopped");
    assert!(has_action(&v, "cancelWrites"));
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
    assert_eq!(v["result"]["transportComplete"], true);
    assert_eq!(v["result"]["displayConfirmation"], Value::Null);
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
        json!({"cmd":"setIcon","pleId":"synthetic-ple","png":png}),
    );
    assert_eq!(v["ok"], true);
    assert!(!has_action(&v, "write")); // waits for position reply, separate operation
    let v = call(
        &mut a,
        json!({"cmd":"setIcon","pleId":"not-selected","png":png}),
    );
    assert_eq!(v["ok"], false);
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
    assert_eq!(v["result"]["session"]["queuedOperations"], 1);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.session.object.removed","payload":{"sessionId":"two-selected","objectDestinationHash":"b"}}}),
    );
    assert!(!has_action(&v, "write"));
    assert!(!has_action(&v, "cancelWrites"));
    assert_eq!(v["result"]["session"]["active"]["pleId"], "a");
    assert_eq!(v["result"]["session"]["queuedOperations"], 0);
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
    assert_eq!(v["result"]["session"]["queuedOperations"], 1);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":10500}));
    let refresh = request(&v, "map.observer.snapshot");
    let v = respond_at(&mut a, &refresh, snapshot(0.0005, 10500), 10500);
    assert_eq!(v["result"]["session"]["queuedOperations"], 1);
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
