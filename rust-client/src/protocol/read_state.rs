//! Ordinary read receipts and mute settings from the bundled desktop protocol.
use super::messaging::{id_string, numeric_id, Target, ThreadKind, WireError, MAX_BATCH};
use crate::domain::{
    read_state::{
        DeliveryState, ReceiptBinding, ReceiptReference, ReceiptUpdate, ServerReadWatermark,
    },
    Message,
};
use serde_json::{json, Value};
use std::collections::HashSet;

fn integer(value: Option<&Value>) -> Option<i64> {
    value.and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
}

fn flag(value: Option<&Value>) -> Option<bool> {
    value
        .and_then(|v| v.as_bool())
        .or_else(|| match integer(value) {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        })
}

fn id(value: Option<&Value>) -> Option<String> {
    id_string(value?).ok()
}

pub(super) fn incoming_reference(
    raw: &Value,
    message: &Message,
    recipient: String,
    message_type: &str,
    client_id: Option<String>,
) -> ReceiptBinding {
    let eligible = integer(raw.pointer("/paramsExt/countUnread")) != Some(0)
        && raw.pointer("/content/action").and_then(Value::as_str) != Some("msginfo.actionlist")
        && matches!(
            message_type,
            "webchat"
                | "share.file"
                | "chat.sticker"
                | "chat.photo"
                | "chat.voice"
                | "chat.webcontent"
                | "chat.recommended"
        );
    ReceiptBinding {
        account_id: message.account_id.clone(),
        thread_id: message.thread_id.clone(),
        message_id: message.id.clone(),
        reference: ReceiptReference {
            message_id: message.id.clone(),
            client_id,
            sender_id: message.sender.clone(),
            recipient_id: recipient,
            message_type: message_type.to_owned(),
            st: integer(raw.get("st")).filter(|&n| n >= 0).unwrap_or(-1),
            at: integer(raw.get("at")).filter(|&n| n >= 0).unwrap_or(-1),
            command: integer(raw.get("cmd")).filter(|&n| n >= 0).unwrap_or(-1),
            sent_at_ms: message.sent_at_ms,
        },
        counts_unread: eligible,
    }
}

pub(crate) fn seen_v2_parameters(
    target: &Target,
    references: &[ReceiptReference],
    imei: &str,
) -> Result<Value, WireError> {
    if references.is_empty() || references.len() > 50 {
        return Err(WireError::Invalid);
    }
    let mut data = Vec::with_capacity(references.len());
    for reference in references {
        let client = reference.client_id.as_deref().ok_or(WireError::Invalid)?;
        numeric_id(client)?;
        numeric_id(&reference.message_id)?;
        numeric_id(&reference.sender_id)?;
        numeric_id(&reference.recipient_id)?;
        let recipient =
            if target.kind == ThreadKind::Direct && reference.sender_id == reference.recipient_id {
                "0"
            } else {
                &reference.recipient_id
            };
        data.push(json!({
            "cmi": client, "gmi": reference.message_id, "si": reference.sender_id,
            "di": recipient, "mt": reference.message_type, "st": reference.st,
            "at": reference.at, "cmd": reference.command,
            "ts": if reference.sent_at_ms > 0 { reference.sent_at_ms } else { -1 },
        }));
    }
    let inner = match target.kind {
        ThreadKind::Direct => json!({"data":data, "senderId":target.remote_id}),
        ThreadKind::Group => json!({"data":data, "grid":target.remote_id}),
    };
    Ok(json!({"msgInfos":inner.to_string(), "imei":imei}))
}

#[derive(Default)]
pub(crate) struct StatusChanges {
    pub deliveries: Vec<ReceiptUpdate>,
    pub watermarks: Vec<ServerReadWatermark>,
}

pub(crate) fn statuses(account: &str, payload: &Value) -> StatusChanges {
    payload
        .get("data")
        .map_or_else(StatusChanges::default, |data| {
            statuses_from_data(account, data)
        })
}

