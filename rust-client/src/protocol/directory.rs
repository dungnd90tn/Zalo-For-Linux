//! Original directory contracts from the local renderer/worker bundle.
//! No request is sent here; callers supply an authenticated, isolated transport.
use super::client::ClientType;
use super::messaging::{numeric_id, validate_origin, ThreadKind};
use serde_json::{json, Value};
use std::collections::HashSet;
use url::Url;

pub const MAX_DIRECTORY_BODY: usize = 16 * 1024 * 1024;
pub const MAX_FRIENDS: usize = 20_000;
pub const GROUP_PAGE_SIZE: u32 = 100;
pub const MAX_GROUP_PAGES: u32 = 99;
pub const DIRECTORY_BATCH_SIZE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryError {
    Invalid,
    Resource,
    Unsupported,
    Service(i64),
}
impl std::fmt::Display for DirectoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => f.write_str("Directory response has an invalid schema"),
            Self::Resource => f.write_str("Directory synchronization exceeded its resource limit"),
            Self::Unsupported => f.write_str("Directory service is not available"),
            Self::Service(code) => write!(f, "Directory server returned code {code}"),
        }
    }
}
impl std::error::Error for DirectoryError {}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryService {
    Profile,
    Group,
    Conversation,
}
impl DirectoryService {
    pub fn name(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Group => "group",
            Self::Conversation => "conversation",
        }
    }
}
/// Contains no session keys/cookies. Parameter encryption belongs to VerifiedSession.
pub struct DirectoryRequest {
    pub service: DirectoryService,
    pub path: &'static str,
    pub parameters: Value,
    pub initial_friends: bool,
}
impl DirectoryRequest {
    pub fn encrypted_url(
        &self,
        root: &Url,
        encrypted: &str,
        client_type: ClientType,
    ) -> Result<Url, DirectoryError> {
        validate_origin(root, "https").map_err(|_| DirectoryError::Invalid)?;
        if encrypted.is_empty() || encrypted.len() > 16 * 1024 {
            return Err(DirectoryError::Resource);
        }
        let mut url = root.clone();
        url.set_path(self.path);
        url.query_pairs_mut()
            .append_pair("zpw_ver", "681")
            .append_pair("zpw_type", client_type.api_type())
            .append_pair("params", encrypted);
        if self.initial_friends {
            url.query_pairs_mut().append_pair("init", "1");
        }
        Ok(url)
    }
}
fn device(imei: &str) -> Result<(), DirectoryError> {
    if imei.is_empty() || imei.len() > 128 || imei.chars().any(char::is_control) {
        return Err(DirectoryError::Invalid);
    }
    Ok(())
}
/// Bundle N398578–398591 requests one bounded full friend list, not guessed paging.
pub fn friends_request(imei: &str) -> Result<DirectoryRequest, DirectoryError> {
    device(imei)?;
    Ok(DirectoryRequest {
        service: DirectoryService::Profile,
        path: "/api/social/friend/getfriends",
        parameters: json!({"incInvalid":1,"page":1,"count":20_000,"avatar_size":120,"actiontime":0,"imei":imei}),
        initial_friends: true,
    })
}
/// Bundle N398630–398642; manager N437914 bounds traversal to pages below100.
pub fn groups_request(imei: &str, page: u32) -> Result<DirectoryRequest, DirectoryError> {
    device(imei)?;
    if !(1..=MAX_GROUP_PAGES).contains(&page) {
        return Err(DirectoryError::Resource);
    }
    Ok(DirectoryRequest {
        service: DirectoryService::Group,
        path: "/api/group/getlg/v3",
        parameters: json!({"page":page,"count":GROUP_PAGE_SIZE,"avatar_size":120,"member_avatar_size":120,"imei":imei}),
        initial_friends: false,
    })
}
/// Bundle N402175–402181; initial cache is empty, encoded as the string "{}".
pub fn previews_request(imei: &str) -> Result<DirectoryRequest, DirectoryError> {
    device(imei)?;
    Ok(DirectoryRequest {
        service: DirectoryService::Conversation,
        path: "/api/preloadconvers/get-last-msgs",
        parameters: json!({"threadIdLocalMsgId":"{}","imei":imei}),
        initial_friends: false,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub kind: ThreadKind,
    pub remote_id: String,
    pub display_name: String,
    /// Metadata only: never confers cookie or automatic media-fetch authority.
    pub avatar_url: Option<String>,
    pub is_friend: Option<bool>,
    pub is_blocked: Option<bool>,
    pub member_count: Option<u32>,
}
impl DirectoryEntry {
    pub fn thread_id(&self) -> String {
        format!(
            "{}:{}",
            if self.kind == ThreadKind::Direct {
                "d"
            } else {
                "g"
            },
            self.remote_id
        )
    }
}
pub struct GroupPage {
    pub entries: Vec<DirectoryEntry>,
    pub next_page: Option<u32>,
}
fn id(value: &Value) -> Result<String, DirectoryError> {
    let text = if let Some(text) = value.as_str() {
        text.to_owned()
    } else if let Some(number) = value.as_u64() {
        number.to_string()
    } else {
        return Err(DirectoryError::Invalid);
    };
    numeric_id(&text).map_err(|_| DirectoryError::Invalid)?;
    Ok(text)
}
fn name(value: &Value, field: &str) -> Result<String, DirectoryError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(DirectoryError::Invalid)
}
fn flag(value: Option<&Value>, default: bool) -> Result<bool, DirectoryError> {
    match value {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::Number(value)) if value.as_u64() == Some(0) => Ok(false),
        Some(Value::Number(value)) if value.as_u64() == Some(1) => Ok(true),
        _ => Err(DirectoryError::Invalid),
    }
}
fn avatar(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str().filter(|s| s.len() <= 2048)?;
    let url = Url::parse(text).ok()?;
    let host = url.host_str()?;
    let allowed = ["zalo.me", "zaloapp.com", "zdn.vn", "zadn.vn"]
        .iter()
        .any(|suffix| {
            host == *suffix || host.strip_suffix(suffix).is_some_and(|s| s.ends_with('.'))
        });
    (url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && allowed)
        .then(|| url.into())
}
pub struct FriendPage {
    pub entries: Vec<DirectoryEntry>,
    /// Valid identity/authority flags but unsupported presentation fields.
    pub unsupported_names: usize,
}
/// Strict codec retained for consumers requiring a complete profile projection.
pub fn parse_friends(data: &Value) -> Result<Vec<DirectoryEntry>, DirectoryError> {
    let page = parse_friends_partial(data)?;
    if page.unsupported_names != 0 {
        return Err(DirectoryError::Unsupported);
    }
    Ok(page.entries)
}
/// Missing/deleted presentation names have no verified fallback in this bundle.
/// Preserve their existing cache and report partial, while rejecting invalid IDs,
/// duplicate identities or malformed authority flags for the entire response.
pub fn parse_friends_partial(data: &Value) -> Result<FriendPage, DirectoryError> {
    let array = data.as_array().ok_or(DirectoryError::Invalid)?;
    if array.len() > MAX_FRIENDS {
        return Err(DirectoryError::Resource);
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::with_capacity(array.len());
    let mut unsupported_names = 0;
    for item in array {
        let remote_id = id(item.get("userId").ok_or(DirectoryError::Invalid)?)?;
        if !seen.insert(remote_id.clone()) {
            return Err(DirectoryError::Invalid);
        }
        let is_friend = item.get("isFr").map(|v| flag(Some(v), false)).transpose()?;
        let is_blocked = item
            .get("isBlocked")
            .map(|v| flag(Some(v), false))
            .transpose()?;
        let Ok(display_name) = name(item, "displayName") else {
            unsupported_names += 1;
            continue;
        };
        entries.push(DirectoryEntry {
            kind: ThreadKind::Direct,
            remote_id,
            display_name,
            avatar_url: avatar(item.get("avatar")),
            is_friend,
            is_blocked,
            member_count: None,
        });
    }
    Ok(FriendPage {
        entries,
        unsupported_names,
    })
}
/// Bundle group normalization N437849 maps groupId/name/avt to conversation fields.
pub fn parse_groups(data: &Value, page: u32) -> Result<GroupPage, DirectoryError> {
    if !(1..=MAX_GROUP_PAGES).contains(&page) {
        return Err(DirectoryError::Resource);
    }
    let array = data
        .get("groups")
        .and_then(Value::as_array)
        .filter(|array| array.len() <= GROUP_PAGE_SIZE as usize)
        .ok_or(DirectoryError::Resource)?;
    let more = flag(data.get("hasMore"), false)?;
    if more && (array.is_empty() || page == MAX_GROUP_PAGES) {
        return Err(DirectoryError::Resource);
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::with_capacity(array.len());
    for item in array {
        let remote_id = id(item.get("groupId").ok_or(DirectoryError::Invalid)?)?;
        if !seen.insert(remote_id.clone()) {
            return Err(DirectoryError::Invalid);
        }
        let member_count = item
            .get("totalMember")
            .map(|value| {
                value
                    .as_u64()
                    .filter(|count| *count <= 100_000)
                    .map(|count| count as u32)
                    .ok_or(DirectoryError::Invalid)
            })
            .transpose()?;
        entries.push(DirectoryEntry {
            kind: ThreadKind::Group,
            remote_id,
            display_name: name(item, "name")?,
            avatar_url: avatar(item.get("avt")),
            is_friend: None,
            is_blocked: None,
            member_count,
        });
    }
    Ok(GroupPage {
        entries,
        next_page: more.then_some(page + 1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_wire_requests_and_profile_normalization_are_bounded() {
        let friends = friends_request("synthetic-device").unwrap();
        assert_eq!(friends.parameters["count"], 20_000);
        assert_eq!(friends.parameters["page"], 1);
        assert_eq!(
            previews_request("synthetic-device").unwrap().parameters["threadIdLocalMsgId"],
            "{}"
        );
        assert!(groups_request("synthetic-device", 100).is_err());
        let entries = parse_friends(&json!([{"userId":"12","displayName":"Nguyễn An","isFr":1,"avatar":"https://s120.avatar.talk.zdn.vn/synthetic.png"}])).unwrap();
        assert_eq!(entries[0].thread_id(), "d:12");
        let groups = parse_groups(&json!({"groups":[{"groupId":"12","name":"Nhóm thật","avt":"http://untrusted.invalid/avatar","totalMember":3}],"hasMore":1}),1).unwrap();
        assert_eq!(groups.entries[0].thread_id(), "g:12");
        assert!(groups.entries[0].avatar_url.is_none());
        assert_eq!(groups.next_page, Some(2));
        assert!(parse_groups(&json!({"groups":[],"hasMore":1}), 1).is_err());
    }
    #[test]
    fn unsupported_friend_name_is_partial_without_hiding_bad_identity_or_authority() {
        let supported = json!({"userId":"12","displayName":"Valid","isFr":1});
        let deleted = json!({"userId":"13","displayName":"","isBlocked":1});
        let page = parse_friends_partial(&json!([supported.clone(), deleted.clone()])).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.unsupported_names, 1);
        assert_eq!(page.entries[0].is_blocked, None);
        let mut bad = deleted.clone();
        bad["userId"] = json!("invalid");
        assert!(parse_friends_partial(&json!([supported.clone(), bad])).is_err());
        let mut bad = deleted.clone();
        bad["isBlocked"] = json!("unknown");
        assert!(parse_friends_partial(&json!([supported, bad])).is_err());
        assert!(parse_friends_partial(&json!([deleted.clone(), deleted])).is_err());
    }

    #[test]
    fn duplicate_ids_and_url_authority_confusion_are_rejected() {
        let contact = json!({"userId":"12","displayName":"An"});
        assert!(parse_friends(&json!([contact.clone(), contact])).is_err());
        assert!(avatar(Some(&json!(
            "https://avatar.talk.zdn.vn.attacker.invalid/a"
        )))
        .is_none());
        assert!(avatar(Some(&json!("https://127.0.0.1/a"))).is_none());
        assert!(friends_request("device")
            .unwrap()
            .encrypted_url(
                &Url::parse("https://profile.zalo.me/").unwrap(),
                "synthetic-cipher",
                ClientType::PcLinux24
            )
            .is_ok());
        assert!(friends_request("device")
            .unwrap()
            .encrypted_url(
                &Url::parse("https://profile.zalo.me.attacker.invalid/").unwrap(),
                "synthetic-cipher",
                ClientType::PcLinux24
            )
            .is_err());
    }
}

pub struct PreviewPage {
    pub messages: Vec<crate::domain::Message>,
    pub unsupported_encrypted: usize,
}
/// Preload is a read-only preview request, not a Signal queue ACK. Reuse the
/// existing legacy codec and keep unsupported encrypted data explicitly pending.
pub fn parse_previews(account: &str, data: &Value) -> Result<PreviewPage, DirectoryError> {
    numeric_id(account).map_err(|_| DirectoryError::Invalid)?;
    let mut messages = Vec::new();
    let mut unsupported_encrypted = 0;
    let mut recognized = false;
    let mut seen = std::collections::HashMap::new();
    for field in ["msgs", "groupMsgs", "pageMsgs"] {
        let Some(array) = data.get(field) else {
            continue;
        };
        recognized = true;
        let array = array
            .as_array()
            .filter(|a| a.len() <= MAX_FRIENDS)
            .ok_or(DirectoryError::Resource)?;
        for chunk in array.chunks(DIRECTORY_BATCH_SIZE) {
            let mut legacy = Vec::with_capacity(chunk.len());
            for message in chunk {
                let encrypted = ["isE2EE", "isE2ee", "e2ee"].iter().any(|key| {
                    message.get(key).is_some_and(|v| {
                        !matches!(v, Value::Null | Value::Bool(false)) && v.as_u64() != Some(0)
                    })
                });
                if encrypted {
                    unsupported_encrypted += 1;
                } else {
                    legacy.push(message.clone());
                }
            }
            let mut batch = serde_json::Map::new();
            batch.insert(field.into(), Value::Array(legacy));
            let payload = json!({"error_code":0,"data":batch});
            let decoded = super::messaging::incoming_messages(account, &payload).map_err(
                |error| match error {
                    super::messaging::WireError::Oversize => DirectoryError::Resource,
                    _ => DirectoryError::Invalid,
                },
            )?;
            for incoming in decoded {
                let key = (
                    incoming.message.thread_id.clone(),
                    incoming.message.id.clone(),
                );
                if let Some(previous) = seen.get(&key) {
                    if previous != &incoming.message {
                        return Err(DirectoryError::Invalid);
                    }
                    continue;
                }
                seen.insert(key, incoming.message.clone());
                messages.push(incoming.message);
                if messages.len() > MAX_FRIENDS {
                    return Err(DirectoryError::Resource);
                }
            }
        }
    }
    if let Some(array) = data.get("eeMsgs") {
        recognized = true;
        unsupported_encrypted += array
            .as_array()
            .filter(|a| a.len() <= MAX_FRIENDS)
            .ok_or(DirectoryError::Resource)?
            .len();
    }
    if !recognized || unsupported_encrypted > MAX_FRIENDS {
        return Err(DirectoryError::Invalid);
    }
    Ok(PreviewPage {
        messages,
        unsupported_encrypted,
    })
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    fn previews_use_real_ids_timestamps_dedup_and_never_accept_unrelated_or_encrypted_content() {
        let mut records = Vec::new();
        for index in 1..=205 {
            records.push(json!({"msgId":(1000+index).to_string(),"msgType":"webchat","uidFrom":"12","idTo":"11","ts":1700000000000_i64+index,"content":format!("synthetic{index}")}));
        }
        records.push(records[0].clone());
        let mut encrypted = records[0].clone();
        encrypted["msgId"] = json!(9999);
        encrypted["isE2EE"] = json!(true);
        encrypted["content"] = json!("must never render ciphertext");
        records.push(encrypted);
        let result = parse_previews(
            "11",
            &json!({"msgs":records,"eeMsgs":[{"encrypted":"pending"}]}),
        )
        .unwrap();
        assert_eq!(result.messages.len(), 205);
        assert_eq!(result.unsupported_encrypted, 2);
        assert_eq!(result.messages[0].thread_id, "d:12");
        assert_eq!(result.messages[0].sent_at_ms, 1700000000001);
        assert!(!result
            .messages
            .iter()
            .any(|m| m.text.contains("ciphertext")));
        assert!(parse_previews("22",&json!({"msgs":[{"msgId":"1","msgType":"webchat","uidFrom":"12","idTo":"11","ts":1,"content":"other account"}]})).is_err());
        assert!(parse_previews("11", &json!({"unknown":[]})).is_err());
        let group=parse_previews("11",&json!({"groupMsgs":[{"msgId":"1","msgType":"webchat","uidFrom":"12","idTo":"12","ts":1,"content":"group"}]})).unwrap();
        assert_eq!(group.messages[0].thread_id, "g:12");
    }
    #[test]
    fn conflicting_preview_ids_across_buckets_are_rejected() {
        let raw = json!({"msgId":"501","msgType":"webchat","uidFrom":"12","idTo":"11","ts":1700000000000_i64,"content":"original"});
        let mut conflict = raw.clone();
        conflict["content"] = json!("conflict");
        assert!(parse_previews("11", &json!({"msgs":[raw],"pageMsgs":[conflict]})).is_err());
    }
}
