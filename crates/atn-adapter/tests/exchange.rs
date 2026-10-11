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
fn request(v: &Value, op: &str) -> Value {
    v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["envelope"]["operation"] == op)
        .expect("host request")["envelope"]
        .clone()
}
#[allow(clippy::needless_pass_by_value)] // Fixture builders hand over complete results.
fn respond(a: &mut Adapter, request: &Value, result: Value) -> Value {
    call(
        a,
        json!({"cmd":"hostResponse","envelope":{"protocolVersion":1,"requestId":request["requestId"],"ok":true,"result":result}}),
    )
}
fn observer() -> Value {
    json!({"position":{"latitude":0.0,"longitude":0.0,"altitudeM":null},"capturedAtMs":10000,"horizontalAccuracyM":1.0,"source":"verifiedColocated","evidenceId":"synthetic-colocation"})
}
fn marker(id: &str, lon: f64) -> Value {
    json!({"local_id":"synthetic-local","object_destination_hash":id,"origin_rch":"synthetic-origin","marker_type":"marker","symbol":"marker","name":"Synthetic point","category":"marker","lat":0.0,"lon":lon,"notes":null,"created_ts_ms":9000,"updated_ts_ms":10000})
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
    let start = call(a, json!({"cmd":"hostStarted"}));
    let r = request(&start, "map.adapter.describe");
    let v = respond(
        a,
        &r,
        json!({"contractVersion":1,"operations":["map.adapter.describe","map.observer.get","map.ple.get","map.ple.subscribe","map.observation.draft.put","map.ple.unsubscribe"]}),
    );
    assert_eq!(v["result"]["hostReady"], true);
}
fn sync(a: &mut Adapter) -> Value {
    let start = call(
        a,
        json!({"cmd":"startSync","sessionId":"synthetic-session","selected":[{"pleId":"synthetic-ple","slotId":2}]}),
    );
    assert_eq!(start["ok"], true, "{start}");
    let v = respond(
        a,
        &request(&start, "map.ple.subscribe"),
        json!({"accepted":true}),
    );
    assert_eq!(v["ok"], true);
    let v = respond(
        a,
        &request(&start, "map.observer.get"),
        json!({"observer":observer()}),
    );
    assert_eq!(v["ok"], true);
    respond(
        a,
        &request(&start, "map.ple.get"),
        json!({"marker":marker("synthetic-ple",0.001)}),
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
    assert_eq!(v["actions"], json!([]));
}
#[test]
fn real_rem_rejection_cannot_authorize_write() {
    let mut a = connected();
    let start = call(&mut a, json!({"cmd":"hostStarted"}));
    let r = request(&start, "map.adapter.describe");
    let v = call(
        &mut a,
        json!({"cmd":"hostResponse","envelope":{"protocolVersion":1,"requestId":r["requestId"],"ok":false,"error":{"code":"unsupported_operation"}}}),
    );
    assert_eq!(v["ok"], false);
    assert_eq!(
        call(
            &mut a,
            json!({"cmd":"startSync","sessionId":"s","selected":[{"pleId":"p","slotId":2}]})
        )["ok"],
        false
    );
}
#[test]
fn observation_to_host_draft_preserves_provenance() {
    let mut a = connected();
    ready(&mut a);
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,1,0,120,0,0,0,90],"observedAtMs":10000}),
    );
    let v = respond(
        &mut a,
        &request(&v, "map.observer.get"),
        json!({"observer":observer()}),
    );
    let r = request(&v, "map.observation.draft.put");
    assert_eq!(r["payload"]["draft"]["state"], "draft");
    assert_eq!(
        r["payload"]["draft"]["observation"]["rawBytes"],
        json!([1, 1, 0, 120, 0, 0, 0, 90])
    );
    assert!(
        r["payload"]["draft"]["position"]["longitude"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert_eq!(
        r["payload"]["draft"]["uncertainty"]["derivedAccuracyM"],
        Value::Null
    );
}
#[test]
fn stale_observer_retains_raw_without_map_write() {
    let mut a = connected();
    ready(&mut a);
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,1,0,120,0,0,0,90],"observedAtMs":10000}),
    );
    let mut o = observer();
    o["capturedAtMs"] = json!(0);
    let v = respond(
        &mut a,
        &request(&v, "map.observer.get"),
        json!({"observer":o}),
    );
    assert_eq!(v["ok"], true);
    assert!(
        !v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "hostRequest")
    );
    assert_eq!(v["result"]["observations"].as_array().unwrap().len(), 1);
}
#[test]
fn rch_selected_update_flows_once_without_expanding_set() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let write = &v["actions"][0];
    assert_eq!(write["kind"], "write");
    assert_eq!(write["bytes"][2], 4); // owned slot2, type0
    let id = write["operationId"].clone();
    let v = call(
        &mut a,
        json!({"cmd":"transportResult","sessionId":"synthetic-session","connectionId":"synthetic-connection","operationId":id,"frameNumber":1,"completed":true}),
    );
    assert_eq!(v["result"]["transportComplete"], true);
    assert_eq!(v["result"]["reply"], Value::Null);
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,2,3,0,0,0],"observedAtMs":10000}),
    );
    assert_eq!(v["result"]["report"]["displayConfirmation"], Value::Null);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.ple.updated","payload":{"sessionId":"synthetic-session","marker":marker("unselected",0.002)}}}),
    );
    assert_eq!(v["actions"], json!([]));
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.ple.updated","payload":{"sessionId":"synthetic-session","marker":marker("synthetic-ple",0.002)}}}),
    );
    assert_eq!(v["actions"][0]["kind"], "write");
}
#[test]
fn disconnect_and_old_callbacks_never_replay() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let old = v["actions"][0]["operationId"].clone();
    call(&mut a, json!({"cmd":"disconnected"}));
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
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.ple.updated","payload":{"sessionId":"synthetic-session","marker":marker("synthetic-ple",0.003)}}}),
    );
    assert_eq!(v["actions"], json!([]));
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
}
#[test]
fn pending_host_deadline_cancels_session() {
    let mut a = connected();
    let v = call(&mut a, json!({"cmd":"hostStarted"}));
    assert_eq!(v["ok"], true);
    let v = call(&mut a, json!({"cmd":"tick","nowMs":12000}));
    assert_eq!(v["ok"], false);
    assert_eq!(v["actions"][0]["kind"], "cancelWrites");
}

