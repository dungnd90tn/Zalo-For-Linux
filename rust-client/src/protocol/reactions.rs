//! Ordinary reaction requests and authenticated notices from the bundled PC client.
use super::messaging::{id_string, numeric_id, Target, ThreadKind, WireError};
use serde_json::{json, Value};

const MAX_JS_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_NOTICE_TARGETS: usize = 32;
pub(crate) const MAX_NOTICES: usize = 200;
const TEXT: i64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReactionKind {
    Heart,
    Like,
    Laugh,
    Wow,
    Sad,
    Angry,
}

impl ReactionKind {
    pub(crate) fn wire(self) -> (i64, &'static str) {
        match self {
            Self::Heart => (5, "/-heart"),
            Self::Like => (3, "/-strong"),
            Self::Laugh => (0, ":>"),
            Self::Wow => (32, ":o"),
            Self::Sad => (2, ":-(("),
            Self::Angry => (20, ":-h"),
        }
    }

    pub(crate) fn from_code(code: i64) -> Option<Self> {
        [
            Self::Heart,
            Self::Like,
            Self::Laugh,
            Self::Wow,
            Self::Sad,
            Self::Angry,
        ]
        .into_iter()
        .find(|kind| kind.wire().0 == code)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReactionAction {
    Add(ReactionKind),
    RemoveAll,
}

impl ReactionAction {
    fn wire(self) -> (i64, &'static str) {
        match self {
            Self::Add(kind) => kind.wire(),
            Self::RemoveAll => (-1, ""),
        }
    }

    pub(crate) fn code(self) -> i64 {
        self.wire().0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReactionSummary {
    pub kind: ReactionKind,
    pub count: u32,
    pub mine: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReactionOutcome {
    Accepted,
    Rejected,
    Unavailable,
    Unknown,
}

fn safe_id(id: &str) -> Result<u64, WireError> {
    numeric_id(id)?;
    id.parse::<u64>()
        .ok()
        .filter(|value| *value <= MAX_JS_INTEGER)
        .ok_or(WireError::Unsupported)
}

pub(crate) fn reference_supported(global_id: &str, client_id: &str) -> bool {
    safe_id(global_id).is_ok() && safe_id(client_id).is_ok()
}

/// N311439–311486 sends the source identifiers as numbers, and N399872 wraps
/// the inner serialized message in a one-entry react_list.
pub(crate) fn parameters(
    target: &Target,
    message_id: &str,
    source_client_id: &str,
    client_id: u64,
    imei: &str,
    action: ReactionAction,
) -> Result<(&'static str, Value), WireError> {
    numeric_id(&target.remote_id)?;
    if !(1..=MAX_JS_INTEGER).contains(&client_id)
        || client_id.to_string() == source_client_id
        || imei.is_empty()
        || imei.len() > 256
    {
        return Err(WireError::Invalid);
    }
    let (code, icon) = action.wire();
    let message = json!({
        "rMsg": [{"gMsgID": safe_id(message_id)?, "cMsgID": safe_id(source_client_id)?, "msgType": TEXT}],
        "rIcon": icon,
        "rType": code,
        "source": 6
    })
    .to_string();
    let mut parameters = json!({"react_list": [{"message": message, "clientId": client_id}]});
    let path = match target.kind {
        ThreadKind::Direct => {
            parameters["toid"] = json!(target.remote_id);
            "/api/message/reaction"
        }
        ThreadKind::Group => {
            parameters["grid"] = json!(target.remote_id);
            parameters["imei"] = json!(imei);
            "/api/group/reaction"
        }
    };
    Ok((path, parameters))
}

pub(crate) struct ReactionNotice {
    pub thread: String,
    pub message_id: String,
    pub source_client_id: String,
    pub event_id: String,
    pub actor: String,
    pub client_id: Option<String>,
    pub timestamp: i64,
    pub code: i64,
}

impl std::fmt::Debug for ReactionNotice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReactionNotice")
            .finish_non_exhaustive()
    }
}

fn zero(value: &Value) -> bool {
    value.as_u64() == Some(0) || value.as_str() == Some("0")
}

pub(crate) fn parse_notice(
    account: &str,
    kind: ThreadKind,
    raw: &Value,
    embedded: bool,
) -> Result<Vec<ReactionNotice>, WireError> {
    numeric_id(account)?;
    if raw["msgType"].as_str() != Some("chat.reaction")
        || raw.get("e2eeExt").is_some()
        || ["isE2EE", "isE2ee", "e2ee"].iter().any(|key| {
            raw.get(key)
                .is_some_and(|v| !v.is_null() && v.as_bool() != Some(false) && !zero(v))
        })
    {
        return Err(WireError::Unsupported);
    }
    let from = &raw["uidFrom"];
    let actor = if zero(from) {
        account.into()
    } else {
        id_string(from)?
    };
    let to = if embedded {
        &raw["msgKey"]
    } else {
        &raw["idTo"]
    };
    let embedded_group = to
        .as_str()
        .and_then(|key| key.strip_prefix('g'))
        .filter(|_| embedded);
    let kind = if embedded_group.is_some() {
        ThreadKind::Group
    } else {
        kind
    };
    let peer = match kind {
        ThreadKind::Group => match embedded_group {
            Some(group) => group.into(),
            None => id_string(to)?,
        },
        ThreadKind::Direct if actor == account => id_string(to)?,
        ThreadKind::Direct if zero(to) => actor.clone(),
        ThreadKind::Direct => {
            let to = id_string(to)?;
            if to != account && to != actor {
                return Err(WireError::Invalid);
            }
            actor.clone()
        }
    };
    if kind == ThreadKind::Direct && peer == account {
        return Err(WireError::Invalid);
    }
    let thread = Target::new(kind, &peer)?.thread_id();
    let event_id = id_string(&raw["msgId"])?;
    let client_id = raw
        .get("cliMsgId")
        .filter(|value| !value.is_null() && !zero(value))
        .map(id_string)
        .transpose()?;
    let timestamp = raw["ts"]
        .as_i64()
        .or_else(|| raw["ts"].as_str()?.parse().ok())
        .filter(|timestamp| *timestamp > 0 && *timestamp <= MAX_JS_INTEGER as i64)
        .ok_or(WireError::Invalid)?;
    // The original long-poll chat fallback nests the same opaque JSON string
    // under content.params and names the destination with msgKey (N311367).
    let content = if embedded {
        &raw["content"]["params"]
    } else {
        &raw["content"]
    }
    .as_str()
    .filter(|content| content.len() <= 8 * 1024)
    .ok_or(WireError::Invalid)?;
    let content: Value = serde_json::from_str(content).map_err(|_| WireError::Invalid)?;
    let code = content["rType"].as_i64().ok_or(WireError::Invalid)?;
    let icon = content["rIcon"].as_str().ok_or(WireError::Invalid)?;
    if code == -1 {
        if !icon.is_empty() {
            return Err(WireError::Invalid);
        }
    } else if !(0..=4096).contains(&code) || icon.is_empty() || icon.len() > 64 {
        return Err(WireError::Invalid);
    }
    // Unknown positives are retained so an explicit removal can still remove
    // an actor's whole set. Presentation only displays known preset codes.
    let targets = content["rMsg"]
        .as_array()
        .filter(|targets| targets.len() <= MAX_NOTICE_TARGETS)
        .ok_or(WireError::Oversize)?;
    let mut notices = Vec::<ReactionNotice>::new();
    for target in targets {
        if target["msgType"].as_i64() != Some(TEXT) {
            continue;
        }
        let (Ok(message_id), Ok(source_client_id)) =
            (id_string(&target["gMsgID"]), id_string(&target["cMsgID"]))
        else {
            continue;
        };
        if message_id == event_id || notices.iter().any(|notice| notice.message_id == message_id) {
            continue;
        }
        notices.push(ReactionNotice {
            thread: thread.clone(),
            message_id,
            source_client_id,
            event_id: event_id.clone(),
            actor: actor.clone(),
            client_id: client_id.clone(),
            timestamp,
            code,
        });
    }
    Ok(notices)
}

pub(crate) fn control_notices(account: &str, frame: &Value) -> Vec<ReactionNotice> {
    if super::messaging::service_code(frame).is_err() {
        return Vec::new();
    }
    let mut notices = Vec::new();
    for (field, kind) in [
        ("reacts", ThreadKind::Direct),
        ("reactGroups", ThreadKind::Group),
    ] {
        let Some(rows) = frame["data"][field]
            .as_array()
            .filter(|rows| rows.len() <= MAX_NOTICES)
        else {
            continue;
        };
        for row in rows {
            if let Ok(parsed) = parse_notice(account, kind, row, false) {
                notices.extend(parsed.into_iter().take(MAX_NOTICES - notices.len()));
            }
        }
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::bootstrap::ServerConfig;

    #[test]
    fn reaction_route_is_optional_and_an_authenticated_disable_removes_it() {
        let mut server = json!({
            "zpw_service_map_v3":{"reaction":["https://wpa.zaloapp.com"]},
            "settings":{"features":{"reaction_setting":{"off_message_reaction":0}}}
        });
        assert!(ServerConfig::parse(&server)
            .unwrap()
            .services
            .contains_key("reaction"));
        server["settings"]["features"]["reaction_setting"]["off_message_reaction"] = json!(1);
        assert!(!ServerConfig::parse(&server)
            .unwrap()
            .services
            .contains_key("reaction"));
        server["settings"]["features"]["reaction_setting"]["off_message_reaction"] = json!(false);
        server["zpw_service_map_v3"]["reaction"] = json!(["https://not-zalo.invalid"]);
        assert!(!ServerConfig::parse(&server)
            .unwrap()
            .services
            .contains_key("reaction"));
    }

    #[test]
    fn single_batch_matches_source_fields_and_source_safe_integer_bound() {
        let direct = Target::new(ThreadKind::Direct, "22").unwrap();
        let group = Target::new(ThreadKind::Group, "23").unwrap();
        let add = ReactionAction::Add(ReactionKind::Heart);
        let (path, body) = parameters(&direct, "999", "77", 1000, "device", add).unwrap();
        assert_eq!(path, "/api/message/reaction");
        assert_eq!(body["toid"], "22");
        assert!(body.get("imei").is_none());
        assert_eq!(body["react_list"][0]["clientId"], 1000);
        assert_eq!(
            serde_json::from_str::<Value>(body["react_list"][0]["message"].as_str().unwrap())
                .unwrap(),
            json!({"rMsg":[{"gMsgID":999,"cMsgID":77,"msgType":1}],"source":6,"rIcon":"/-heart","rType":5}),
        );
        let (path, body) = parameters(
            &group,
            "999",
            "77",
            1000,
            "device",
            ReactionAction::RemoveAll,
        )
        .unwrap();
        assert_eq!(path, "/api/group/reaction");
        assert_eq!(body["grid"], "23");
        assert_eq!(body["imei"], "device");
        let inner =
            serde_json::from_str::<Value>(body["react_list"][0]["message"].as_str().unwrap())
                .unwrap();
        assert_eq!(inner["rType"], -1);
        assert_eq!(inner["rIcon"], "");
        assert!(parameters(&direct, "9007199254740992", "77", 1000, "device", add).is_err());
    }

    #[test]
    fn scoped_fallback_and_control_reject_foreign_and_invalid_actors() {
        let content =
            json!({"rMsg":[{"gMsgID":999,"cMsgID":77,"msgType":1}],"rIcon":"/-strong","rType":3});
        let mut raw = json!({"msgType":"chat.reaction","msgId":"1000","ts":123,"uidFrom":"22","idTo":"0",
            "content":content.to_string()});
        let parsed = parse_notice("11", ThreadKind::Direct, &raw, false).unwrap();
        assert_eq!(parsed[0].thread, "d:22");
        raw["idTo"] = json!("33");
        assert!(parse_notice("11", ThreadKind::Direct, &raw, false).is_err());
        let mut embedded = json!({"msgType":"chat.reaction","msgId":"1001","ts":"123","uidFrom":"0","msgKey":"22",
            "content":{"params":content.to_string()}});
        let parsed = parse_notice("11", ThreadKind::Direct, &embedded, true).unwrap();
        assert_eq!(parsed[0].actor, "11");
        embedded["msgKey"] = json!("g23");
        let parsed = parse_notice("11", ThreadKind::Direct, &embedded, true).unwrap();
        assert_eq!(parsed[0].thread, "g:23");
        embedded["e2eeExt"] = json!({});
        assert!(parse_notice("11", ThreadKind::Direct, &embedded, true).is_err());
    }
}