pub(crate) fn statuses_from_data(account: &str, data: &Value) -> StatusChanges {
    let mut result = StatusChanges::default();
    let Some(data) = data.as_object() else {
        return result;
    };
    for (key, group_seen) in [
        ("delivereds", false),
        ("groupSeens", true),
        ("seens", false),
    ] {
        let Some(entries) = data
            .get(key)
            .and_then(Value::as_array)
            .filter(|a| a.len() <= MAX_BATCH)
        else {
            continue;
        };
        for entry in entries {
            let Some(message_id) = id(entry.get("msgId")) else {
                continue;
            };
            let client_id = id(entry.get("cliMsgId"));
            let raw_thread = entry.get("threadId");
            let peer = id(raw_thread);
            let prefixed_group = raw_thread
                .and_then(Value::as_str)
                .and_then(|id| id.strip_prefix('g'))
                .filter(|id| numeric_id(id).is_ok());
            let peers = entry.get("deliveredUids").and_then(Value::as_array);
            let explicit_group = id(entry.get("groupId"));
            if group_seen && explicit_group.is_none() {
                continue;
            }
            if peers
                .is_some_and(|uids| uids.len() == 1 && id(uids.first()).as_deref() == Some(account))
            {
                continue;
            }
            let (kind, remote) = if let Some(group) = explicit_group {
                (ThreadKind::Group, group)
            } else if let Some(group) = prefixed_group {
                (ThreadKind::Group, group.to_owned())
            } else if key == "seens" || peer.is_some() {
                let Some(peer) = peer.or_else(|| id(entry.get("idTo"))) else {
                    continue;
                };
                let kind = if key == "seens" && flag(entry.get("isGroup")) == Some(true) {
                    ThreadKind::Group
                } else {
                    ThreadKind::Direct
                };
                (kind, peer)
            } else if let Some(peer) = peers
                .filter(|uids| uids.len() == 1)
                .and_then(|uids| id(uids.first()))
            {
                (ThreadKind::Direct, peer)
            } else {
                continue;
            };
            if kind == ThreadKind::Direct
                && (remote == account
                    || peers.is_some_and(|uids| {
                        !uids.is_empty()
                            && (uids.len() != 1 || id(uids.first()).as_deref() != Some(&remote))
                    }))
            {
                continue;
            }
            let Ok(target) = Target::new(kind, &remote) else {
                continue;
            };
            let thread_id = target.thread_id();
            let seen = group_seen || key == "seens" || flag(entry.get("seen")) == Some(true);
            let mut add = |reader_id, state| {
                if result.deliveries.len() < MAX_BATCH {
                    result.deliveries.push(ReceiptUpdate {
                        account_id: account.into(),
                        thread_id: thread_id.clone(),
                        message_id: message_id.clone(),
                        client_id: client_id.clone(),
                        reader_id,
                        state,
                    });
                }
            };
            if kind == ThreadKind::Direct {
                // The authenticated direct thread binds the only remote participant.
                add(
                    Some(remote),
                    if seen {
                        DeliveryState::Seen
                    } else {
                        DeliveryState::Delivered
                    },
                );
            } else if seen {
                let mut readers = entry
                    .get("seenUids")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .take(MAX_BATCH)
                    .filter_map(|v| id(Some(v)))
                    .filter(|uid| uid != account)
                    .peekable();
                if readers.peek().is_none() {
                    add(None, DeliveryState::Delivered);
                }
                for reader in readers {
                    add(Some(reader), DeliveryState::Seen);
                }
            } else {
                add(None, DeliveryState::Delivered);
            }
        }
    }
    if let Some(entries) = data
        .get("clearUnreads")
        .and_then(Value::as_array)
        .filter(|a| a.len() <= MAX_BATCH)
    {
        for entry in entries {
            if integer(entry.get("type")) == Some(2)
                && matches!(integer(entry.get("sct")), Some(1 | 2))
            {
                continue;
            }
            let kind = match flag(entry.get("isGroup")) {
                Some(false) => ThreadKind::Direct,
                Some(true) => ThreadKind::Group,
                _ => continue,
            };
            if let (Some(remote), Some(last_message_id)) =
                (id(entry.get("idTo")), id(entry.get("lastMsgId")))
            {
                if let Ok(target) = Target::new(kind, &remote) {
                    result.watermarks.push(ServerReadWatermark {
                        thread_id: target.thread_id(),
                        last_message_id,
                    });
                }
            }
        }
    }
    result
}