#[test]
fn early_reply_and_platform_completion_remain_separate() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let write = v["actions"][0].clone();
    let v = call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,2,3,0,0,0],"observedAtMs":10000}),
    );
    assert_eq!(v["result"]["report"]["transportComplete"], false);
    assert_eq!(v["result"]["report"]["reply"]["answer_signed"], 3);
    let v = call(
        &mut a,
        json!({"cmd":"transportResult","connectionId":write["connectionId"],"sessionId":write["sessionId"],"operationId":write["operationId"],"frameNumber":1,"completed":true}),
    );
    assert_eq!(v["result"]["transportComplete"], true);
    assert_eq!(v["result"]["displayConfirmation"], Value::Null);
}

#[test]
fn observer_is_revalidated_at_actual_send_after_queueing() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let write = v["actions"][0].clone();
    let v = call(
        &mut a,
        json!({"cmd":"transportResult","connectionId":write["connectionId"],"sessionId":write["sessionId"],"operationId":write["operationId"],"frameNumber":1,"completed":true,"nowMs":11001}),
    );
    assert_eq!(v["ok"], false);
    assert_eq!(v["actions"][0]["kind"], "cancelWrites");
}

#[test]
fn stale_session_observer_event_is_ignored_and_conflicting_fix_rejected() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let mut changed = observer();
    changed["position"]["longitude"] = json!(0.5);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.observer.updated","payload":{"sessionId":"old-session","observer":changed}}}),
    );
    assert_eq!(v["actions"], json!([]));
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.observer.updated","payload":{"sessionId":"synthetic-session","observer":changed}}}),
    );
    assert_eq!(v["ok"], false);
}

