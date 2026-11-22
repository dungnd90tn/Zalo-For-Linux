//! The bundled PC client's ordinary onlyMe request and receipt.
use super::messaging::{id_string, numeric_id, service_code, Target, ThreadKind, WireError};
use serde_json::{json, Value};

pub(crate) const DIRECT_PATH: &str = "/api/message/delete";
pub(crate) const GROUP_PATH: &str = "/api/group/deletemsg";
pub(crate) use crate::domain::MAX_PAGE_SIZE as MAX_NOTICES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeleteForMeNotice {
    pub(crate) thread_id: String,
    pub(crate) message_id: String,
    pub(crate) sender_id: String,
    pub(crate) source_client_id: String,
}

fn unsigned(value: &Value, number: u64, string: &str) -> bool {
    value.as_u64() == Some(number) || value.as_str() == Some(string)
}

pub(super) fn ordinary(value: &Value) -> bool {
    value.get("e2eeExt").is_none_or(Value::is_null)
        && ["e2ee", "isE2ee", "isE2EE", "e2eeStatus"]
            .iter()
            .all(|key| {
                value.get(key).is_none_or(|marker| {
                    marker.is_null() || marker.as_bool() == Some(false) || unsigned(marker, 0, "0")
                })
            })
}

fn canonical_user(account: &str, value: &Value) -> Option<String> {
    if unsigned(value, 0, "0") {
        Some(account.to_owned())
    } else {
        id_string(value).ok()
    }
}

fn account_only(value: &Value) -> bool {
    value
        .get("onlyMe")
        .is_none_or(|flag| unsigned(flag, 1, "1"))
        && value
            .get("actionType")
            .is_none_or(|action| unsigned(action, 0, "0"))
}

/// Only the authenticated message queue may call this parser. The bundle
/// carries the deletion actor in the envelope, and the original sender and
/// recipient in each target row (469779–469814). Type 2/4 deletes prior history,
/// and actionType 1 means everyone (471398–471425).
pub(super) fn notices(
    account: &str,
    kind: ThreadKind,
    envelope: &Value,
    remaining: usize,
) -> Vec<DeleteForMeNotice> {
    if !ordinary(envelope)
        || !account_only(envelope)
        || envelope
            .get("uidFrom")
            .and_then(|value| canonical_user(account, value))
            .as_deref()
            != Some(account)
        || envelope
            .get("fromUid")
            .is_some_and(|value| canonical_user(account, value).as_deref() != Some(account))
        || envelope.get("isGroup").is_some_and(|flag| match kind {
            ThreadKind::Direct => flag.as_bool() != Some(false) && !unsigned(flag, 0, "0"),
            ThreadKind::Group => flag.as_bool() != Some(true) && !unsigned(flag, 1, "1"),
        })
    {
        return Vec::new();
    }
    let Some(rows) = envelope
        .get("content")
        .and_then(Value::as_array)
        .filter(|rows| rows.len() <= remaining)
    else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| notice(account, kind, envelope, row))
        .collect()
}

fn notice(
    account: &str,
    kind: ThreadKind,
    envelope: &Value,
    row: &Value,
) -> Option<DeleteForMeNotice> {
    if !ordinary(row)
        || !account_only(row)
        || !unsigned(row.get("actionType")?, 0, "0")
        || !row
            .get("type")
            .is_some_and(|t| unsigned(t, 1, "1") || unsigned(t, 3, "3"))
    {
        return None;
    }
    let sender_id = canonical_user(account, row.get("uidFrom")?)?;
    let destination = id_string(row.get("uidTo")?).ok()?;
    if row
        .get("destId")
        .filter(|value| !value.is_null())
        .is_some_and(|value| id_string(value).as_deref() != Ok(destination.as_str()))
    {
        return None;
    }
    let (prefix, peer) = match kind {
        ThreadKind::Direct if sender_id == account && destination != account => {
            ("d", destination.as_str())
        }
        ThreadKind::Direct if destination == account && sender_id != account => {
            ("d", sender_id.as_str())
        }
        ThreadKind::Direct => return None,
        ThreadKind::Group => ("g", destination.as_str()),
    };
    for field in ["idTo", "toUid"] {
        if let Some(value) = envelope.get(field) {
            let bound = id_string(value).ok()?;
            if bound != peer && (kind != ThreadKind::Direct || bound != account) {
                return None;
            }
        }
    }
    // The source derives msgKey from uidFrom for direct and g + idTo for group
    // (638285–638295). This field is absent on unprocessed wire frames.
    if let Some(value) = envelope.get("msgKey") {
        match kind {
            ThreadKind::Direct if canonical_user(account, value).as_deref() == Some(account) => {}
            ThreadKind::Group
                if value.as_str().and_then(|id| id.strip_prefix('g')) == Some(peer) => {}
            _ => return None,
        }
    }
    Some(DeleteForMeNotice {
        thread_id: format!("{prefix}:{peer}"),
        message_id: id_string(row.get("globalDelMsgId")?).ok()?,
        source_client_id: id_string(row.get("clientDelMsgId")?).ok()?,
        sender_id,
    })
}

