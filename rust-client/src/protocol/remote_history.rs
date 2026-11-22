//! Authenticated NEW_TO_OLD pages for ordinary group history.
use super::{
    messaging::{self as wire, IncomingBatch, Target, ThreadKind, WireError},
    session::VerifiedSession,
};
use crate::domain::Cursor;
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) const SERVICE: &str = "group_cloud_message";
pub(crate) const LATEST_SERVICE: &str = "group_cloud_latest";
const PAGE_SIZE: usize = 50;
// The PC newest-page MAX_MSG_ID (N398225) passes through JavaScript parseInt (N302120).
const LATEST_BOUND: u64 = 10_000_000_000_000_000;
const RECENT: &str = "/api/cm/getrecentv2";
const OLD: &str = "/api/cm/getoldv2";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RemoteCursor {
    pub id: u64,
    pub old: bool,
}

impl RemoteCursor {
    pub fn newest() -> Self {
        Self {
            id: LATEST_BOUND,
            old: false,
        }
    }

    pub fn from_anchor(anchor: &Cursor) -> Result<Self, WireError> {
        if anchor.sent_at_ms <= 0 {
            return Err(WireError::Invalid);
        }
        Ok(Self {
            id: exact_id(&anchor.id)?,
            old: false,
        })
    }

    pub fn path(self) -> &'static str {
        if self.old {
            OLD
        } else {
            RECENT
        }
    }

    pub fn parameters(self, target: &Target, imei: &str) -> Result<Value, WireError> {
        self.parameters_with_source(target, imei, 3)
    }

    pub fn newest_parameters(target: &Target, imei: &str) -> Result<Value, WireError> {
        Self::newest().parameters_with_source(target, imei, 1)
    }

    fn parameters_with_source(
        self,
        target: &Target,
        imei: &str,
        source: u8,
    ) -> Result<Value, WireError> {
        if target.kind != ThreadKind::Group || self.id == 0 || imei.is_empty() {
            return Err(WireError::Invalid);
        }
        Ok(json!({"groupId":target.remote_id,"globalMsgId":self.id,
            "count":PAGE_SIZE,"msgIds":[],"imei":imei,"src":source}))
    }
}

pub(crate) struct Page {
    pub batch: IncomingBatch,
    pub next: Option<RemoteCursor>,
}

fn exact_id(id: &str) -> Result<u64, WireError> {
    wire::numeric_id(id)?;
    id.parse::<u64>().map_err(|_| WireError::Invalid)
}

fn flag(value: &Value) -> Result<bool, WireError> {
    match value {
        Value::Bool(value) => Ok(*value),
        value if value.as_u64() == Some(0) => Ok(false),
        value if value.as_u64() == Some(1) => Ok(true),
        _ => Err(WireError::Invalid),
    }
}

fn zero_or_unset(value: &Value) -> bool {
    value.is_null()
        || value.as_u64() == Some(0)
        || value.as_str() == Some("0")
        || value.as_bool() == Some(false)
}

fn ordinary(value: &Value) -> bool {
    ["e2ee", "isE2EE", "isE2ee", "e2eeExt", "e2eeStatus"]
        .iter()
        .all(|key| value.get(key).is_none_or(zero_or_unset))
}

fn check_scope(value: &Value, group_id: &str) -> Result<(), WireError> {
    if !value.is_object() {
        return Err(WireError::Invalid);
    }
    for key in ["groupId", "grid", "idTo", "uidTo", "toUid"] {
        if let Some(id) = value.get(key).filter(|value| !value.is_null()) {
            let destination = match id.as_str().and_then(|value| value.strip_prefix('g')) {
                Some(id) => {
                    wire::numeric_id(id)?;
                    id.to_owned()
                }
                None => wire::id_string(id)?,
            };
            if destination != group_id {
                return Err(WireError::Invalid);
            }
        }
    }
    Ok(())
}

fn sender(row: &Value, account: &str) -> Result<String, WireError> {
    let canonical = |value: &Value| {
        if value.as_str() == Some("0") || value.as_u64() == Some(0) {
            return Ok(account.to_owned());
        }
        wire::id_string(value)
    };
    let direct = row
        .get("uidFrom")
        .filter(|value| !value.is_null())
        .map(canonical)
        .transpose()?;
    let alias = row
        .get("fromUid")
        .filter(|value| !value.is_null())
        .map(canonical)
        .transpose()?;
    if direct
        .as_ref()
        .zip(alias.as_ref())
        .is_some_and(|(first, second)| first != second)
    {
        return Err(WireError::Invalid);
    }
    direct.or(alias).ok_or(WireError::Invalid)
}

