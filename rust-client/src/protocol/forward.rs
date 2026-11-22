//! Single-recipient ordinary text forwarding and its client-ID-bound response.
use super::messaging::{
    id_string, numeric_id, service_code, SendAck, Target, ThreadKind, WireError,
};
use crate::domain::{canonical_plain_text, Message, MAX_TEXT_BYTES};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(crate) const DIRECT_PATH: &str = "/api/message/mforward";
pub(crate) const GROUP_PATH: &str = "/api/group/mforward";
const MAX_CLIENT_ID: u64 = 9_007_199_254_740_991;
const MAX_REFERENCE_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForwardRoot {
    id: String,
    ts: i64,
    log_src_type: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForwardReference {
    #[serde(flatten)]
    source: ForwardRoot,
    fw_lvl: u32,
    root_msg_ref: ForwardRoot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ForwardMarker {
    Ordinary,
    Parsed(ForwardReference),
    Unavailable,
    /// Authenticated structured webchat, without proof of an ordinary text echo.
    StructuredWebchat,
}

impl ForwardMarker {
    pub(crate) fn parse(reference: &Value) -> Option<Self> {
        if reference.get("type").and_then(Value::as_str) == Some("3") {
            return Some(Self::Unavailable);
        }
        match ForwardReference::from_wire(reference) {
            Ok(Some(reference)) => Some(Self::Parsed(reference)),
            Ok(None) => None,
            Err(_) => Some(Self::Unavailable),
        }
    }
}

impl ForwardReference {
    /// The caller supplies a committed, ordinary text message, with eligibility
    /// and the absence of a previous forwarding reference already established.
    pub(crate) fn from_source(message: &Message) -> Result<Self, WireError> {
        let scoped = Target::from_thread(&message.thread_id)?;
        let cli = message
            .source_client_id
            .as_deref()
            .filter(|_| message.reply_reference().is_some())
            .ok_or(WireError::Unsupported)?;
        if message.quote.is_some() || !(1..=MAX_CLIENT_ID as i64).contains(&message.sent_at_ms) {
            return Err(WireError::Unsupported);
        }
        let (conversation_id, log_src_type) = match scoped.kind {
            ThreadKind::Direct => (scoped.remote_id, 1),
            ThreadKind::Group => (format!("g{}", scoped.remote_id), 2),
        };
        let mut digest = Md5::new();
        digest.update(cli.as_bytes());
        digest.update(message.sender.as_bytes());
        digest.update(conversation_id.as_bytes());
        let source = ForwardRoot {
            id: format!("{:x}", digest.finalize()),
            ts: message.sent_at_ms,
            log_src_type,
        };
        Ok(Self {
            root_msg_ref: source.clone(),
            source,
            fw_lvl: 1,
        })
    }

    /// Classify incoming top-level reference metadata without treating an
    /// unrecognized or malformed forward reference as an ordinary text echo.
    pub(crate) fn from_wire(reference: &Value) -> Result<Option<Self>, WireError> {
        if reference.get("type").and_then(Value::as_u64) != Some(3) {
            return Ok(None);
        }
        let data = reference.get("data").ok_or(WireError::Invalid)?;
        let parsed = if let Some(text) = data.as_str() {
            if text.len() > MAX_REFERENCE_BYTES {
                return Err(WireError::Oversize);
            }
            serde_json::from_str(text)
        } else {
            serde_json::from_value(data.clone())
        };
        let parsed: Self = parsed.map_err(|_| WireError::Invalid)?;
        if parsed.fw_lvl == 0 || !valid_root(&parsed.source) || !valid_root(&parsed.root_msg_ref) {
            return Err(WireError::Invalid);
        }
        Ok(Some(parsed))
    }
}

fn valid_root(root: &ForwardRoot) -> bool {
    root.id.len() == 32
        && root.id.bytes().all(|byte| byte.is_ascii_hexdigit())
        && (1..=MAX_CLIENT_ID as i64).contains(&root.ts)
        && root.log_src_type <= 5
}

pub(crate) fn parameters(
    recipient: &Target,
    client_id: u64,
    imei: &str,
    text: &str,
    reference: &ForwardReference,
) -> Result<Value, WireError> {
    if !(1..=MAX_CLIENT_ID).contains(&client_id)
        || (recipient.kind == ThreadKind::Direct && imei.is_empty())
        || text.len() > MAX_TEXT_BYTES
        || canonical_plain_text(text).is_empty()
        || text.chars().any(|c| matches!(c, '\u{0}'..='\u{7}'))
        || !valid_root(&reference.source)
        || reference.fw_lvl != 1
        || reference.source != reference.root_msg_ref
    {
        return Err(WireError::Invalid);
    }
    numeric_id(&recipient.remote_id)?;
    let inner_ref = json!({
        "type": 3,
        "data": serde_json::to_string(reference).map_err(|_| WireError::Invalid)?,
    })
    .to_string();
    let root_log = |root: &ForwardRoot| json!({"st":root.log_src_type,"ts":root.ts,"id":root.id});
    let decor_log = json!({
        "fw": {
            "pmsg": root_log(&reference.source),
            "rmsg": root_log(&reference.root_msg_ref),
            "fwLvl": reference.fw_lvl
        }
    })
    .to_string();
    let mut params = json!({
        "ttl": 0,
        "msgType": "1",
        "totalIds": 1,
        "msgInfo": json!({"message":text,"reference":inner_ref}).to_string(),
        "decorLog": decor_log,
    });
    match recipient.kind {
        ThreadKind::Direct => {
            params["toIds"] =
                json!([{"toUid":recipient.remote_id,"clientId":client_id.to_string(),"ttl":0}]);
            params["imei"] = json!(imei);
        }
        ThreadKind::Group => {
            params["grids"] =
                json!([{"grid":recipient.remote_id,"clientId":client_id.to_string(),"ttl":0}]);
        }
    }
    Ok(params)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ForwardAck {
    Accepted(SendAck),
    Rejected,
}

/// The response is already authenticated and decrypted. Unbound or ambiguous
/// rows are errors and the caller must retain the durable unknown-send state.
pub(crate) fn ack(
    payload: &Value,
    recipient: &Target,
    expected_client_id: u64,
) -> Result<ForwardAck, WireError> {
    if !(1..=MAX_CLIENT_ID).contains(&expected_client_id) {
        return Err(WireError::Invalid);
    }
    numeric_id(&recipient.remote_id)?;
    service_code(payload)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    let success = data
        .get("success")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    let failed = data
        .get("failed")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    let (entry, accepted) = match (success.as_slice(), failed.as_slice()) {
        ([entry], []) => (entry, true),
        ([], [entry]) => (entry, false),
        _ => return Err(WireError::Invalid),
    };
    let client = id_string(entry.get("clientId").ok_or(WireError::Invalid)?)?;
    if client != expected_client_id.to_string() {
        return Err(WireError::Invalid);
    }
    if let Some(destination) = entry.get("toUid") {
        let matches = match (
            recipient.kind,
            destination.as_str().and_then(|id| id.strip_prefix('g')),
        ) {
            (ThreadKind::Group, Some(id)) => id == recipient.remote_id,
            _ => id_string(destination)? == recipient.remote_id,
        };
        if !matches {
            return Err(WireError::Invalid);
        }
    }
    if recipient.kind == ThreadKind::Group {
        if let Some(destination) = entry.get("grid") {
            if id_string(destination)? != recipient.remote_id {
                return Err(WireError::Invalid);
            }
        }
    }
    if !accepted {
        return if entry.get("error").is_some_and(|error| !error.is_null())
            && entry.get("msgId").is_none()
        {
            Ok(ForwardAck::Rejected)
        } else {
            Err(WireError::Invalid)
        };
    }
    if entry.get("error").is_some_and(|error| !error.is_null()) {
        return Err(WireError::Invalid);
    }
    let ack = super::messaging::send_ack(&json!({"error_code":0,"data":entry}))?;
    Ok(ForwardAck::Accepted(ack))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expanded(mut wire: Value) -> Value {
        let info: Value = serde_json::from_str(wire["msgInfo"].as_str().unwrap()).unwrap();
        let mut reference: Value =
            serde_json::from_str(info["reference"].as_str().unwrap()).unwrap();
        reference["data"] = serde_json::from_str(reference["data"].as_str().unwrap()).unwrap();
        wire["msgInfo"] = json!({"message":info["message"],"reference":reference});
        wire["decorLog"] = serde_json::from_str(wire["decorLog"].as_str().unwrap()).unwrap();
        wire
    }

    #[test]
    fn source_oracle_matches_direct_forward_request_and_echo_reference() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/forward.json"))
                .unwrap();
        let mut supported = 0;
        for case in fixtures["cases"].as_array().unwrap() {
            if case["native_supported"] != true {
                continue;
            }
            supported += 1;
            let original = &case["original"];
            let conversation = original["toUid"].as_str().unwrap();
            let thread_id = match conversation.strip_prefix('g') {
                Some(group) => format!("g:{group}"),
                None => format!("d:{conversation}"),
            };
            let owner = original["fromUid"].as_str().unwrap();
            let message = Message {
                account_id: "11".into(),
                thread_id,
                id: original["msgId"].as_str().unwrap().into(),
                sender: if owner == "0" { "11" } else { owner }.into(),
                text: original["message"].as_str().unwrap().into(),
                sent_at_ms: original["sendDttm"].as_i64().unwrap(),
                expires_at_ms: None,
                source_client_id: original["cliMsgId"].as_str().map(str::to_owned),
                quote: None,
                media: None,
                sticker: None,
                mentions: Vec::new(),
                reply_blocked: false,
                recalled: false,
            };
            let reference = ForwardReference::from_source(&message).unwrap();
            let target = Target::new(ThreadKind::Direct, case["toUid"].as_str().unwrap()).unwrap();
            let request =
                parameters(&target, 7001, "synthetic-device", &message.text, &reference).unwrap();
            let expected = expanded(case["wire"]["payload"].clone());
            assert_eq!(case["wire"]["endpoint"]["path"], DIRECT_PATH);
            assert_eq!(expanded(request), expected, "{}", case["name"]);
            let echo_reference = &expected["msgInfo"]["reference"];
            assert_eq!(
                ForwardReference::from_wire(echo_reference).unwrap(),
                Some(reference.clone())
            );
            let echo_string = json!({"type":3,"data":echo_reference["data"].to_string()});
            assert_eq!(
                ForwardReference::from_wire(&echo_string).unwrap(),
                Some(reference)
            );

            let mut unsupported = message;
            unsupported.source_client_id = None;
            assert_eq!(
                ForwardReference::from_source(&unsupported),
                Err(WireError::Unsupported)
            );
        }
        assert_eq!(supported, 3);
        assert_eq!(ForwardReference::from_wire(&json!({"type":1})), Ok(None));
        assert!(ForwardReference::from_wire(&json!({"type":3,"data":"{"})).is_err());
    }

    #[test]
    fn forward_ack_binds_one_row_to_the_submitted_client_and_recipient() {
        let target = Target::new(ThreadKind::Direct, "33").unwrap();
        let envelope = |success, failed| {
            json!({
                "error_code":0,"data":{"success":success,"failed":failed}
            })
        };
        let accepted = json!({"clientId":"7001","msgId":"9001","ts":"1700000000123"});
        assert_eq!(
            ack(&envelope(json!([accepted]), json!([])), &target, 7001),
            Ok(ForwardAck::Accepted(SendAck {
                server_id: "9001".into(),
                timestamp_ms: Some(1700000000123),
            }))
        );
        let mut legacy = accepted.clone();
        legacy["grid"] = Value::Null;
        assert!(matches!(
            ack(&envelope(json!([legacy]), json!([])), &target, 7001),
            Ok(ForwardAck::Accepted(_))
        ));
        let rejected = json!({"clientId":7001,"error":{"code":211}});
        assert_eq!(
            ack(&envelope(json!([]), json!([rejected])), &target, 7001),
            Ok(ForwardAck::Rejected)
        );
        for ambiguous in [
            envelope(json!([{"clientId":"7000","msgId":"9001"}]), json!([])),
            envelope(
                json!([{"clientId":"7001","msgId":"9001","toUid":"34"}]),
                json!([]),
            ),
            envelope(json!([{"clientId":"7001","msgId":"0"}]), json!([])),
            envelope(json!([accepted, accepted]), json!([])),
            envelope(json!([accepted]), json!([rejected])),
            envelope(json!([]), json!([])),
            envelope(json!([]), json!([{"clientId":"7001","error":null}])),
            json!({"error_code":0,"data":{"msgId":"9001"}}),
        ] {
            assert_eq!(ack(&ambiguous, &target, 7001), Err(WireError::Invalid));
        }
    }

    #[test]
    fn group_forward_uses_grid_ttl_and_binds_any_explicit_ack_destination() {
        let group = Target::new(ThreadKind::Group, "33").unwrap();
        let root = ForwardRoot {
            id: "11111111111111111111111111111111".into(),
            ts: 1700000000000,
            log_src_type: 1,
        };
        let reference = ForwardReference {
            source: root.clone(),
            root_msg_ref: root,
            fw_lvl: 1,
        };
        let request =
            parameters(&group, 7001, "not-a-group-field", "Tin nhắn", &reference).unwrap();
        assert_eq!(
            request["grids"],
            json!([{"grid":"33","clientId":"7001","ttl":0}])
        );
        assert_eq!(request["ttl"], 0);
        assert_eq!(request["totalIds"], 1);
        assert!(request.get("toIds").is_none());
        assert!(request.get("imei").is_none());
        assert!(request.get("visibility").is_none());
        let reply = |entry| json!({"error_code":0,"data":{"success":[entry],"failed":[]}});
        let exact = json!({"clientId":"7001","msgId":"9001"});
        assert!(matches!(
            ack(&reply(exact.clone()), &group, 7001),
            Ok(ForwardAck::Accepted(_))
        ));
        let mut bound = exact;
        bound["grid"] = json!(33);
        bound["toUid"] = json!("g33");
        assert!(matches!(
            ack(&reply(bound.clone()), &group, 7001),
            Ok(ForwardAck::Accepted(_))
        ));
        bound["grid"] = json!("34");
        assert_eq!(
            ack(&reply(bound.clone()), &group, 7001),
            Err(WireError::Invalid)
        );
        bound["grid"] = json!("33");
        bound["toUid"] = json!("g34");
        assert_eq!(ack(&reply(bound), &group, 7001), Err(WireError::Invalid));
    }
}