pub(crate) fn parameters(
    account: &str,
    target: &Target,
    action_id: u64,
    message_id: &str,
    sender: &str,
    source_client_id: &str,
    imei: &str,
) -> Result<Value, WireError> {
    for value in [
        account,
        &target.remote_id,
        message_id,
        sender,
        source_client_id,
    ] {
        numeric_id(value)?;
    }
    if !(1..=9_007_199_254_740_991).contains(&action_id) {
        return Err(WireError::Invalid);
    }
    let destination = match target.kind {
        ThreadKind::Direct if target.remote_id != account && !imei.is_empty() => {
            if sender == account {
                target.remote_id.as_str()
            } else if sender == target.remote_id {
                account
            } else {
                return Err(WireError::Invalid);
            }
        }
        ThreadKind::Group => &target.remote_id,
        ThreadKind::Direct => return Err(WireError::Invalid),
    };
    // compact-app-pc 269915–269927, 472907–472928, 491041–491075 and
    // 633906–633945. The action ID is numeric and separate from the source ID.
    let mut parameters = json!({
        "cliMsgId": action_id,
        "msgs": [{
            "cliMsgId": source_client_id,
            "globalMsgId": message_id,
            "ownerId": sender,
            "destId": destination
        }],
        "onlyMe": 1
    });
    match target.kind {
        ThreadKind::Direct => {
            parameters["toid"] = json!(target.remote_id);
            parameters["imei"] = json!(imei);
        }
        ThreadKind::Group => {
            parameters["grid"] = json!(target.remote_id);
        }
    }
    Ok(parameters)
}