#[test]
fn style_changes_queue_position_and_icon_is_selected_only() {
    let mut a = connected();
    ready(&mut a);
    let v = sync(&mut a);
    let write = v["actions"][0].clone();
    call(
        &mut a,
        json!({"cmd":"transportResult","connectionId":write["connectionId"],"sessionId":write["sessionId"],"operationId":write["operationId"],"frameNumber":1,"completed":true}),
    );
    call(
        &mut a,
        json!({"cmd":"notification","bytes":[1,2,3,0,0,0],"observedAtMs":10000}),
    );
    let v = call(&mut a, json!({"cmd":"setStyle","markerType":1,"color":3}));
    assert_eq!(v["actions"][0]["bytes"][2], 5);
    assert_eq!(v["actions"][0]["bytes"][3], 19);
    let png = include_bytes!("../../../atn-rangefinder-docs/fixtures/synthetic-icon.png").to_vec();
    let v = call(
        &mut a,
        json!({"cmd":"setIcon","pleId":"synthetic-ple","png":png}),
    );
    assert_eq!(v["ok"], true);
    assert_eq!(v["actions"], json!([])); // waits for position reply, separate operation
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
    let start = call(
        &mut a,
        json!({"cmd":"startSync","sessionId":"two-selected","selected":[{"pleId":"a","slotId":2},{"pleId":"b","slotId":3}]}),
    );
    respond(
        &mut a,
        &request(&start, "map.ple.subscribe"),
        json!({"accepted":true}),
    );
    respond(
        &mut a,
        &request(&start, "map.observer.get"),
        json!({"observer":observer()}),
    );
    let requests = start["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["envelope"]["operation"] == "map.ple.get")
        .map(|a| a["envelope"].clone())
        .collect::<Vec<_>>();
    let v = respond(&mut a, &requests[0], json!({"marker":marker("a",0.001)}));
    assert_eq!(v["actions"][0]["kind"], "write");
    respond(&mut a, &requests[1], json!({"marker":marker("b",0.002)}));
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","envelope":{"protocolVersion":1,"event":"map.ple.removed","payload":{"sessionId":"two-selected","pleId":"b"}}}),
    );
    assert_eq!(v["actions"], json!([]));
    assert_eq!(v["result"]["session"]["active"]["pleId"], "a");
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
fn user_stop_releases_subscription_without_device_clear() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let v = call(&mut a, json!({"cmd":"stopSync"}));
    let r = request(&v, "map.ple.unsubscribe");
    assert_eq!(r["payload"]["sessionId"], "synthetic-session");
    assert!(
        !v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "write")
    );
}

#[test]
fn connecting_during_host_negotiation_preserves_host_lifetime_request() {
    let mut a = Adapter::new();
    call(&mut a, json!({"cmd":"configure","config":config()}));
    let v = call(&mut a, json!({"cmd":"hostStarted"}));
    let r = request(&v, "map.adapter.describe");
    call(
        &mut a,
        json!({"cmd":"connected","connectionId":"fresh","writeCapacity":509}),
    );
    let v = respond(
        &mut a,
        &r,
        json!({"contractVersion":1,"operations":["map.adapter.describe","map.observer.get","map.ple.get","map.ple.subscribe","map.observation.draft.put","map.ple.unsubscribe"]}),
    );
    assert_eq!(v["result"]["hostReady"], true);
}

#[test]
fn stopped_session_timer_remains_read_only_without_repeated_errors() {
    let mut a = connected();
    ready(&mut a);
    sync(&mut a);
    let stop = call(&mut a, json!({"cmd":"stopSync"}));
    respond(
        &mut a,
        &request(&stop, "map.ple.unsubscribe"),
        json!({"accepted":true}),
    );
    let v = call(&mut a, json!({"cmd":"tick","nowMs":13000}));
    assert_eq!(v["ok"], true);
    assert_eq!(v["actions"], json!([]));
}

#[test]
fn moving_observer_replaces_unsent_geometry_before_next_transfer() {
    let mut a = connected();
    ready(&mut a);
    let first = sync(&mut a)["actions"][0].clone();
    call(
        &mut a,
        json!({"cmd":"hostEvent","nowMs":10500,"envelope":{"protocolVersion":1,"event":"map.ple.updated","payload":{"sessionId":"synthetic-session","marker":marker("synthetic-ple",0.002)}}}),
    );
    let mut changed = observer();
    changed["capturedAtMs"] = json!(10500);
    changed["position"]["longitude"] = json!(0.0005);
    let v = call(
        &mut a,
        json!({"cmd":"hostEvent","nowMs":10500,"envelope":{"protocolVersion":1,"event":"map.observer.updated","payload":{"sessionId":"synthetic-session","observer":changed}}}),
    );
    assert_eq!(v["result"]["session"]["queuedOperations"], 1);
    call(
        &mut a,
        json!({"cmd":"transportResult","nowMs":10500,"connectionId":first["connectionId"],"sessionId":first["sessionId"],"operationId":first["operationId"],"frameNumber":1,"completed":true}),
    );
    let v = call(
        &mut a,
        json!({"cmd":"notification","nowMs":10500,"observedAtMs":10500,"bytes":[1,2,3,0,0,0]}),
    );
    let bytes = &v["actions"][0]["bytes"];
    let distance = i16::from_le_bytes([
        u8::try_from(bytes[4].as_u64().unwrap()).unwrap(),
        u8::try_from(bytes[5].as_u64().unwrap()).unwrap(),
    ]);
    assert_eq!(distance, 167); // fresh observer at 0.0005deg -> selected point 0.002deg
}
