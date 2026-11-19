//! Ordinary direct-message recall as used by the bundled PC client.
use super::messaging::{id_string, numeric_id, service_code, Target, ThreadKind, WireError};
use crate::domain::Message;
use serde_json::{json, Value};

pub(crate) const DIRECT_PATH: &str = "/api/message/undo";
const MAX_CLIENT_ID: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecallReference {
    pub message_id: String,
    pub client_id: String,
    pub sent_at_ms: i64,
}

impl RecallReference {
    /// The store additionally checks that this source is ordinary committed
    /// text with positive scope and no existing recall.
    pub(crate) fn from_source(account: &str, message: &Message) -> Result<Self, WireError> {
        numeric_id(account)?;
        let target = Target::from_thread(&message.thread_id)?;
        if target.kind != ThreadKind::Direct
            || target.remote_id == account
            || message.account_id != account
            || message.sender != account
            || message.quote.is_some()
            || message.sent_at_ms <= 0
            || message.reply_reference().is_none()
        {
            return Err(WireError::Unsupported);
        }
        Ok(Self {
            message_id: message.id.clone(),
            client_id: message
                .source_client_id
                .clone()
                .ok_or(WireError::Unsupported)?,
            sent_at_ms: message.sent_at_ms,
        })
    }

    pub(crate) fn within_window(&self, now_ms: i64, max_age_ms: i64) -> bool {
        self.sent_at_ms > 0
            && now_ms
                .checked_sub(self.sent_at_ms)
                .is_some_and(|age| age >= 0 && age < max_age_ms)
    }
}

pub(crate) fn direct_parameters(
    target: &Target,
    new_client_id: u64,
    original: &RecallReference,
) -> Result<Value, WireError> {
    if target.kind != ThreadKind::Direct
        || !(1..=MAX_CLIENT_ID).contains(&new_client_id)
        || new_client_id.to_string() == original.client_id
    {
        return Err(WireError::Invalid);
    }
    numeric_id(&target.remote_id)?;
    numeric_id(&original.message_id)?;
    numeric_id(&original.client_id)?;
    Ok(json!({
        "msgId": original.message_id,
        "toid": target.remote_id,
        "clientId": new_client_id,
        "cliMsgIdUndo": original.client_id,
    }))
}

