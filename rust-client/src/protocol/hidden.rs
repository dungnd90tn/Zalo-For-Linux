//! Read-only hidden-conversation contract from the bundled PC client.
//! compact-app-pc:491392 get-all;158364 snapshot;158638/687302 PIN check.
use super::{
    directory::{DirectoryError, DirectoryRequest, DirectoryService},
    messaging::{numeric_id, Target, ThreadKind},
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const MAX_HIDDEN_THREADS: usize = 20_000;
/// Verifier remains in backend memory only, never SQLite, UI events or diagnostics.
pub(crate) struct HiddenSnapshot {
    pub threads: Vec<String>,
    verifier: Option<[u8; 16]>,
}
impl HiddenSnapshot {
    pub fn has_verifier(&self) -> bool {
        self.verifier.is_some()
    }
    pub fn verifies(&self, digest: &[u8; 16]) -> bool {
        self.verifier.as_ref().is_some_and(|expected| {
            expected
                .iter()
                .zip(digest)
                .fold(0_u8, |different, (a, b)| different | (a ^ b))
                == 0
        })
    }
    pub fn same_policy(&self, other: &Self) -> bool {
        self.threads == other.threads && self.verifier == other.verifier
    }
}
impl Drop for HiddenSnapshot {
    fn drop(&mut self) {
        if let Some(pin) = &mut self.verifier {
            pin.fill(0);
        }
    }
}
pub(crate) fn request(imei: &str) -> Result<DirectoryRequest, DirectoryError> {
    if imei.is_empty() || imei.len() > 128 || imei.chars().any(char::is_control) {
        return Err(DirectoryError::Invalid);
    }
    Ok(DirectoryRequest {
        service: DirectoryService::Conversation,
        path: "/api/hiddenconvers/get-all",
        parameters: json!({"imei":imei}),
        initial_friends: false,
    })
}
pub(crate) fn parse(value: &Value) -> Result<HiddenSnapshot, DirectoryError> {
    let items = value
        .get("threads")
        .and_then(Value::as_array)
        .ok_or(DirectoryError::Invalid)?;
    if items.len() > MAX_HIDDEN_THREADS {
        return Err(DirectoryError::Resource);
    }
    let mut threads = BTreeSet::new();
    for item in items {
        let kind = match item.get("is_group") {
            Some(Value::Bool(false)) => ThreadKind::Direct,
            Some(Value::Bool(true)) => ThreadKind::Group,
            Some(Value::Number(n)) if n.as_u64() == Some(0) => ThreadKind::Direct,
            Some(Value::Number(n)) if n.as_u64() == Some(1) => ThreadKind::Group,
            _ => return Err(DirectoryError::Invalid),
        };
        let id = match item.get("thread_id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or(DirectoryError::Invalid)?
                .to_string(),
            _ => return Err(DirectoryError::Invalid),
        };
        numeric_id(&id).map_err(|_| DirectoryError::Invalid)?;
        threads.insert(
            Target::new(kind, &id)
                .map_err(|_| DirectoryError::Invalid)?
                .thread_id(),
        );
    }
    let verifier = match value.get("pin") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.is_empty() => None,
        Some(Value::String(text))
            if text.len() == 32
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
        {
            let mut hash = [0; 16];
            for (i, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
                let digit = |b: u8| {
                    if b.is_ascii_digit() {
                        b - b'0'
                    } else {
                        b - b'a' + 10
                    }
                };
                hash[i] = digit(pair[0]) * 16 + digit(pair[1]);
            }
            Some(hash)
        }
        _ => return Err(DirectoryError::Invalid),
    };
    Ok(HiddenSnapshot {
        threads: threads.into_iter().collect(),
        verifier,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_partial_policy_is_never_an_empty_snapshot() {
        for value in [
            json!({}),
            json!({"threads":null}),
            json!({"threads":[{"is_group":2,"thread_id":"22"}]}),
            json!({"threads":[{"is_group":0,"thread_id":"0"}]}),
            json!({"threads":[],"pin":"not-a-verifier"}),
        ] {
            assert!(parse(&value).is_err());
        }
        assert!(parse(&json!({"threads":[],"pin":null}))
            .unwrap()
            .threads
            .is_empty());
    }
    #[test]
    fn scope_and_pin_match_original_pc_semantics() {
        let snapshot=parse(&json!({"threads":[{"is_group":0,"thread_id":"22"},{"is_group":1,"thread_id":"22"}],"pin":"81dc9bdb52d04dc20036dbd8313ed055"})).unwrap();
        assert_eq!(snapshot.threads, vec!["d:22", "g:22"]);
        assert!(snapshot.verifies(&crate::app::live::HiddenPin::new("1234").unwrap().digest()));
        assert!(!snapshot.verifies(&crate::app::live::HiddenPin::new("0000").unwrap().digest()));
        for invalid in [" 1234", "1234\n", "１２３４", "abcd", "123"] {
            assert!(crate::app::live::HiddenPin::new(invalid).is_none());
        }
        assert!(
            !format!("{:?}", crate::app::live::HiddenPin::new("1234").unwrap()).contains("1234")
        );
    }
    #[test]
    fn independent_original_pc_snapshot_and_pin_oracle() {
        let oracle: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/privacy/hidden.json")).unwrap();
        for case in oracle["snapshots"].as_array().unwrap() {
            let parsed =
                parse(&json!({"threads":case["threads"],"pin":oracle["synthetic_pin_digest"]}))
                    .unwrap();
            let mut expected = case["original_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    let id = id.as_str().unwrap();
                    if let Some(id) = id.strip_prefix('g') {
                        format!("g:{id}")
                    } else {
                        format!("d:{id}")
                    }
                })
                .collect::<Vec<_>>();
            expected.sort();
            assert_eq!(parsed.threads, expected);
        }
        let snapshot = parse(&json!({"threads":[],"pin":oracle["synthetic_pin_digest"]})).unwrap();
        for case in oracle["pin_cases"].as_array().unwrap() {
            let accepted = case["pin"]
                .as_str()
                .and_then(crate::app::live::HiddenPin::new)
                .is_some_and(|pin| snapshot.verifies(&pin.digest()));
            assert_eq!(accepted, case["accepted"].as_bool().unwrap());
        }
    }
}