pub(crate) fn page(
    session: &VerifiedSession,
    payload: &Value,
    target: &Target,
    anchor: &Cursor,
    requested: RemoteCursor,
) -> Result<Page, WireError> {
    let (rows, next) = parse_page(session.account_id(), payload, target, anchor, requested)?;
    Ok(Page {
        batch: decode_rows(session, rows)?,
        next,
    })
}

pub(crate) fn newest_anchor() -> Cursor {
    Cursor {
        id: LATEST_BOUND.to_string(),
        sent_at_ms: LATEST_BOUND as i64,
    }
}

pub(crate) fn newest_page(
    session: &VerifiedSession,
    payload: &Value,
    target: &Target,
) -> Result<IncomingBatch, WireError> {
    let (rows, _) = parse_page(
        session.account_id(),
        payload,
        target,
        &newest_anchor(),
        RemoteCursor::newest(),
    )?;
    decode_rows(session, rows)
}

fn decode_rows(session: &VerifiedSession, rows: Vec<Value>) -> Result<IncomingBatch, WireError> {
    let batch = wire::incoming_verified_preview(
        session,
        &json!({
            "error_code":0,"data":{"groupMsgs":rows}
        }),
    )
    .map_err(|failure| failure.error)?;
    Ok(batch)
}

fn parse_page(
    account: &str,
    payload: &Value,
    target: &Target,
    anchor: &Cursor,
    requested: RemoteCursor,
) -> Result<(Vec<Value>, Option<RemoteCursor>), WireError> {
    if target.kind != ThreadKind::Group
        || requested.id == 0
        || requested.id > RemoteCursor::from_anchor(anchor)?.id
    {
        return Err(WireError::Invalid);
    }
    wire::service_code(payload)?;
    let data: Value = serde_json::from_str(
        payload
            .get("data")
            .and_then(Value::as_str)
            .ok_or(WireError::Invalid)?,
    )
    .map_err(|_| WireError::Invalid)?;
    check_scope(&data, &target.remote_id)?;
    if !ordinary(&data)
        || data
            .get("isFiltered")
            .is_some_and(|value| !zero_or_unset(value))
        || ["msgs", "pageMsgs", "deletedMsgs"].iter().any(|key| {
            data.get(key).is_some_and(|value| {
                !value.is_null() && !value.as_array().is_some_and(Vec::is_empty)
            })
        })
    {
        return Err(WireError::Unsupported);
    }
    let rows = data
        .get("groupMsgs")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    if rows.len() > PAGE_SIZE {
        return Err(WireError::Oversize);
    }
    let old = flag(data.get("isOld").ok_or(WireError::Invalid)?)?;
    let more = flag(data.get("hasMore").ok_or(WireError::Invalid)?)?;
    if requested.old && !old {
        return Err(WireError::Invalid);
    }
    let cutoff = match data.get("lastDeletedMsgID") {
        None => 0,
        Some(value) if zero_or_unset(value) => 0,
        Some(value) => exact_id(&wire::id_string(value)?)?,
    };
    let mut smallest = requested.id;
    let mut seen = HashSet::new();
    let mut supported = Vec::new();
    for row in rows {
        check_scope(row, &target.remote_id)?;
        let id = wire::id_string(row.get("msgId").ok_or(WireError::Invalid)?)?;
        let numeric = exact_id(&id)?;
        if numeric >= requested.id || !seen.insert(numeric) {
            return Err(WireError::Invalid);
        }
        smallest = smallest.min(numeric);
        let kind = row
            .get("msgType")
            .and_then(Value::as_str)
            .ok_or(WireError::Invalid)?;
        if matches!(kind, "chat.undo" | "chat.delete") || kind.starts_with("chat.delete.") {
            return Err(WireError::Unsupported);
        }
        if numeric <= cutoff
            || !ordinary(row)
            || row.get("ttl").is_some_and(|value| !zero_or_unset(value))
        {
            continue;
        }
        if !plain_text(kind, row) {
            continue;
        }
        let stamp = row
            .get("ts")
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
            .filter(|stamp| *stamp > 0)
            .ok_or(WireError::Invalid)?;
        if (stamp, id.as_str()) >= (anchor.sent_at_ms, anchor.id.as_str()) {
            return Err(WireError::Invalid);
        }
        let mut scoped = row.clone();
        // The cloud transform binds the destination to its requested group.
        // An explicit conflicting envelope destination was rejected above.
        scoped["idTo"] = Value::String(target.remote_id.clone());
        scoped["uidFrom"] = Value::String(sender(row, account)?);
        supported.push(scoped);
    }
    let last = data
        .get("lastMsgId")
        .filter(|value| !zero_or_unset(value))
        .map(|value| exact_id(&wire::id_string(value)?))
        .transpose()?;
    if last.is_some_and(|id| id > smallest) {
        return Err(WireError::Invalid);
    }
    let next = if requested == RemoteCursor::newest() {
        None
    } else if rows.is_empty() && !requested.old && old {
        // Empty recent→archive changes the endpoint and keeps the same start.
        (requested.id > cutoff).then_some(RemoteCursor {
            id: requested.id,
            old,
        })
    } else if more {
        let id = last
            .filter(|id| *id < requested.id)
            .ok_or(WireError::Invalid)?;
        (id > cutoff).then_some(RemoteCursor { id, old })
    } else {
        // A populated terminal response that flips endpoints does not prove
        // the archive has been exhausted.
        if old != requested.old {
            return Err(WireError::Invalid);
        }
        None
    };
    Ok((supported, next))
}