/// A mute entry can expire after the snapshot. Suppressing until the next
/// refresh is conservative and avoids trusting local wall-clock offsets.
pub(crate) fn muted_threads(payload: &Value) -> Result<HashSet<String>, WireError> {
    const MAX_MUTES: usize = 10_000;
    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .ok_or(WireError::Invalid)?;
    if !data.contains_key("chatEntries") && !data.contains_key("groupChatEntries") {
        return Err(WireError::Invalid);
    }
    let mut muted = HashSet::new();
    for (key, kind) in [
        ("chatEntries", ThreadKind::Direct),
        ("groupChatEntries", ThreadKind::Group),
    ] {
        let Some(entries) = data.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let entries = entries.as_array().ok_or(WireError::Invalid)?;
        if entries.len() > MAX_MUTES {
            return Err(WireError::Oversize);
        }
        for entry in entries {
            let remote = id(entry.get("id")).ok_or(WireError::Invalid)?;
            let duration = integer(entry.get("duration"))
                .filter(|&n| n >= -1)
                .ok_or(WireError::Invalid)?;
            let active = if duration == -1 {
                true
            } else {
                let current = integer(entry.get("currentTime"))
                    .filter(|&t| t > 0)
                    .ok_or(WireError::Invalid)?;
                let system = integer(entry.get("systemTime"))
                    .filter(|&t| t > 0)
                    .ok_or(WireError::Invalid)?;
                duration >= current.saturating_sub(system)
            };
            if active {
                muted.insert(Target::new(kind, &remote)?.thread_id());
            }
        }
    }
    Ok(muted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::messaging::incoming_messages;

    #[test]
    fn ordinary_read_references_use_source_ids_and_exact_direct_group_shapes() {
        let raw = json!({"error_code":0,"data":{
            "msgs":[{"msgId":"9007199254740993","cliMsgId":"30","msgType":"webchat","uidFrom":"22","idTo":"11","ts":4,"st":0,"content":"body"}],
            "groupMsgs":[{"msgId":"901","msgType":"webchat","uidFrom":22,"idTo":"55","ts":"6","cmd":0,"content":"body","paramsExt":{"countUnread":"0"}}]
        }});
        let parsed = incoming_messages("11", &raw).unwrap();
        let binding = parsed[0].receipt.as_ref().unwrap();
        assert!(binding.counts_unread);
        let target = Target::from_thread("d:22").unwrap();
        let parameters = seen_v2_parameters(
            &target,
            std::slice::from_ref(&binding.reference),
            "synthetic",
        )
        .unwrap();
        assert_eq!(parameters["imei"], "synthetic");
        let inner: Value = serde_json::from_str(parameters["msgInfos"].as_str().unwrap()).unwrap();
        assert_eq!(
            inner,
            json!({"senderId":"22","data":[{"cmi":"30","gmi":"9007199254740993","si":"22","di":"11","mt":"webchat","st":0,"at":-1,"cmd":-1,"ts":4}]})
        );

        let group = parsed[1].receipt.as_ref().unwrap();
        assert!(!group.counts_unread);
        let target = Target::from_thread("g:55").unwrap();
        assert!(
            seen_v2_parameters(&target, std::slice::from_ref(&group.reference), "synthetic")
                .is_err()
        );
        let mut reference = group.reference.clone();
        reference.client_id = Some("31".into());
        let parameters = seen_v2_parameters(&target, &[reference], "synthetic").unwrap();
        let inner: Value = serde_json::from_str(parameters["msgInfos"].as_str().unwrap()).unwrap();
        assert_eq!(
            inner,
            json!({"grid":"55","data":[{"cmi":"31","gmi":"901","si":"22","di":"55","mt":"webchat","st":-1,"at":-1,"cmd":0,"ts":6}]})
        );
        let mut reference = binding.reference.clone();
        reference.recipient_id = reference.sender_id.clone();
        reference.sent_at_ms = 0;
        let params = seen_v2_parameters(
            &Target::from_thread("d:22").unwrap(),
            &[reference],
            "synthetic",
        )
        .unwrap();
        let inner: Value = serde_json::from_str(params["msgInfos"].as_str().unwrap()).unwrap();
        assert_eq!(
            (
                inner["data"][0]["di"].clone(),
                inner["data"][0]["ts"].clone()
            ),
            (json!("0"), json!(-1))
        );
    }

    #[test]
    fn statuses_ignore_self_ambiguous_deliveries_and_subchat_watermarks() {
        let decoded = statuses(
            "11",
            &json!({"data":{
                "delivereds":[
                    {"msgId":"801","deliveredUids":["11"],"threadId":"22"},
                    {"msgId":"802","deliveredUids":["22","33"]},
                    {"msgId":"803","deliveredUids":["22"],"seen":0},
                    {"msgId":"804","deliveredUids":["33"],"threadId":"22","seen":1}
                ],
                "groupSeens":[{"groupId":"55","msgId":"805","seenUids":[11,22,33]}],
                "seens":[{"idTo":"22","msgId":"806","isGroup":false}],
                "clearUnreads":[
                    {"idTo":"22","lastMsgId":"806","isGroup":false},
                    {"idTo":"22","lastMsgId":"807","isGroup":false,"type":2,"sct":1},
                    {"idTo":"55","lastMsgId":"808","isGroup":true,"type":2,"sct":2},
                    {"idTo":"55","lastMsgId":"0","isGroup":true}
                ]
            }}),
        );
        assert_eq!(decoded.deliveries.len(), 4);
        assert_eq!(decoded.deliveries[0].message_id, "803");
        assert_eq!(decoded.deliveries[0].state, DeliveryState::Delivered);
        assert_eq!(decoded.deliveries[1].thread_id, "g:55");
        assert_eq!(decoded.deliveries[1].reader_id.as_deref(), Some("22"));
        assert_eq!(decoded.deliveries[2].reader_id.as_deref(), Some("33"));
        assert_eq!(decoded.deliveries[3].state, DeliveryState::Seen);
        assert_eq!(
            decoded.watermarks,
            [ServerReadWatermark {
                thread_id: "d:22".into(),
                last_message_id: "806".into()
            }]
        );
    }

    #[test]
    fn mute_policy_uses_server_seconds_and_fails_closed_on_invalid_snapshot() {
        let response = json!({"data":{"chatEntries":[
            {"id":"22","duration":"60","currentTime":1059,"systemTime":"1000"},
            {"id":"33","duration":60,"currentTime":"1061","systemTime":1000}
        ], "groupChatEntries":[{"id":55,"duration":-1}]}});
        assert_eq!(
            muted_threads(&response).unwrap(),
            HashSet::from(["d:22".into(), "g:55".into()])
        );
        assert!(muted_threads(&json!({"data":{"chatEntries": {}}})).is_err());
        assert!(
            muted_threads(&json!({"data":{"chatEntries": [{"id":"22","duration":60}]}})).is_err()
        );
    }
}