/// Source derives success from the authenticated endpoint's error envelope. It
/// does not require an ID for the recall action itself.
pub(crate) fn direct_ack(payload: &Value) -> Result<(), WireError> {
    service_code(payload)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallNotice {
    pub(crate) thread_id: String,
    pub(crate) message_id: String,
    pub(crate) actor_id: String,
    pub(crate) client_id: Option<String>,
    pub(crate) event_id: String,
    pub(crate) occurred_at_ms: i64,
}

fn zero(value: &Value) -> bool {
    value.as_u64() == Some(0) || value.as_str() == Some("0")
}

fn unavailable(value: &Value) -> bool {
    value.is_null() || value.as_i64() == Some(-1) || value.as_str() == Some("-1")
}

/// Call only for the authenticated ordinary DM collection. Do not infer the
/// recalled message ID from the action envelope or a legacy client-only notice.
pub(crate) fn direct_notice(account: &str, raw: &Value) -> Result<RecallNotice, WireError> {
    numeric_id(account)?;
    if raw.get("msgType").and_then(Value::as_str) != Some("chat.undo")
        || raw.get("e2eeExt").is_some()
        || raw
            .get("isGroup")
            .is_some_and(|v| !v.is_null() && v.as_bool() != Some(false) && !zero(v))
        || ["isE2EE", "isE2ee", "e2ee"].iter().any(|key| {
            raw.get(key)
                .is_some_and(|v| !v.is_null() && v.as_bool() != Some(false) && !zero(v))
        })
    {
        return Err(WireError::Unsupported);
    }
    let account_or_id = |value: &Value| {
        if zero(value) {
            Ok(account.to_owned())
        } else {
            id_string(value)
        }
    };
    let actor_id = account_or_id(raw.get("uidFrom").ok_or(WireError::Invalid)?)?;
    let recipient = account_or_id(raw.get("idTo").ok_or(WireError::Invalid)?)?;
    let peer = match (actor_id.as_str() == account, recipient.as_str() == account) {
        (true, false) => recipient.as_str(),
        (false, true) => actor_id.as_str(),
        _ => return Err(WireError::Invalid),
    };
    let thread_id = Target::new(ThreadKind::Direct, peer)?.thread_id();
    let content = raw
        .get("content")
        .and_then(Value::as_object)
        .ok_or(WireError::Invalid)?;
    if let Some(source) = content.get("srcId").filter(|value| !unavailable(value)) {
        if account_or_id(source)? != actor_id {
            return Err(WireError::Invalid);
        }
    }
    if let Some(destination) = content
        .get("destId")
        .filter(|value| !unavailable(value) && !zero(value))
    {
        // A source build may name the direct conversation ID or the recipient.
        // The envelope must independently agree with the bound account above.
        let destination = id_string(destination)?;
        if destination != peer && destination != recipient {
            return Err(WireError::Invalid);
        }
    }
    let message_id = id_string(content.get("globalMsgId").ok_or(WireError::Invalid)?)?;
    let client_id = content
        .get("cliMsgId")
        .filter(|value| !value.is_null() && !zero(value))
        .map(id_string)
        .transpose()?;
    let event_id = id_string(raw.get("msgId").ok_or(WireError::Invalid)?)?;
    if event_id == message_id {
        return Err(WireError::Invalid);
    }
    let occurred_at_ms = raw
        .get("ts")
        .and_then(|time| time.as_i64().or_else(|| time.as_str()?.parse().ok()))
        .filter(|time| *time > 0)
        .ok_or(WireError::Invalid)?;
    Ok(RecallNotice {
        thread_id,
        message_id,
        actor_id,
        client_id,
        event_id,
        occurred_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::bootstrap::DEFAULT_RECALL_MAX_AGE_MS as DEFAULT_MAX_AGE_MS;

    #[test]
    fn original_oracle_matches_direct_recall_builder() {
        let oracle: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/recall.json"))
                .unwrap();
        for fixture in oracle["direct"].as_array().unwrap() {
            let target =
                Target::new(ThreadKind::Direct, fixture["peer"].as_str().unwrap()).unwrap();
            let original = RecallReference {
                message_id: fixture["msgId"].as_str().unwrap().into(),
                client_id: fixture["cliMsgIdUndo"].as_str().unwrap().into(),
                sent_at_ms: 1000,
            };
            assert_eq!(
                direct_parameters(&target, fixture["clientId"].as_u64().unwrap(), &original)
                    .unwrap(),
                fixture["request"]["payload"]
            );
            assert_eq!(fixture["request"]["path"], DIRECT_PATH);
            assert_eq!(direct_ack(&json!({"error_code":0,"data":null})), Ok(()));
            assert_eq!(
                direct_ack(&json!({"error_code":211})),
                Err(WireError::Service(211))
            );
            assert!(original.within_window(1000, DEFAULT_MAX_AGE_MS));
            assert!(original.within_window(300_999, DEFAULT_MAX_AGE_MS));
            assert!(!original.within_window(301_000, DEFAULT_MAX_AGE_MS));
            assert!(!original.within_window(999, DEFAULT_MAX_AGE_MS));
        }
    }

    #[test]
    fn authenticated_direct_recall_distinguishes_original_and_event_ids() {
        let mut raw = json!({
            "msgType":"chat.undo", "msgId":"990",
            "uidFrom":"0","idTo":"22","ts":1500,
            "content":{"globalMsgId":777, "cliMsgId":"800", "srcId":"11","destId":"22"}
        });
        assert_eq!(
            direct_notice("11", &raw).unwrap(),
            RecallNotice {
                thread_id: "d:22".into(),
                actor_id: "11".into(),
                message_id: "777".into(),
                client_id: Some("800".into()),
                event_id: "990".into(),
                occurred_at_ms: 1500,
            }
        );
        raw["uidFrom"] = json!(22);
        raw["idTo"] = json!(0);
        raw["content"]["srcId"] = json!("22");
        raw["content"]["destId"] = json!("11");
        assert_eq!(direct_notice("11", &raw).unwrap().actor_id, "22");
        for (pointer, value) in [
            ("/content/globalMsgId", json!(0)),
            ("/content/globalMsgId", json!("990")),
            ("/idTo", json!("44")),
            ("/content/srcId", json!("55")),
            ("/content/destId", json!("55")),
        ] {
            let mut misbound = raw.clone();
            *misbound.pointer_mut(pointer).unwrap() = value;
            assert_eq!(direct_notice("11", &misbound), Err(WireError::Invalid));
        }
        raw["e2eeExt"] = json!({});
        assert_eq!(direct_notice("11", &raw), Err(WireError::Unsupported));
    }
}
