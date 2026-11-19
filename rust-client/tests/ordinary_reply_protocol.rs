#![cfg(feature = "messaging")]
//! Compare native ordinary replies with independently executed source builders.
use serde_json::{json, Value};
use zalo_rust::{
    domain::MessageQuote,
    protocol::{
        client::ClientType,
        messaging::{self, Target, ThreadKind},
    },
};

#[test]
fn native_reply_parameters_match_original_direct_and_group_builders() {
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/messaging/reply.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let source = &case["quote"];
        let quote = MessageQuote::from_wire(
            "11",
            &json!({
                "fromUid":source["fromUid"],"msgId":source["msgId"],"cliMsgId":source["cliMsgId"],
                "cliMsgType":source["cliMsgType"],"ts":source["ts"],"msg":source["textParams"][0],
                "ttl":source.get("ttl").cloned().unwrap_or(json!(0))
            }),
        )
        .unwrap();
        let group = case["group"].as_bool().unwrap();
        let target = Target::new(
            if group {
                ThreadKind::Group
            } else {
                ThreadKind::Direct
            },
            if group { "77" } else { "22" },
        )
        .unwrap();
        let native = messaging::quote_parameters(
            &target,
            "Trả lời tổng hợp",
            7001,
            "synthetic-device",
            &quote,
            Some(0),
        );
        if !case["native_supported"].as_bool().unwrap() {
            assert!(
                native.is_err(),
                "Unsupported source must not fabricate metadata: {}",
                case["name"]
            );
            continue;
        }
        let mut expected = case["output"]["payload"].clone();
        // Wire IDs can be numeric in the source; native routing stores canonical decimal strings.
        for field in ["qmsgOwner", "qmsgId", "qmsgCliId"] {
            if let Some(id) = expected[field].as_u64() {
                expected[field] = json!(id.to_string());
            }
        }
        assert_eq!(native.unwrap(), expected, "{}", case["name"]);
        let url = messaging::quote_url(
            &url::Url::parse("https://wpa.zaloapp.com").unwrap(),
            target.kind,
            ClientType::PcLinux24,
        )
        .unwrap();
        assert_eq!(
            url.path(),
            case["output"]["endpoint"]["path"].as_str().unwrap()
        );
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "zpw_type" && value == "24"));
    }
}

fn incoming(quote: Value, extras: Value) -> messaging::Incoming {
    let mut message = json!({"uidFrom":"22","idTo":"11","msgId":"9001","cliMsgId":"8001",
        "ts":"1700000000123","msgType":"webchat","content":"Tin hợp lệ tiếng Việt 😀","quote":quote});
    message
        .as_object_mut()
        .unwrap()
        .extend(extras.as_object().unwrap().clone());
    messaging::incoming_messages("11", &json!({"error_code":0,"data":{"msgs":[message]}}))
        .unwrap()
        .remove(0)
}

#[test]
fn malformed_unknown_or_oversized_quote_never_discards_otherwise_valid_text() {
    let malformed = [
        json!(false),
        json!("opaque unknown"),
        json!([1, 2]),
        json!({"cliMsgType":999,"opaque":{"future":[1,2,3]}}),
        json!({"cliMsgType":1,"msg":"x".repeat(140*1024)}),
    ];
    for quote in malformed {
        let incoming = incoming(quote.clone(), json!({}));
        assert_eq!(incoming.message.text, "Tin hợp lệ tiếng Việt 😀");
        assert!(incoming.message.quote.is_some());
        if serde_json::to_vec(&quote).unwrap().len() <= 128 * 1024 {
            assert_eq!(
                serde_json::from_str::<Value>(incoming.quote_content.as_ref().unwrap().as_json())
                    .unwrap(),
                quote
            );
        } else {
            assert!(incoming.quote_content.is_none());
        }
        assert!(!format!("{:?}", incoming.message.quote).contains("opaque"));
    }
}

