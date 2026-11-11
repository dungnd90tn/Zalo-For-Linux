#![cfg(feature = "messaging")]

use serde_json::{json, Value};
use zalo_rust::protocol::messaging::e2ee_send_ack_for;

#[test]
fn original_response_metadata_normalizes_without_accepting_conflicting_ids() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/messaging/send_ack.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let response = json!({"error_code":0, "data":case["input"]});
        let result = e2ee_send_ack_for(&response, 101);
        match name {
            "encrypted_string_ids"
            | "encrypted_numeric_ids"
            | "plain_passthrough"
            | "missing_client_passthrough"
            | "no_timestamp"
            | "unknown_fields_retained" => {
                let ack =
                    result.unwrap_or_else(|_| panic!("rejected original response case {name}"));
                assert_eq!(ack.server_id, "201");
            }
            // The original helper is a normalizer only. Native additionally rejects
            // contradictory/invalid identifiers rather than attributing a wrong ACK.
            _ => assert!(result.is_err(), "unsafe metadata accepted: {name}"),
        }
    }
}

#[test]
fn unsuccessful_or_mismatched_responses_never_authorize_a_local_echo() {
    for data in [
        json!({"clientId":"102","minGlobalMsgId":"201","ts":123}),
        json!({"clientId":null,"minGlobalMsgId":"201"}),
        json!({"clientId":"101","minGlobalMsgId":"201","ts":-1}),
        json!({"clientId":"101","minGlobalMsgId":"201","ts":{}}),
    ] {
        assert!(e2ee_send_ack_for(&json!({"error_code":0,"data":data}), 101).is_err());
    }
    assert!(e2ee_send_ack_for(
        &json!({"error_code":5,"data":{"clientId":"101","minGlobalMsgId":"201"}}),
        101
    )
    .is_err());
    let matching = json!({"error_code":0,"data":{"clientId":101,"minGlobalMsgId":201,"msgId":"201","ts":"123"}});
    let ack = e2ee_send_ack_for(&matching, 101).unwrap();
    assert_eq!(ack.timestamp_ms, Some(123));
}
