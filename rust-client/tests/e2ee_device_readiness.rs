#![cfg(feature = "messaging")]

use serde_json::{json, Value};
use zalo_rust::protocol::signal::{self, DevicePlatform, Scope};

fn response(request: &signal::SignalRequest, compatible: Value) -> Value {
    json!({"error_code":0,"data":{"reqId":request.request_id(),"data":{
        "err":0,"compatible":compatible,"incompatible":[]
    }}})
}

#[test]
fn empty_or_unknown_compatibility_never_invents_device_coverage() {
    let scope = Scope::new("11", 7).unwrap();
    let request = signal::compatible(&scope, 42, "22").unwrap();
    let empty = response(&request, json!([]));
    assert!(signal::compatible_reply(&request, &scope, &empty, "22")
        .unwrap()
        .is_empty());
    for value in [Value::Null, json!({}), json!(false), json!("all")] {
        assert!(
            signal::compatible_reply(&request, &scope, &response(&request, value), "22").is_err()
        );
    }
    let mut missing = empty.clone();
    missing["data"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("compatible");
    assert!(signal::compatible_reply(&request, &scope, &missing, "22").is_err());
}

#[test]
fn compatibility_is_bound_to_account_generation_request_and_target() {
    let scope = Scope::new("11", 7).unwrap();
    let request = signal::compatible(&scope, 42, "22").unwrap();
    let good = response(
        &request,
        json!([{"uid":"22","deviceId":2},{"uid":"22","deviceId":0}]),
    );
    assert_eq!(
        signal::compatible_reply(&request, &scope, &good, "22").unwrap(),
        vec![DevicePlatform::Mobile, DevicePlatform::Pc]
    );
    for wrong in [Scope::new("11", 8).unwrap(), Scope::new("33", 7).unwrap()] {
        assert!(signal::compatible_reply(&request, &wrong, &good, "22").is_err());
    }
    assert!(signal::compatible_reply(&request, &scope, &good, "33").is_err());
    let mut mismatch = good.clone();
    mismatch["data"]["reqId"] = json!("other-request");
    assert!(signal::compatible_reply(&request, &scope, &mismatch, "22").is_err());
    for entries in [
        json!([{"uid":"33","deviceId":0}]),
        json!([{"uid":"22","deviceId":0},{"uid":"22","deviceId":0}]),
        json!([{"uid":"22","deviceId":3}]),
        json!([{"uid":"22","deviceId":"0"}]),
    ] {
        assert!(
            signal::compatible_reply(&request, &scope, &response(&request, entries), "22").is_err()
        );
    }
    let mut refused = good;
    refused["data"]["data"]["err"] = json!(-11);
    assert!(signal::compatible_reply(&request, &scope, &refused, "22").is_err());
}

fn metadata_response(request: &signal::SignalRequest, devices: Value) -> Value {
    json!({"error_code":0,"data":{"reqId":request.request_id(),"data":{
        "err":0,"threads":[{"uid":"22","devices":devices}],
        "uids":[],"waitUids":[],"pendingUids":[]
    }}})
}

#[test]
fn target_metadata_preserves_unknown_iid_without_inventing_registration_or_completeness() {
    let scope = Scope::new("11", 7).unwrap();
    let request = signal::metadata(&scope, 51, None).unwrap();
    let value = metadata_response(
        &request,
        json!([{"di":0},{"di":1,"iid":0},{"di":2,"iid":123}]),
    );
    let page = signal::target_device_page(&request, &scope, &value, "22").unwrap();
    let devices = page.devices.unwrap();
    assert_eq!(devices[0].iid, None);
    assert_eq!(devices[1].iid, None);
    assert_eq!(devices[2].iid, Some(123));
    assert!(!page.uncertain && page.next.is_none());
    for field in ["threads", "uids", "waitUids", "pendingUids"] {
        let mut incomplete = value.clone();
        incomplete["data"]["data"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(signal::target_device_page(&request, &scope, &incomplete, "22").is_err());
    }
    for flag in [Value::Null, json!(false), json!(0)] {
        let mut terminal = value.clone();
        terminal["data"]["data"]["hasMore"] = flag;
        assert!(
            signal::target_device_page(&request, &scope, &terminal, "22")
                .unwrap()
                .next
                .is_none()
        );
    }
    let mut continuation = value.clone();
    continuation["data"]["data"]["hasMore"] = json!(1);
    assert!(signal::target_device_page(&request, &scope, &continuation, "22").is_err());
    continuation["data"]["data"]["lastId"] = json!(90);
    let first = signal::target_device_page(&request, &scope, &continuation, "22").unwrap();
    let next = signal::metadata(&scope, 52, first.next.as_ref()).unwrap();
    continuation["data"]["reqId"] = json!(next.request_id());
    continuation["data"]["data"]["lastId"] = json!(40);
    assert!(
        signal::target_device_page(&next, &scope, &continuation, "22")
            .unwrap()
            .next
            .is_some(),
        "A page reader must not invent ascending server cursor order"
    );
}

#[test]
fn pending_waiting_or_conflicting_target_metadata_never_becomes_complete_coverage() {
    let scope = Scope::new("11", 7).unwrap();
    let request = signal::metadata(&scope, 51, None).unwrap();
    let good = metadata_response(&request, json!([{"di":0,"iid":123}]));
    for (field, entries) in [
        ("waitUids", json!(["22"])),
        ("pendingUids", json!([{"uid":"22","deviceId":0}])),
    ] {
        let mut pending = good.clone();
        pending["data"]["data"][field] = entries;
        assert!(
            signal::target_device_page(&request, &scope, &pending, "22")
                .unwrap()
                .uncertain
        );
    }
    for devices in [
        json!([{"di":0,"iid":null}]),
        json!([{"di":0,"iid":-1}]),
        json!([{"di":0,"iid":16384}]),
        json!([{"di":0,"iid":123},{"di":0,"iid":123}]),
        json!([{"di":3,"iid":123}]),
    ] {
        assert!(signal::target_device_page(
            &request,
            &scope,
            &metadata_response(&request, devices),
            "22"
        )
        .is_err());
    }
    let mut duplicate = good.clone();
    duplicate["data"]["data"]["threads"]
        .as_array_mut()
        .unwrap()
        .push(json!({"uid":"22","devices":[]}));
    assert!(signal::target_device_page(&request, &scope, &duplicate, "22").is_err());
    let mut absent = good.clone();
    absent["data"]["data"]["threads"] = json!([]);
    assert!(signal::target_device_page(&request, &scope, &absent, "22")
        .unwrap()
        .devices
        .is_none());
}