#[test]
fn reply_references_require_real_client_ids_and_non_ephemeral_plain_sources() {
    for extras in [
        json!({"cliMsgId":null}),
        json!({"cliMsgId":"invalid"}),
        json!({"cliMsgId":"0"}),
        json!({"ttl":60000}),
        json!({"ttl":"unknown"}),
        json!({"e2eeExt":{"version":0}}),
    ] {
        let received = incoming(Value::Null, extras);
        assert!(
            received.message.reply_reference().is_none(),
            "Unavailable metadata cannot substitute server/action IDs"
        );
    }
    let received = incoming(Value::Null, json!({}));
    assert_eq!(received.message.source_client_id.as_deref(), Some("8001"));
    assert!(received.message.reply_reference().is_some());
}

#[test]
fn explicit_ephemeral_replay_blocks_old_reply_identity_but_missing_optional_client_id_does_not() {
    let path = std::env::temp_dir().join(format!(
        "zalo-reply-ttl-replay-{}.sqlite3",
        zalo_rust::domain::local_message_id().unwrap()
    ));
    let mut store = zalo_rust::store::MessageStore::open(&path).unwrap();
    let original = incoming(Value::Null, json!({})).message;
    store.insert_batch(&[original]).unwrap();
    let missing = incoming(Value::Null, json!({"cliMsgId":null})).message;
    store.insert_batch(&[missing]).unwrap();
    assert!(
        store.page("11", "d:22", None, 10).unwrap()[0]
            .reply_reference()
            .is_some(),
        "Missing optional metadata preserves the known original reference"
    );
    let ephemeral = incoming(Value::Null, json!({"ttl":60000})).message;
    store.insert_batch(&[ephemeral]).unwrap();
    drop(store);
    let mut store = zalo_rust::store::MessageStore::open(&path).unwrap();
    assert!(
        store.page("11", "d:22", None, 10).unwrap()[0]
            .reply_reference()
            .is_none(),
        "Explicit TTL revokes the old source's repliability across restart"
    );
    assert!(store
        .prepare_outgoing_reply("11", "d:22", "Không được gửi", "9001", 1000)
        .is_err());
    store
        .insert_batch(&[incoming(Value::Null, json!({})).message])
        .unwrap();
    assert!(
        store.page("11", "d:22", None, 10).unwrap()[0]
            .reply_reference()
            .is_none(),
        "An ordinary replay cannot silently clear the recorded unsupported state"
    );
    drop(store);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn equivalent_quote_ids_across_transports_enrich_optional_client_id_without_rejecting_replay() {
    let path = std::env::temp_dir().join(format!(
        "zalo-reply-cross-transport-{}.sqlite3",
        zalo_rust::domain::local_message_id().unwrap()
    ));
    let mut store = zalo_rust::store::MessageStore::open(&path).unwrap();
    for (quote, cursor) in [
        (
            json!({"ownerId":33,"globalMsgId":7001,"cliMsgType":1,"ts":100,"msg":"Trích dẫn tổng hợp","opaque":{"future":[1,2]}}),
            "100",
        ),
        (
            json!({"ownerId":"33","globalMsgId":"7001","cliMsgId":"6001","cliMsgType":1,"ts":"100","msg":"Trích dẫn tổng hợp","opaque":{"future":[1,2]}}),
            "101",
        ),
    ] {
        let item = incoming(quote, json!({}));
        store
            .receive_batch_with_replies(
                "11",
                &[(
                    item.message,
                    item.client_id,
                    item.info,
                    item.media,
                    item.quote_content,
                )],
                Some(("fixture", cursor)),
            )
            .unwrap();
    }
    drop(store);
    let store = zalo_rust::store::MessageStore::open(&path).unwrap();
    let messages = store.page("11", "d:22", None, 10).unwrap();
    assert_eq!(messages.len(), 1);
    let quote = messages[0].quote.as_ref().unwrap();
    assert_eq!(
        (
            quote.owner_id.as_str(),
            quote.message_id.as_str(),
            quote.client_id.as_deref()
        ),
        ("33", "7001", Some("6001"))
    );
    let raw = store
        .message_quote_content("11", "d:22", "9001")
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(raw.as_json()).unwrap()["opaque"],
        json!({"future":[1,2]})
    );
    assert!(
        !format!("{:?}", messages[0]).contains("future"),
        "History snapshots never carry opaque worker data"
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT last_id FROM receive_cursors WHERE account_id='11' AND queue='fixture'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "101"
    );
    drop(conn);
    drop(store);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