// Accept the request-bound receipt without requiring IDs absent from the original reply.
pub(crate) fn ack(payload: &Value, target: &Target, action_id: u64) -> Result<(), WireError> {
    service_code(payload)?;
    for candidate in [Some(payload), payload.get("data")].into_iter().flatten() {
        for key in ["cliMsgId", "clientId"] {
            if let Some(value) = candidate.get(key) {
                if id_string(value)? != action_id.to_string() {
                    return Err(WireError::Invalid);
                }
            }
        }
        for (key, kind) in [("toid", ThreadKind::Direct), ("grid", ThreadKind::Group)] {
            if let Some(value) = candidate.get(key) {
                if kind != target.kind || id_string(value)? != target.remote_id {
                    return Err(WireError::Invalid);
                }
            }
        }
        if candidate
            .get("onlyMe")
            .is_some_and(|value| value.as_u64() != Some(1))
        {
            return Err(WireError::Invalid);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_delete_notice_binds_own_actor_sender_and_destination() {
        let incoming = json!({"msgType":"chat.delete","uidFrom":"0","idTo":"22","msgKey":"0","content":[
            {"type":1,"actionType":0,"globalDelMsgId":"70","clientDelMsgId":"18",
                "uidFrom":"22","uidTo":"11","destId":"11"},
            {"type":"3","actionType":"0","globalDelMsgId":71,"clientDelMsgId":19,
                "uidFrom":"0","uidTo":"22","destId":"22"}
        ]});
        let parsed = notices("11", ThreadKind::Direct, &incoming, MAX_NOTICES);
        assert_eq!(
            parsed,
            [
                DeleteForMeNotice {
                    thread_id: "d:22".into(),
                    message_id: "70".into(),
                    sender_id: "22".into(),
                    source_client_id: "18".into()
                },
                DeleteForMeNotice {
                    thread_id: "d:22".into(),
                    message_id: "71".into(),
                    sender_id: "11".into(),
                    source_client_id: "19".into()
                },
            ]
        );
        let group = json!({"uidFrom":11,"idTo":"33","msgKey":"g33","isGroup":true,"content":[
            {"type":3,"actionType":0,"globalDelMsgId":"77","clientDelMsgId":"19",
                "uidFrom":"44","uidTo":"33","destId":"33"}
        ]});
        let parsed = notices("11", ThreadKind::Group, &group, MAX_NOTICES);
        assert_eq!(
            parsed,
            [DeleteForMeNotice {
                thread_id: "g:33".into(),
                message_id: "77".into(),
                sender_id: "44".into(),
                source_client_id: "19".into()
            },]
        );
        for (field, invalid) in [
            ("uidFrom", json!("22")),
            ("idTo", json!("99")),
            ("msgKey", json!("g99")),
            ("isGroup", json!(false)),
            ("onlyMe", json!(0)),
            ("e2ee", json!(true)),
            ("actionType", json!(1)),
            ("fromUid", json!("33")),
        ] {
            let mut malformed = group.clone();
            malformed[field] = invalid;
            assert!(notices("11", ThreadKind::Group, &malformed, MAX_NOTICES).is_empty());
        }
        for (field, invalid) in [
            ("actionType", Value::Null),
            ("actionType", json!(1)),
            ("actionType", json!(2)),
            ("type", json!(2)),
            ("type", json!(4)),
            ("destId", json!("22")),
            ("globalDelMsgId", json!("0")),
            ("clientDelMsgId", json!("-1")),
        ] {
            let mut malformed = group.clone();
            malformed["content"][0][field] = invalid;
            assert!(notices("11", ThreadKind::Group, &malformed, MAX_NOTICES).is_empty());
        }
        assert!(notices("11", ThreadKind::Direct, &incoming, 1).is_empty());
    }

    #[test]
    fn delete_for_me_requests_match_source_destination_and_only_me() {
        let direct = Target::from_thread("d:22").unwrap();
        let group = Target::from_thread("g:33").unwrap();
        let build = |target, sender| parameters("11", target, 500, "70", sender, "18", "fixture");
        assert_eq!(
            build(&direct, "11").unwrap(),
            json!({"toid":"22","imei":"fixture","cliMsgId":500,"onlyMe":1,"msgs":[
                {"cliMsgId":"18","globalMsgId":"70","ownerId":"11","destId":"22"}
            ]})
        );
        assert_eq!(
            build(&direct, "22").unwrap(),
            json!({"toid":"22","imei":"fixture","cliMsgId":500,"onlyMe":1,"msgs":[
                {"cliMsgId":"18","globalMsgId":"70","ownerId":"22","destId":"11"}
            ]})
        );
        assert_eq!(
            build(&group, "44").unwrap(),
            json!({"grid":"33","cliMsgId":500,"onlyMe":1,"msgs":[
                {"cliMsgId":"18","globalMsgId":"70","ownerId":"44","destId":"33"}
            ]})
        );
        assert!(build(&direct, "44").is_err());
        assert!(build(&Target::from_thread("d:11").unwrap(), "11").is_err());
    }

    #[test]
    fn delete_for_me_receipt_cannot_confirm_another_scope_or_everyone() {
        let target = Target::from_thread("g:33").unwrap();
        assert!(ack(&json!({"error_code":0}), &target, 500).is_ok());
        assert!(ack(
            &json!({"error_code":0,"data":{"clientId":"500","grid":"33","onlyMe":1}}),
            &target,
            500
        )
        .is_ok());
        for payload in [
            json!({"error_code":0,"data":{"clientId":"501"}}),
            json!({"error_code":0,"data":{"toid":"33"}}),
            json!({"error_code":0,"data":{"grid":"22"}}),
            json!({"error_code":0,"data":{"onlyMe":0}}),
            json!({"error_code":211}),
        ] {
            assert!(ack(&payload, &target, 500).is_err());
        }
    }
}
