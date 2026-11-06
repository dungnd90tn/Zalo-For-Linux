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
        if self.path == "/api/social/profile/me-v2" {
            // Source getOSCode() returns8 for navigator.platform Linux (688250).
            url.query_pairs_mut().append_pair("os", "8");
        }
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
/// Original getProfileMeV2 (bundle491758), consumed as data.profile (332565).
pub fn self_profile_request(imei: &str) -> Result<DirectoryRequest, DirectoryError> {
    device(imei)?;
    Ok(DirectoryRequest {
        service: DirectoryService::Profile,
        path: "/api/social/profile/me-v2",
        parameters: json!({"avatar_size":120,"imei":imei}),
        initial_friends: false,
    })
}
pub fn parse_self_profile(data: &Value, account: &str) -> Result<DirectoryEntry, DirectoryError> {
    numeric_id(account).map_err(|_| DirectoryError::Invalid)?;
    let profile = data.get("profile").ok_or(DirectoryError::Invalid)?;
    let remote_id = id(profile.get("userId").ok_or(DirectoryError::Invalid)?)?;
    if remote_id != account {
        return Err(DirectoryError::Invalid);
    }
    Ok(DirectoryEntry {
        kind: ThreadKind::Direct,
        remote_id,
        display_name: name(profile, "displayName")?,
        avatar_url: avatar(profile.get("avatar")),
        is_friend: None,
        is_blocked: None,
        member_count: None,
    })
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
    avatar_url(value?.as_str()?)
}
pub fn avatar_url(text: &str) -> Option<String> {
    if text.len() > 2048 || text.trim() != text || text.chars().any(char::is_control) {
        return None;
    }
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
    fn source_self_profile_is_bound_to_authenticated_user_and_linux_contract() {
        let request = self_profile_request("native-fixture").unwrap();
        assert_eq!(
            request.parameters,
            json!({"avatar_size":120,"imei":"native-fixture"})
        );
        let address = request
            .encrypted_url(
                &Url::parse("https://wpa.zaloapp.com/").unwrap(),
                "synthetic",
                ClientType::PcLinux24,
            )
            .unwrap();
        assert_eq!(address.path(), "/api/social/profile/me-v2");
        assert!(address
            .query_pairs()
            .any(|(key, value)| key == "os" && value == "8"));
        let value = json!({"profile":{"userId":"11","displayName":"Tài khoản tổng hợp","avatar":"https://s120.avatar.talk.zdn.vn/self.png"},"biz":{"unknown":"retain"}});
        let profile = parse_self_profile(&value, "11").unwrap();
        assert_eq!(profile.thread_id(), "d:11");
        assert!(profile.avatar_url.is_some());
        assert!(parse_self_profile(&value, "22").is_err());
        assert!(parse_self_profile(&value["profile"], "11").is_err());
    }
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
    pub message_info: Vec<crate::domain::MessageInfoBinding>,
    pub message_media: Vec<crate::domain::MessageMediaBinding>,
    pub unsupported_encrypted: usize,
}
/// Closed HTTP/parser locations only; never stores account IDs or payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewField {
    ResponseJson,
    ResponseEnvelope,
    EncryptedData,
    DecryptedJson,
    DecryptedEnvelope,
    MessageCollections,
    Msgs,
    GroupMsgs,
    PageMsgs,
    EeMsgs,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewDiagnostic {
    pub reason: super::messaging::FrameReason,
    pub field: PreviewField,
    pub field_type: super::messaging::FrameValueType,
    pub field_bytes: Option<usize>,
    pub message_kind: Option<super::messaging::FrameMessageKind>,
}
impl PreviewDiagnostic {
    pub(crate) fn new(
        reason: super::messaging::FrameReason,
        field: PreviewField,
        value: Option<&Value>,
    ) -> Self {
        Self {
            reason,
            field,
            field_type: super::messaging::FrameValueType::of(value),
            field_bytes: None,
            message_kind: None,
        }
    }
    fn from_frame(failure: super::messaging::FrameFailure, field: PreviewField) -> Self {
        Self {
            reason: failure.reason,
            field,
            field_type: failure.context.field_type.unwrap_or_default(),
            field_bytes: failure.context.field_bytes,
            message_kind: failure.context.message_kind,
        }
    }
    fn failure(self, error: DirectoryError) -> PreviewFailure {
        PreviewFailure {
            error,
            diagnostic: self,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewFailure {
    pub error: DirectoryError,
    pub diagnostic: PreviewDiagnostic,
}
/// Untrusted/offline callers retain explicit recipient checks. HTTP callers must
/// supply the verified session capability, never a caller-chosen account ID.
pub fn parse_previews(account: &str, data: &Value) -> Result<PreviewPage, DirectoryError> {
    parse_previews_detailed(account, data).map_err(|failure| failure.error)
}
pub fn parse_previews_detailed(account: &str, data: &Value) -> Result<PreviewPage, PreviewFailure> {
    parse_previews_inner(account, data, None)
}
pub fn parse_verified_previews(
    session: &super::session::VerifiedSession,
    data: &Value,
) -> Result<PreviewPage, PreviewFailure> {
    parse_previews_inner(session.account_id(), data, Some(session))
}
/// Preload is a read-only preview request, not a Signal queue ACK. Reuse the
/// existing legacy codec and keep unsupported encrypted data explicitly pending.
fn parse_previews_inner(
    account: &str,
    data: &Value,
    session: Option<&super::session::VerifiedSession>,
) -> Result<PreviewPage, PreviewFailure> {
    use super::messaging::{FrameReason, FrameValueType};
    let mut diagnostic = PreviewDiagnostic::new(
        FrameReason::BatchEnvelope,
        PreviewField::MessageCollections,
        Some(data),
    );
    numeric_id(account).map_err(|_| diagnostic.failure(DirectoryError::Invalid))?;
    if !data.is_object() {
        return Err(diagnostic.failure(DirectoryError::Invalid));
    }
    let mut messages: Vec<crate::domain::Message> = Vec::new();
    let mut message_info: Vec<crate::domain::MessageInfoBinding> = Vec::new();
    let mut message_media: Vec<crate::domain::MessageMediaBinding> = Vec::new();
    let mut media_indices = std::collections::HashMap::new();
    let mut info_indices = std::collections::HashMap::new();
    let mut info_bytes = 0usize;
    let mut unsupported_encrypted = 0usize;
    let mut seen = std::collections::HashMap::new();
    for field in ["msgs", "groupMsgs", "pageMsgs"] {
        let Some(array) = data.get(field) else {
            continue;
        };
        // getLastMessagesForPreview delegates to preProcessChatMsgs, whose
        // optional collection gates are `field && field.length > 0`.
        if array.is_null() {
            continue;
        }
        diagnostic = PreviewDiagnostic::new(
            FrameReason::BatchEnvelope,
            match field {
                "msgs" => PreviewField::Msgs,
                "groupMsgs" => PreviewField::GroupMsgs,
                _ => PreviewField::PageMsgs,
            },
            Some(array),
        );
        let array = array
            .as_array()
            .ok_or_else(|| diagnostic.failure(DirectoryError::Invalid))?;
        if array.len() > MAX_FRIENDS {
            diagnostic.reason = FrameReason::BatchLimit;
            return Err(diagnostic.failure(DirectoryError::Resource));
        }
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
            let decoded = match session {
                Some(session) => super::messaging::incoming_verified_preview(session, &payload),
                None => super::messaging::incoming_messages_detailed(account, &payload),
            }
            .map_err(|failure| PreviewFailure {
                error: if failure.error == super::messaging::WireError::Oversize {
                    DirectoryError::Resource
                } else {
                    DirectoryError::Invalid
                },
                diagnostic: PreviewDiagnostic::from_frame(failure, diagnostic.field),
            })?;
            for incoming in decoded {
                diagnostic.reason = FrameReason::MessageValidation;
                diagnostic.message_kind = Some(if incoming.info.is_some() {
                    super::messaging::FrameMessageKind::WebchatInfo
                } else if let Some(media) = &incoming.media {
                    match media.kind() {
                        crate::domain::MediaKind::Photo => {
                            super::messaging::FrameMessageKind::Photo
                        }
                        crate::domain::MediaKind::WebContent => {
                            super::messaging::FrameMessageKind::WebContent
                        }
                        crate::domain::MediaKind::Recommended => {
                            super::messaging::FrameMessageKind::Recommended
                        }
                        crate::domain::MediaKind::Poll => super::messaging::FrameMessageKind::Poll,
                        crate::domain::MediaKind::File => super::messaging::FrameMessageKind::File,
                    }
                } else if incoming.message.sticker.is_some() {
                    super::messaging::FrameMessageKind::Sticker
                } else if incoming.unsupported {
                    super::messaging::FrameMessageKind::Other
                } else {
                    super::messaging::FrameMessageKind::Webchat
                });
                let key = (
                    incoming.message.thread_id.clone(),
                    incoming.message.id.clone(),
                );
                if let Some(previous) = seen.get(&key) {
                    let previous_info = info_indices
                        .get(&key)
                        .map(|index: &usize| &message_info[*index].content);
                    let previous_media = media_indices
                        .get(&key)
                        .map(|index: &usize| &message_media[*index].content);
                    if messages[*previous] != incoming.message
                        || previous_info != incoming.info.as_ref()
                        || previous_media != incoming.media.as_ref()
                    {
                        return Err(diagnostic.failure(DirectoryError::Invalid));
                    }
                    continue;
                }
                if let Some(content) = incoming.info {
                    info_bytes =
                        info_bytes
                            .checked_add(content.as_json().len())
                            .ok_or_else(|| {
                                let mut d = diagnostic;
                                d.reason = FrameReason::BatchLimit;
                                d.failure(DirectoryError::Resource)
                            })?;
                    // Bound the complete response, not each individual field or
                    // 200-message codec chunk, before emitting any sink batch.
                    if info_bytes > crate::domain::MAX_INFO_BATCH_BYTES {
                        diagnostic.reason = FrameReason::BatchLimit;
                        return Err(diagnostic.failure(DirectoryError::Resource));
                    }
                    info_indices.insert(key.clone(), message_info.len());
                    message_info.push(crate::domain::MessageInfoBinding {
                        account_id: incoming.message.account_id.clone(),
                        thread_id: incoming.message.thread_id.clone(),
                        message_id: incoming.message.id.clone(),
                        content,
                    });
                }
                if let Some(content) = incoming.media {
                    info_bytes =
                        info_bytes
                            .checked_add(content.as_json().len())
                            .ok_or_else(|| {
                                let mut d = diagnostic;
                                d.reason = FrameReason::BatchLimit;
                                d.failure(DirectoryError::Resource)
                            })?;
                    if info_bytes > crate::domain::MAX_INFO_BATCH_BYTES {
                        diagnostic.reason = FrameReason::BatchLimit;
                        return Err(diagnostic.failure(DirectoryError::Resource));
                    }
                    media_indices.insert(key.clone(), message_media.len());
                    message_media.push(crate::domain::MessageMediaBinding {
                        account_id: incoming.message.account_id.clone(),
                        thread_id: incoming.message.thread_id.clone(),
                        message_id: incoming.message.id.clone(),
                        content,
                    });
                }
                seen.insert(key, messages.len());
                messages.push(incoming.message);
                if messages.len() > MAX_FRIENDS {
                    diagnostic.reason = FrameReason::BatchLimit;
                    return Err(diagnostic.failure(DirectoryError::Resource));
                }
            }
        }
    }
    if let Some(array) = data.get("eeMsgs") {
        diagnostic = PreviewDiagnostic::new(
            FrameReason::BatchEnvelope,
            PreviewField::EeMsgs,
            Some(array),
        );
        if !array.is_null() {
            let array = array
                .as_array()
                .ok_or_else(|| diagnostic.failure(DirectoryError::Invalid))?;
            unsupported_encrypted = unsupported_encrypted
                .checked_add(array.len())
                .ok_or_else(|| diagnostic.failure(DirectoryError::Resource))?;
        }
    }
    if unsupported_encrypted > MAX_FRIENDS {
        diagnostic.reason = FrameReason::BatchLimit;
        diagnostic.field_type = FrameValueType::Array;
        return Err(diagnostic.failure(DirectoryError::Resource));
    }
    Ok(PreviewPage {
        messages,
        message_info,
        message_media,
        unsupported_encrypted,
    })
}

#[cfg(test)]
mod preview_tests {
    #[test]
    fn recommended_and_poll_previews_keep_scoped_complete_sidecars() {
        use serde_json::{json, Value};
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/rich_cards.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let message = json!({"uidFrom":"22","idTo":"11","msgId":"815","cliMsgId":"991","ts":7,"msgType":case["kind"],"content":case["content"]});
            let page = super::parse_previews(
                "11",
                &json!({"msgs":[message.clone()],"groupMsgs":[message.clone()]}),
            )
            .unwrap();
            assert_eq!(page.messages.len(), 2);
            assert_eq!(page.message_media.len(), 2);
            for (binding, message) in page.message_media.iter().zip(&page.messages) {
                assert_eq!(binding.account_id, "11");
                assert_eq!(binding.thread_id, message.thread_id);
                assert_eq!(binding.message_id, message.id);
                assert_eq!(
                    serde_json::from_str::<Value>(binding.content.as_json()).unwrap(),
                    case["content"]
                );
                assert_eq!(message.media.as_ref(), Some(binding.content.descriptor()));
            }
            assert_eq!(page.messages[0].thread_id, "d:22");
            assert_eq!(page.messages[1].thread_id, "g:11");
            let duplicate =
                super::parse_previews("11", &json!({"msgs":[message.clone(),message.clone()]}))
                    .unwrap();
            assert_eq!(duplicate.messages.len(), 1);
            assert_eq!(duplicate.message_media.len(), 1);
            let mut conflict = message.clone();
            conflict["content"]["unknownSourceField"] = json!(true);
            assert!(super::parse_previews("11", &json!({"msgs":[message,conflict]})).is_err());
        }
    }

    #[test]
    fn verified_http_previews_match_original_caller_and_peer_routing() {
        use crate::{
            profile::Profile,
            protocol::{bootstrap::ServerConfig, session::synthetic_session},
        };
        let scratch = std::env::temp_dir().join(format!(
            "zalo-preview-parser-{}",
            crate::domain::local_message_id().unwrap()
        ));
        let profile = std::sync::Arc::new(Profile::at_root(&scratch, "synthetic").unwrap());
        let session = synthetic_session(
            profile.clone(),
            ServerConfig::parse(&serde_json::json!({})).unwrap(),
        );
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/directory/previews.json"))
                .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let page = super::parse_verified_previews(&session, &case["data"]).unwrap();
            let actual: Vec<_> = page
                .messages
                .iter()
                .map(|message| message.thread_id.as_str())
                .collect();
            let expected: Vec<_> = case["threads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|thread| thread.as_str().unwrap())
                .collect();
            assert_eq!(actual, expected, "{}", case["name"]);
            assert!(page
                .messages
                .iter()
                .all(|message| message.account_id == "11"));
            for media in page.message_media {
                assert!(page
                    .messages
                    .iter()
                    .any(|message| message.account_id == media.account_id
                        && message.thread_id == media.thread_id
                        && message.id == media.message_id
                        && message.media.as_ref() == Some(media.content.descriptor())));
            }
        }
        let peer = &fixture["cases"][3]["data"];
        let strict = super::parse_previews_detailed("11", peer).err().unwrap();
        assert_eq!(
            strict.diagnostic.reason,
            crate::protocol::messaging::FrameReason::MessageScope
        );
        assert_eq!(
            strict.diagnostic.message_kind,
            Some(crate::protocol::messaging::FrameMessageKind::Photo)
        );
        drop(session);
        drop(profile);
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn preview_rejections_keep_closed_collection_field_type_and_message_kind() {
        use super::*;
        use crate::protocol::messaging::{FrameMessageKind, FrameReason, FrameValueType};
        let malformed = parse_previews_detailed("11", &serde_json::json!({"pageMsgs":{}}))
            .err()
            .unwrap();
        assert_eq!(malformed.error, DirectoryError::Invalid);
        assert_eq!(
            (
                malformed.diagnostic.field,
                malformed.diagnostic.reason,
                malformed.diagnostic.field_type
            ),
            (
                PreviewField::PageMsgs,
                FrameReason::BatchEnvelope,
                FrameValueType::Object
            )
        );
        let row = serde_json::json!({"msgType":"chat.photo","uidFrom":"22","idTo":"11","msgId":"841","ts":1700000000000_i64,"content":null});
        let malformed = parse_previews_detailed("11", &serde_json::json!({"groupMsgs":[row]}))
            .err()
            .unwrap();
        assert_eq!(
            (
                malformed.diagnostic.field,
                malformed.diagnostic.reason,
                malformed.diagnostic.field_type,
                malformed.diagnostic.message_kind
            ),
            (
                PreviewField::GroupMsgs,
                FrameReason::MessageContent,
                FrameValueType::Null,
                Some(FrameMessageKind::Photo)
            )
        );
        for value in [
            serde_json::Value::Null,
            serde_json::json!([]),
            serde_json::json!(false),
        ] {
            assert_eq!(
                parse_previews_detailed("11", &value)
                    .err()
                    .unwrap()
                    .diagnostic
                    .field,
                PreviewField::MessageCollections
            );
        }
    }
    #[test]
    fn source_optional_null_collections_are_empty_but_wrong_types_fail() {
        let empty = super::parse_previews(
            "11",
            &serde_json::json!({"msgs":null,"groupMsgs":null,"pageMsgs":null,"eeMsgs":null}),
        )
        .unwrap();
        assert!(
            empty.messages.is_empty()
                && empty.message_info.is_empty()
                && empty.message_media.is_empty()
        );
        for field in ["msgs", "groupMsgs", "pageMsgs", "eeMsgs"] {
            for invalid in [
                serde_json::json!({}),
                serde_json::json!(false),
                serde_json::json!(""),
            ] {
                assert!(super::parse_previews("11", &serde_json::json!({field:invalid})).is_err());
            }
        }
    }
    use super::*;
    #[test]
    fn info_previews_preserve_metadata_and_reject_metadata_only_duplicate_conflicts() {
        let content = json!({"action":"msginfo.actionlist","title":"Thông báo nhóm",
            "params":json!({"msg":{"vi":"Đã cập nhật nhóm"},"unknown":[1,{"future":true}]}).to_string()});
        let message = json!({"msgId":"501","msgType":"webchat","uidFrom":"22","idTo":"11",
            "ts":1700000000000_i64,"content":content});
        let page = parse_previews(
            "11",
            &json!({"msgs":[message.clone()],"pageMsgs":[message.clone()]}),
        )
        .unwrap();
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.message_info.len(), 1);
        let info = &page.message_info[0];
        assert_eq!(
            (
                info.account_id.as_str(),
                info.thread_id.as_str(),
                info.message_id.as_str()
            ),
            ("11", "d:22", "501")
        );
        assert_eq!(
            serde_json::from_str::<Value>(info.content.as_json()).unwrap(),
            content
        );
        let mut conflict = message.clone();
        conflict["content"]["future"] = json!({"changed":true});
        assert!(matches!(
            parse_previews("11", &json!({"msgs":[message],"pageMsgs":[conflict]})),
            Err(DirectoryError::Invalid)
        ));
    }

    #[test]
    fn info_preview_budget_covers_fields_and_codec_chunks_before_any_sink_batch() {
        let content = json!({"action":"msginfo.actionlist","title":"Thông báo",
            "future":"x".repeat(crate::domain::MAX_INFO_BATCH_BYTES / 2)});
        let first = json!({"msgId":"501","msgType":"webchat","uidFrom":"22","idTo":"11",
            "ts":1700000000000_i64,"content":content});
        let mut second = first.clone();
        second["msgId"] = json!("502");
        assert!(matches!(
            parse_previews(
                "11",
                &json!({"msgs":[first.clone()],"pageMsgs":[second.clone()]})
            ),
            Err(DirectoryError::Resource)
        ));
        let mut chunked = vec![first];
        for index in 1..DIRECTORY_BATCH_SIZE {
            chunked.push(json!({"msgId":(1000+index).to_string(),"msgType":"webchat","uidFrom":"22","idTo":"11",
                "ts":1700000000000_i64,"content":"plain"}));
        }
        chunked.push(second);
        assert!(matches!(
            parse_previews("11", &json!({"msgs":chunked})),
            Err(DirectoryError::Resource)
        ));
    }
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
        assert!(parse_previews("11", &json!({"unknown":[]}))
            .unwrap()
            .messages
            .is_empty());
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
