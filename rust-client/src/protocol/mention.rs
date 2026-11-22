//! Ordinary group mention wire payload from PC bundle N399977–399996.
use super::messaging::{id_string, Target, ThreadKind, WireError};
use crate::domain::mention::{self, Mention, ALL_ID, MAX_MENTIONS};
use serde_json::{json, Value};

pub(crate) const PATH: &str = "/api/group/mention";
const INDIVIDUAL: u64 = 0;
const ALL: u64 = 1;

pub(crate) fn parameters(
    target: &Target,
    text: &str,
    client_id: u64,
    mentions: &[Mention],
    visibility: u64,
) -> Result<Value, WireError> {
    if target.kind != ThreadKind::Group
        || visibility > 255
        || !(1..=9_007_199_254_740_991).contains(&client_id)
    {
        return Err(WireError::Unsupported);
    }
    super::messaging::numeric_id(&target.remote_id)?;
    mention::validate_outgoing(text, mentions).map_err(|_| WireError::Invalid)?;
    let mention_info = mentions
        .iter()
        .map(|mention| {
            let kind = if mention.is_all() { ALL } else { INDIVIDUAL };
            json!({
                "pos": mention.pos, "len": mention.len, "uid": mention.uid, "type": kind
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "grid":target.remote_id, "message":text, "clientId":client_id,
        "mentionInfo":Value::Array(mention_info).to_string(), "language":"VN", "visibility":visibility, "ttl":0
    }))
}

/// Malformed or unsupported metadata leaves the original plain text readable.
pub(crate) fn incoming(text: &str, raw: Option<&Value>) -> Vec<Mention> {
    let Some(entries) = raw
        .and_then(Value::as_array)
        .filter(|entries| entries.len() <= MAX_MENTIONS)
    else {
        return Vec::new();
    };
    let mentions: Option<Vec<_>> = entries
        .iter()
        .map(|entry| {
            let uid = match entry.get("type") {
                Some(kind) if kind.as_u64() == Some(ALL) => {
                    (entry["uid"].as_str() == Some(ALL_ID)).then(|| ALL_ID.to_owned())?
                }
                // Individual group mentions use type 0. Old messages may omit it.
                None => id_string(&entry["uid"]).ok()?,
                Some(kind) if kind.as_u64() == Some(INDIVIDUAL) => id_string(&entry["uid"]).ok()?,
                _ => return None,
            };
            Some(Mention {
                uid,
                pos: u32::try_from(entry["pos"].as_u64()?).ok()?,
                len: u32::try_from(entry["len"].as_u64()?).ok()?,
            })
        })
        .collect();
    let Some(mut mentions) = mentions else {
        return Vec::new();
    };
    mentions.sort_unstable_by_key(|mention| mention.pos);
    if mention::validate(text, &mentions).is_err() {
        return Vec::new();
    }
    mentions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ordinary_group_payload_retains_utf16_spans() {
        let text = "😀 @Ánh";
        let mentions = [Mention {
            uid: "22".into(),
            pos: 3,
            len: 4,
        }];
        let target = Target::new(ThreadKind::Group, "33").unwrap();
        let mut payload = parameters(&target, text, 101, &mentions, 0).unwrap();
        let raw = payload
            .as_object_mut()
            .unwrap()
            .remove("mentionInfo")
            .unwrap();
        assert_eq!(
            payload,
            json!({"grid":"33","message":text,"clientId":101,"language":"VN","visibility":0,"ttl":0})
        );
        let wire: Value = serde_json::from_str(raw.as_str().unwrap()).unwrap();
        assert_eq!(wire, json!([{"pos":3,"len":4,"uid":"22","type":0}]));
        assert_eq!(incoming(text, Some(&wire)), mentions);
        assert!(incoming(text, Some(&json!([{"pos":3,"len":4,"uid":"-1","type":1}]))).is_empty());
        assert!(parameters(&target, text, 101, &mentions, 256).is_err());
        assert!(parameters(&target, " 😀 @Ánh", 101, &mentions, 0).is_err());
    }

    #[test]
    fn broadcast_metadata_binds_explicit_type_sentinel_and_span() {
        let text = "😀 @All @Ánh";
        let mentions = [
            Mention {
                uid: ALL_ID.into(),
                pos: 3,
                len: 4,
            },
            Mention {
                uid: "22".into(),
                pos: 8,
                len: 4,
            },
        ];
        let target = Target::new(ThreadKind::Group, "33").unwrap();
        let payload = parameters(&target, text, 101, &mentions, 0).unwrap();
        let wire: Value = serde_json::from_str(payload["mentionInfo"].as_str().unwrap()).unwrap();
        assert_eq!(
            wire,
            json!([
                {"pos":3,"len":4,"uid":"-1","type":1},
                {"pos":8,"len":4,"uid":"22","type":0}
            ])
        );
        assert_eq!(incoming(text, Some(&wire)), mentions);
        for forged in [
            json!({"pos":3,"len":4,"uid":"-1"}),
            json!({"pos":3,"len":4,"uid":"-1","type":0}),
            json!({"pos":3,"len":4,"uid":"22","type":1}),
        ] {
            assert!(incoming(text, Some(&json!([forged]))).is_empty());
        }
    }
}