fn plain_text(kind: &str, row: &Value) -> bool {
    kind == "webchat"
        && row
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|text| {
                !text.trim().is_empty() && text.len() <= crate::domain::MAX_TEXT_BYTES
            })
        && ["quote", "reference"]
            .iter()
            .all(|key| row.get(key).is_none_or(Value::is_null))
        && row.get("refMessageId").is_none_or(zero_or_unset)
        && row
            .get("mentions")
            .is_none_or(|value| value.is_null() || value.as_array().is_some_and(Vec::is_empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrapped(data: Value) -> Value {
        json!({"error_code":0,"data":data.to_string()})
    }

    #[test]
    fn remote_group_pages_bind_groups_senders_and_exact_numeric_cursors() {
        let target = Target::new(ThreadKind::Group, "777").unwrap();
        let anchor = Cursor {
            id: "9007199254740993".into(),
            sent_at_ms: 10,
        };
        let cursor = RemoteCursor::from_anchor(&anchor).unwrap();
        assert_eq!(
            cursor.parameters(&target, "fixture").unwrap()["globalMsgId"],
            json!(9_007_199_254_740_993_u64)
        );
        let row = json!({"msgId":"9007199254740992","ts":9,"msgType":"webchat",
            "content":"Tin tổng hợp","uidFrom":0,"fromUid":"11","toUid":"g777"});
        let response = |rows| {
            wrapped(json!({"groupId":"777","hasMore":true,"isOld":false,
            "lastMsgId":"9007199254740992","groupMsgs":rows}))
        };
        let (rows, next) = parse_page(
            "11",
            &response(json!([row.clone()])),
            &target,
            &anchor,
            cursor,
        )
        .unwrap();
        assert_eq!(rows[0]["uidFrom"], "11");
        assert_eq!(rows[0]["idTo"], "777");
        assert_eq!(
            next,
            Some(RemoteCursor {
                id: 9_007_199_254_740_992,
                old: false
            })
        );
        for (key, wrong) in [
            ("toUid", json!("g778")),
            ("fromUid", json!("22")),
            ("msgId", json!("9007199254740993")),
            ("ts", json!(11)),
        ] {
            let mut wrong_row = row.clone();
            wrong_row[key] = wrong;
            assert_eq!(
                parse_page(
                    "11",
                    &response(json!([wrong_row])),
                    &target,
                    &anchor,
                    cursor
                ),
                Err(WireError::Invalid)
            );
        }
        assert_eq!(
            parse_page("22", &response(json!([row])), &target, &anchor, cursor),
            Err(WireError::Invalid)
        );
    }

    #[test]
    fn remote_group_page_switches_phase_once_and_rejects_stall_or_delete_control() {
        let target = Target::new(ThreadKind::Group, "777").unwrap();
        let anchor = Cursor {
            id: "100".into(),
            sent_at_ms: 10,
        };
        let cursor = RemoteCursor::from_anchor(&anchor).unwrap();
        let response = |old, last, rows| {
            wrapped(json!({"hasMore":true,"isOld":old,
            "lastMsgId":last,"groupMsgs":rows}))
        };
        let empty = response(true, 50, json!([]));
        let archive = RemoteCursor { id: 100, old: true };
        assert_eq!(
            parse_page("11", &empty, &target, &anchor, cursor),
            Ok((vec![], Some(archive)))
        );
        let stall = response(true, 100, json!([]));
        assert_eq!(
            parse_page("11", &stall, &target, &anchor, archive),
            Err(WireError::Invalid)
        );
        for kind in ["chat.undo", "chat.delete", "chat.delete.everyone"] {
            let data = response(false, 50, json!([{"msgId":51,"msgType":kind}]));
            assert_eq!(
                parse_page("11", &data, &target, &anchor, cursor),
                Err(WireError::Unsupported)
            );
        }
    }
}
