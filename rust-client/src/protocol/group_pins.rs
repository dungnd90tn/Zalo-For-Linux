//! Ordinary group text pins from the PC GROUP_BOARD v2 contract.
use super::group_roster;
use super::messaging::{id_string, numeric_id, service_code, Target, ThreadKind, WireError};
use crate::domain::MAX_TEXT_BYTES;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) const LIST_PATH: &str = "/api/board/pin/list";
pub(crate) const PIN_PATH: &str = "/api/board/topic/createv2";
pub(crate) const UNPIN_PATH: &str = "/api/board/unpinv2";
const MSG_TOPIC: u64 = 2;
const MSG_TEXT: u64 = 1;
const MAX_ITEMS: usize = 100;
const MAX_NAME_BYTES: usize = 256;
const MAX_BOARD_VERSION: u64 = (1 << 53) - 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PinReference {
    pub message_id: String,
    pub client_id: String,
    pub sender_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PinEntry {
    pub topic_id: String,
    pub reference: PinReference,
    wire_id: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PinSnapshot {
    pub version: u64,
    pub limit: usize,
    pub total: usize,
    pub items: Vec<PinEntry>,
    group_id: String,
    wire_version: Value,
}

fn group(target: &Target) -> Result<(), WireError> {
    if target.kind != ThreadKind::Group {
        return Err(WireError::Unsupported);
    }
    numeric_id(&target.remote_id)
}

pub(crate) fn list_parameters(target: &Target, imei: &str) -> Result<Value, WireError> {
    group(target)?;
    // No local board cache is authoritative, so request a full list.
    Ok(json!({"groupId":target.remote_id,"boardVersion":0,"imei":imei}))
}

pub(crate) fn pin_parameters(
    target: &Target,
    source: &PinReference,
    title: &str,
    sender_name: &str,
    imei: &str,
) -> Result<Value, WireError> {
    group(target)?;
    numeric_id(&source.message_id)?;
    numeric_id(&source.client_id)?;
    numeric_id(&source.sender_id)?;
    if title.len() > MAX_TEXT_BYTES || sender_name.len() > MAX_NAME_BYTES {
        return Err(WireError::Oversize);
    }
    if title.trim().is_empty() || sender_name.chars().any(char::is_control) {
        return Err(WireError::Invalid);
    }
    let params = json!({
        "client_msg_id":source.client_id,
        "global_msg_id":source.message_id,
        "senderUid":source.sender_id,
        "senderName":sender_name,
        "title":title,
        "msg_type":MSG_TEXT,
    });
    Ok(json!({
        "grid":target.remote_id,
        "type":MSG_TOPIC,
        "color":-16777216,
        "emoji":"📌",
        "startTime":-1,
        "duration":-1,
        "params":params.to_string(),
        "repeat":0,
        "src":-1,
        "imei":imei,
        "pinAct":1,
    }))
}

pub(crate) fn unpin_parameters(
    target: &Target,
    snapshot: &PinSnapshot,
    topic_id: &str,
    imei: &str,
) -> Result<Value, WireError> {
    group(target)?;
    if snapshot.group_id != target.remote_id {
        return Err(WireError::Invalid);
    }
    let entry = snapshot
        .items
        .iter()
        .find(|entry| entry.topic_id == topic_id)
        .ok_or(WireError::Invalid)?;
    Ok(json!({
        "grid":target.remote_id,
        "topic":{"topicId":entry.wire_id,"topicType":MSG_TOPIC},
        "boardVersion":snapshot.wire_version,
        "imei":imei,
    }))
}

fn board_version(value: &Value) -> Result<u64, WireError> {
    value
        .as_u64()
        .or_else(|| {
            let text = value.as_str()?;
            if text.is_empty() || text.len() > 16 || !text.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            text.parse().ok()
        })
        .filter(|version| *version <= MAX_BOARD_VERSION)
        .ok_or(WireError::Invalid)
}

fn bound_group(value: &Value, expected: &str) -> Result<(), WireError> {
    for key in ["groupId", "group_id", "grid"] {
        if let Some(id) = value.get(key) {
            if id_string(id)? != expected {
                return Err(WireError::Invalid);
            }
        }
    }
    Ok(())
}

fn text_reference(value: &Value) -> Result<Option<PinReference>, WireError> {
    let decoded;
    let parameters = if let Some(encoded) = value.as_str() {
        if encoded.len() > 6 * MAX_TEXT_BYTES + 4096 {
            return Err(WireError::Oversize);
        }
        decoded = serde_json::from_str::<Value>(encoded).map_err(|_| WireError::Invalid)?;
        &decoded
    } else {
        value
    };
    if parameters
        .get("msg_type")
        .and_then(Value::as_u64)
        .ok_or(WireError::Invalid)?
        != MSG_TEXT
    {
        return Ok(None);
    }
    let id = |key| {
        parameters
            .get(key)
            .ok_or(WireError::Invalid)
            .and_then(id_string)
    };
    Ok(Some(PinReference {
        message_id: id("global_msg_id")?,
        client_id: id("client_msg_id")?,
        sender_id: id("senderUid")?,
    }))
}

pub(crate) fn parse_snapshot(payload: &Value, target: &Target) -> Result<PinSnapshot, WireError> {
    group(target)?;
    service_code(payload)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    bound_group(data, &target.remote_id)?;
    let version = data.get("boardVersion").ok_or(WireError::Invalid)?;
    let limit = data
        .get("pinLimit")
        .and_then(Value::as_u64)
        .filter(|value| (1..=MAX_ITEMS as u64).contains(value))
        .ok_or(WireError::Invalid)? as usize;
    let rows = data
        .get("items")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    if rows.len() > MAX_ITEMS {
        return Err(WireError::Oversize);
    }
    let mut items = Vec::new();
    let mut topic_ids = BTreeSet::new();
    let mut message_ids = BTreeSet::new();
    for row in rows {
        bound_group(row, &target.remote_id)?;
        if row
            .get("type")
            .and_then(Value::as_u64)
            .ok_or(WireError::Invalid)?
            != MSG_TOPIC
        {
            continue;
        }
        let wire_id = row.get("id").ok_or(WireError::Invalid)?;
        let topic_id = id_string(wire_id)?;
        if !topic_ids.insert(topic_id.clone()) {
            return Err(WireError::Invalid);
        }
        let parameters = row.get("params").ok_or(WireError::Invalid)?;
        if let Some(reference) = text_reference(parameters)? {
            if !message_ids.insert(reference.message_id.clone()) {
                return Err(WireError::Invalid);
            }
            items.push(PinEntry {
                topic_id,
                reference,
                wire_id: wire_id.clone(),
            });
        }
    }
    Ok(PinSnapshot {
        version: board_version(version)?,
        limit,
        total: rows.len(),
        items,
        group_id: target.remote_id.clone(),
        wire_version: version.clone(),
    })
}

pub(crate) fn parse_permission(
    payload: &Value,
    account: &str,
    target: &Target,
) -> Result<bool, WireError> {
    Ok(group_roster::verify_group(payload, account, target)?.can_pin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> Target {
        Target::new(ThreadKind::Group, "71").unwrap()
    }

    fn reference() -> PinReference {
        PinReference {
            message_id: "9007199254740993".into(),
            client_id: "1234001".into(),
            sender_id: "33".into(),
        }
    }

    fn snapshot() -> Value {
        let mut params =
            pin_parameters(&group(), &reference(), "Chào nhóm", "An", "device").unwrap();
        let text_parameters = params["params"].take();
        json!({"error_code":0,"data":{
            "groupId":"71","boardVersion":"75","pinLimit":10,
            "items":[
                {"id":6,"type":0,"params":{}},
                {"id":7,"type":2,"params":text_parameters},
                {"id":"8","type":2,"params":{"msg_type":3}}
            ]
        }})
    }

    #[test]
    fn board_requests_match_pc_contract_and_keep_exact_wire_scalars() {
        let target = group();
        assert_eq!(
            list_parameters(&target, "device").unwrap(),
            json!({"groupId":"71","boardVersion":0,"imei":"device"})
        );

        let mut pin = pin_parameters(&target, &reference(), "Chào nhóm\n", "An", "device").unwrap();
        let encoded = pin.as_object_mut().unwrap().remove("params").unwrap();
        assert_eq!(
            pin,
            json!({
                "grid":"71","type":2,"color":-16777216,"emoji":"📌",
                "startTime":-1,"duration":-1,"repeat":0,"src":-1,"imei":"device","pinAct":1
            })
        );
        assert_eq!(
            serde_json::from_str::<Value>(encoded.as_str().unwrap()).unwrap(),
            json!({
                "client_msg_id":"1234001","global_msg_id":"9007199254740993",
                "senderUid":"33","senderName":"An","title":"Chào nhóm\n","msg_type":1
            })
        );

        let board = parse_snapshot(&snapshot(), &target).unwrap();
        assert_eq!((board.version, board.total, board.limit), (75, 3, 10));
        assert_eq!(board.items.len(), 1);
        assert_eq!(board.items[0].reference, reference());
        assert_eq!(
            unpin_parameters(&target, &board, "7", "device").unwrap(),
            json!({"grid":"71","topic":{"topicId":7,"topicType":2},"boardVersion":"75","imei":"device"})
        );
        assert_eq!(
            unpin_parameters(&target, &board, "8", "device"),
            Err(WireError::Invalid)
        );
        let other = Target::new(ThreadKind::Group, "72").unwrap();
        assert_eq!(
            unpin_parameters(&other, &board, "7", "device"),
            Err(WireError::Invalid)
        );
    }

    #[test]
    fn board_rejects_ambiguous_rows_and_partial_or_unscoped_results() {
        for (key, value) in [
            ("groupId", json!("72")),
            ("boardVersion", json!(9_007_199_254_740_992u64)),
            ("pinLimit", json!(0)),
        ] {
            let mut payload = snapshot();
            payload["data"][key] = value;
            assert!(parse_snapshot(&payload, &group()).is_err());
        }
        let mut payload = snapshot();
        payload["data"]["items"][1]["params"] = json!({"msg_type":1,"client_msg_id":"1234001"});
        assert_eq!(parse_snapshot(&payload, &group()), Err(WireError::Invalid));

        let mut payload = snapshot();
        let mut duplicate = payload["data"]["items"][1].clone();
        duplicate["id"] = json!("9");
        payload["data"]["items"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert_eq!(parse_snapshot(&payload, &group()), Err(WireError::Invalid));

        let unchanged = json!({"error_code":265,"data":null});
        assert_eq!(
            parse_snapshot(&unchanged, &group()),
            Err(WireError::Service(265))
        );
        let direct = Target::new(ThreadKind::Direct, "71").unwrap();
        assert_eq!(
            list_parameters(&direct, "device"),
            Err(WireError::Unsupported)
        );
    }

    #[test]
    fn pin_permission_requires_a_verified_member_and_exact_topic_setting() {
        let mut roster = json!({"error_code":0,"data":{"gridInfoMap":{"71":{
            "groupId":"71","type":1,"creatorId":"33","totalMember":3,"adminIds":["34"],
            "memVerList":["33_0","34_0","35_0"],"setting":{"setTopicOnly":1}
        }}}});
        let target = group();
        assert_eq!(parse_permission(&roster, "33", &target), Ok(true));
        assert_eq!(parse_permission(&roster, "34", &target), Ok(true));
        assert_eq!(parse_permission(&roster, "35", &target), Ok(false));
        assert!(parse_permission(&roster, "36", &target).is_err());
        for value in [json!(false), json!("0"), Value::Null] {
            roster["data"]["gridInfoMap"]["71"]["setting"]["setTopicOnly"] = value;
            assert_eq!(parse_permission(&roster, "35", &target), Ok(false));
        }
        roster["data"]["gridInfoMap"]["71"]["setting"]["setTopicOnly"] = json!(0);
        assert_eq!(parse_permission(&roster, "35", &target), Ok(true));
        roster["data"]["gridInfoMap"]["71"]["setting"] = Value::Null;
        assert_eq!(parse_permission(&roster, "33", &target), Ok(false));
    }
}
