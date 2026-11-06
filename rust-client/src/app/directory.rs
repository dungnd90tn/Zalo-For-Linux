//! Cancellable directory discovery. Network I/O and SQLite writes have distinct owners.
use crate::{
    protocol::{directory as wire, session::VerifiedSession},
    store::directory::{DirectoryBatch, DirectoryRecord},
};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

const MAX_BODY: usize = wire::MAX_DIRECTORY_BODY;
const TOTAL_DEADLINE: Duration = Duration::from_secs(90);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryStage {
    Account,
    Friends,
    Groups,
    Previews,
    Storage,
    Synchronization,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryCategory {
    Unavailable,
    Network,
    Timeout,
    Http,
    Server,
    Crypto,
    Schema,
    Resource,
    Cancelled,
    Storage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryFailure {
    pub stage: DirectoryStage,
    pub category: DirectoryCategory,
    pub http_status: Option<u16>,
    pub server_code: Option<i64>,
    pub parser: Option<wire::PreviewDiagnostic>,
}
impl std::fmt::Display for DirectoryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Directory {:?}/{:?}", self.stage, self.category)?;
        if let Some(status) = self.http_status {
            write!(f, " HTTP {status}")?;
        }
        if let Some(code) = self.server_code {
            write!(f, " code {code}")?;
        }
        Ok(())
    }
}
impl std::error::Error for DirectoryFailure {}
fn failure(stage: DirectoryStage, category: DirectoryCategory) -> DirectoryFailure {
    DirectoryFailure {
        stage,
        category,
        http_status: None,
        server_code: None,
        parser: None,
    }
}
fn schema_failure(
    stage: DirectoryStage,
    field: wire::PreviewField,
    reason: crate::protocol::messaging::FrameReason,
    value: Option<&Value>,
) -> DirectoryFailure {
    let mut error = failure(stage, DirectoryCategory::Schema);
    error.parser = Some(wire::PreviewDiagnostic::new(reason, field, value));
    error
}
fn preview_failure(error: wire::PreviewFailure) -> DirectoryFailure {
    let mut failure = wire_failure(DirectoryStage::Previews, error.error);
    failure.parser = Some(error.diagnostic);
    failure
}
fn wire_failure(stage: DirectoryStage, error: wire::DirectoryError) -> DirectoryFailure {
    match error {
        wire::DirectoryError::Resource => failure(stage, DirectoryCategory::Resource),
        wire::DirectoryError::Unsupported => failure(stage, DirectoryCategory::Unavailable),
        wire::DirectoryError::Service(code) => DirectoryFailure {
            stage,
            category: DirectoryCategory::Server,
            http_status: None,
            server_code: Some(code),
            parser: None,
        },
        wire::DirectoryError::Invalid => schema_failure(
            stage,
            wire::PreviewField::MessageCollections,
            crate::protocol::messaging::FrameReason::BatchEnvelope,
            None,
        ),
    }
}
/// The runtime implements this using its existing bounded Database actor. A sink
/// must not publish metadata before its transaction commits. No second writer.
pub trait DirectorySink: Send {
    fn apply(
        &mut self,
        batch: DirectoryBatch,
    ) -> impl Future<Output = Result<(), DirectoryFailure>> + Send;
}
#[derive(Debug, Clone, Default)]
pub struct DirectoryReport {
    pub completed_profile: bool,
    pub contacts: usize,
    pub unsupported_contact_names: usize,
    pub groups: usize,
    pub preview_messages: usize,
    pub unsupported_encrypted: usize,
    /// Partial metadata never implies absent contacts/history or permission changes.
    pub completed_friends: bool,
    pub completed_groups: bool,
    pub completed_previews: bool,
    pub failures: Vec<DirectoryFailure>,
}
pub struct DirectoryFetcher {
    client: Client,
}
impl DirectoryFetcher {
    pub(crate) fn from_client(client: Client) -> Self {
        Self { client }
    }
    pub fn new() -> Result<Self, DirectoryFailure> {
        let client = Client::builder()
            .no_proxy()
            .https_only(true)
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(1)
            .user_agent("ZaloPC-linux-24v681")
            .build()
            .map_err(|_| failure(DirectoryStage::Friends, DirectoryCategory::Network))?;
        Ok(Self { client })
    }
    #[cfg(test)]
    pub(crate) fn with_client(client: Client) -> Self {
        Self { client }
    }
    pub(crate) async fn fetch(
        &self,
        session: &VerifiedSession,
        request: wire::DirectoryRequest,
        stage: DirectoryStage,
    ) -> Result<Value, DirectoryFailure> {
        let root = session
            .config()
            .services
            .get(request.service.name())
            .and_then(|list| list.first())
            .ok_or_else(|| failure(stage, DirectoryCategory::Unavailable))?;
        let plaintext = serde_json::to_vec(&request.parameters).map_err(|_| {
            schema_failure(
                stage,
                wire::PreviewField::EncryptedData,
                crate::protocol::messaging::FrameReason::OuterData,
                Some(&request.parameters),
            )
        })?;
        let encrypted = session
            .encrypt_payload(&plaintext)
            .map_err(|_| failure(stage, DirectoryCategory::Crypto))?;
        let url = request
            .encrypted_url(root, &encrypted, session.client_type())
            .map_err(|e| wire_failure(stage, e))?;
        let cookie = session
            .authorization_cookie(&url)
            .ok_or_else(|| failure(stage, DirectoryCategory::Unavailable))?;
        let mut response = self
            .client
            .get(url)
            .header(reqwest::header::COOKIE, cookie)
            .send()
            .await
            .map_err(|error| {
                failure(
                    stage,
                    if error.is_timeout() {
                        DirectoryCategory::Timeout
                    } else {
                        DirectoryCategory::Network
                    },
                )
            })?;
        if response.status() != StatusCode::OK {
            return Err(DirectoryFailure {
                stage,
                category: DirectoryCategory::Http,
                http_status: Some(response.status().as_u16()),
                server_code: None,
                parser: None,
            });
        }
        let headers = response.headers();
        let header_bytes: usize = headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum();
        if headers.len() > 128
            || header_bytes > 32 * 1024
            || response
                .content_length()
                .is_some_and(|length| length > MAX_BODY as u64)
        {
            return Err(failure(stage, DirectoryCategory::Resource));
        }
        let mut body = Vec::new();
        let mut chunks = 0usize;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failure(stage, DirectoryCategory::Network))?
        {
            if chunk.len() > MAX_BODY - body.len() {
                return Err(failure(stage, DirectoryCategory::Resource));
            }
            let needed = body.len() + chunk.len();
            if needed > body.capacity() {
                let capacity = body
                    .capacity()
                    .saturating_mul(2)
                    .max(4096)
                    .max(needed)
                    .min(MAX_BODY);
                body.try_reserve_exact(capacity - body.len())
                    .map_err(|_| failure(stage, DirectoryCategory::Resource))?;
            }
            body.extend_from_slice(&chunk);
            chunks += 1;
            if chunks.is_multiple_of(64) {
                tokio::task::yield_now().await;
            }
        }
        let outer: Value = serde_json::from_slice(&body).map_err(|_| {
            schema_failure(
                stage,
                wire::PreviewField::ResponseJson,
                crate::protocol::messaging::FrameReason::OuterJson,
                None,
            )
        })?;
        drop(body);
        let encrypted = envelope_data(outer, stage, false)?;
        let plaintext = session
            .decrypt_directory_payload(encrypted.as_str().ok_or_else(|| {
                schema_failure(
                    stage,
                    wire::PreviewField::EncryptedData,
                    crate::protocol::messaging::FrameReason::OuterData,
                    Some(&encrypted),
                )
            })?)
            .map_err(|_| failure(stage, DirectoryCategory::Crypto))?;
        drop(encrypted);
        let inner: Value = serde_json::from_slice(&plaintext).map_err(|_| {
            schema_failure(
                stage,
                wire::PreviewField::DecryptedJson,
                crate::protocol::messaging::FrameReason::InnerJson,
                None,
            )
        })?;
        envelope_data(inner, stage, true)
    }
    pub(crate) async fn hidden(
        &self,
        session: &VerifiedSession,
    ) -> Result<crate::protocol::hidden::HiddenSnapshot, DirectoryFailure> {
        let stage = DirectoryStage::Synchronization;
        let request =
            crate::protocol::hidden::request(session.imei()).map_err(|e| wire_failure(stage, e))?;
        let value = self.fetch(session, request, stage).await?;
        crate::protocol::hidden::parse(&value).map_err(|e| wire_failure(stage, e))
    }
    pub async fn synchronize<S: DirectorySink>(
        &self,
        session: &VerifiedSession,
        sink: &mut S,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<DirectoryReport, DirectoryFailure> {
        if *cancel.borrow_and_update() {
            return Err(failure(
                DirectoryStage::Synchronization,
                DirectoryCategory::Cancelled,
            ));
        }
        tokio::select! {biased;_=cancel.changed()=>Err(failure(DirectoryStage::Synchronization,DirectoryCategory::Cancelled)),result=tokio::time::timeout(TOTAL_DEADLINE,self.run(session,sink))=>result.map_err(|_|failure(DirectoryStage::Synchronization,DirectoryCategory::Timeout))}
    }
    async fn run<S: DirectorySink>(
        &self,
        session: &VerifiedSession,
        sink: &mut S,
    ) -> DirectoryReport {
        let mut report = DirectoryReport::default();
        let observed_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0);
        let contacts = async {
            let request = wire::friends_request(session.imei())
                .map_err(|e| wire_failure(DirectoryStage::Friends, e))?;
            let data = self
                .fetch(session, request, DirectoryStage::Friends)
                .await?;
            let page = wire::parse_friends_partial(&data)
                .map_err(|e| wire_failure(DirectoryStage::Friends, e))?;
            let count = page.entries.len();
            apply_entries(
                sink,
                session,
                page.entries,
                observed_at_ms,
                &mut report.contacts,
            )
            .await?;
            Ok::<(usize, usize), DirectoryFailure>((count, page.unsupported_names))
        }
        .await;
        match contacts {
            Ok((count, unsupported)) => {
                report.contacts = count;
                report.unsupported_contact_names = unsupported;
                report.completed_friends = unsupported == 0 && count < wire::MAX_FRIENDS;
                if !report.completed_friends {
                    report.failures.push(failure(
                        DirectoryStage::Friends,
                        if unsupported > 0 {
                            DirectoryCategory::Schema
                        } else {
                            DirectoryCategory::Resource
                        },
                    ));
                }
            }
            Err(error) => report.failures.push(error),
        }
        if report
            .failures
            .iter()
            .any(|f| f.category == DirectoryCategory::Storage)
        {
            return report;
        }
        let groups = async {
            let mut page = 1;
            let mut count = 0;
            let mut seen = std::collections::HashSet::new();
            loop {
                let request = wire::groups_request(session.imei(), page)
                    .map_err(|e| wire_failure(DirectoryStage::Groups, e))?;
                let data = self.fetch(session, request, DirectoryStage::Groups).await?;
                let parsed = wire::parse_groups(&data, page)
                    .map_err(|e| wire_failure(DirectoryStage::Groups, e))?;
                let entries: Vec<_> = parsed
                    .entries
                    .into_iter()
                    .filter(|entry| seen.insert(entry.remote_id.clone()))
                    .collect();
                if entries.is_empty() && parsed.next_page.is_some() {
                    return Err(failure(DirectoryStage::Groups, DirectoryCategory::Resource));
                }
                count += entries.len();
                apply_entries(sink, session, entries, observed_at_ms, &mut report.groups).await?;
                if let Some(next) = parsed.next_page {
                    page = next;
                } else {
                    break;
                }
            }
            Ok::<usize, DirectoryFailure>(count)
        }
        .await;
        match groups {
            Ok(count) => {
                report.groups = count;
                report.completed_groups = true;
            }
            Err(error) => report.failures.push(error),
        }
        if report
            .failures
            .iter()
            .any(|f| f.category == DirectoryCategory::Storage)
        {
            return report;
        }
        let previews = async {
            let request = wire::previews_request(session.imei())
                .map_err(|e| wire_failure(DirectoryStage::Previews, e))?;
            let data = self
                .fetch(session, request, DirectoryStage::Previews)
                .await?;
            let previews =
                wire::parse_verified_previews(session, &data).map_err(preview_failure)?;
            let count = previews.messages.len();
            let unsupported = previews.unsupported_encrypted;
            let batches = preview_batches(session.account_id(), previews, observed_at_ms)?;
            for batch in batches {
                sink.apply(batch).await?;
            }
            Ok::<(usize, usize), DirectoryFailure>((count, unsupported))
        }
        .await;
        match previews {
            Ok((count, unsupported)) => {
                report.preview_messages = count;
                report.unsupported_encrypted = unsupported;
                report.completed_previews = unsupported == 0;
            }
            Err(error) => report.failures.push(error),
        }
        // Independent optional metadata: a profile failure must never stop messaging
        // or turn a verified hidden policy into an unknown policy.
        if !report
            .failures
            .iter()
            .any(|failure| failure.category == DirectoryCategory::Storage)
        {
            let profile = async {
                let stage = DirectoryStage::Account;
                let request = wire::self_profile_request(session.imei())
                    .map_err(|e| wire_failure(stage, e))?;
                let data = self.fetch(session, request, stage).await?;
                let entry = wire::parse_self_profile(&data, session.account_id())
                    .map_err(|e| wire_failure(stage, e))?;
                let mut ignored = 0;
                apply_entries(sink, session, vec![entry], observed_at_ms, &mut ignored).await
            }
            .await;
            match profile {
                Ok(()) => report.completed_profile = true,
                Err(error) => report.failures.push(error),
            }
        }
        report
    }
}
/// Match full scoped identities before any sink call, then move metadata with
/// its owning message chunk. Neither rich content nor message bodies are cloned.
fn preview_batches(
    account: &str,
    page: wire::PreviewPage,
    observed_at_ms: i64,
) -> Result<Vec<DirectoryBatch>, DirectoryFailure> {
    let invalid = || {
        let mut error = schema_failure(
            DirectoryStage::Previews,
            wire::PreviewField::MessageCollections,
            crate::protocol::messaging::FrameReason::MessageValidation,
            None,
        );
        if let Some(parser) = &mut error.parser {
            parser.field_type = crate::protocol::messaging::FrameValueType::Array;
        }
        error
    };
    let mut metadata = std::collections::BTreeMap::new();
    let mut info_bytes = 0usize;
    for info in page.message_info {
        if info.account_id != account {
            return Err(invalid());
        }
        info_bytes = info_bytes
            .checked_add(info.content.as_json().len())
            .ok_or_else(|| failure(DirectoryStage::Previews, DirectoryCategory::Resource))?;
        if info_bytes > crate::domain::MAX_INFO_BATCH_BYTES {
            return Err(failure(
                DirectoryStage::Previews,
                DirectoryCategory::Resource,
            ));
        }
        let key = (
            info.account_id.clone(),
            info.thread_id.clone(),
            info.message_id.clone(),
        );
        if metadata.insert(key, info).is_some() {
            return Err(invalid());
        }
    }
    let mut media_metadata = std::collections::BTreeMap::new();
    for media in page.message_media {
        if media.account_id != account {
            return Err(invalid());
        }
        info_bytes = info_bytes
            .checked_add(media.content.as_json().len())
            .ok_or_else(|| failure(DirectoryStage::Previews, DirectoryCategory::Resource))?;
        if info_bytes > crate::domain::MAX_INFO_BATCH_BYTES {
            return Err(failure(
                DirectoryStage::Previews,
                DirectoryCategory::Resource,
            ));
        }
        let key = (
            media.account_id.clone(),
            media.thread_id.clone(),
            media.message_id.clone(),
        );
        if metadata.contains_key(&key) || media_metadata.insert(key, media).is_some() {
            return Err(invalid());
        }
    }
    let mut identities = std::collections::HashSet::new();
    for message in &page.messages {
        let key = (
            message.account_id.clone(),
            message.thread_id.clone(),
            message.id.clone(),
        );
        if message.account_id != account
            || message.validate().is_err()
            || !identities.insert((message.thread_id.clone(), message.id.clone()))
            || message.media.as_ref()
                != media_metadata
                    .get(&key)
                    .map(|binding| binding.content.descriptor())
            || metadata
                .get(&key)
                .is_some_and(|_| message.sticker.is_some() || message.media.is_some())
        {
            return Err(invalid());
        }
    }
    let mut messages = page.messages.into_iter();
    let mut batches = Vec::new();
    loop {
        let messages: Vec<_> = messages.by_ref().take(wire::DIRECTORY_BATCH_SIZE).collect();
        if messages.is_empty() {
            break;
        }
        let mut message_info = Vec::new();
        let mut message_media = Vec::new();
        for message in &messages {
            let key = (
                message.account_id.clone(),
                message.thread_id.clone(),
                message.id.clone(),
            );
            if let Some(media) = media_metadata.remove(&key) {
                message_media.push(media);
            }
            if let Some(info) = metadata.remove(&key) {
                message_info.push(info);
            }
        }
        batches.push(DirectoryBatch {
            account_id: account.into(),
            entries: Vec::new(),
            messages,
            message_info,
            message_media,
            observed_at_ms,
        });
    }
    if !metadata.is_empty() || !media_metadata.is_empty() {
        return Err(invalid());
    }
    Ok(batches)
}
fn envelope_data(
    mut value: Value,
    stage: DirectoryStage,
    decrypted: bool,
) -> Result<Value, DirectoryFailure> {
    use crate::protocol::messaging::FrameReason;
    let field = if decrypted {
        wire::PreviewField::DecryptedEnvelope
    } else {
        wire::PreviewField::ResponseEnvelope
    };
    let reason = if decrypted {
        FrameReason::InnerEnvelope
    } else {
        FrameReason::OuterEnvelope
    };
    if !value.is_object() {
        return Err(schema_failure(stage, field, reason, Some(&value)));
    }
    let code = value
        .get("error_code")
        .and_then(Value::as_i64)
        .ok_or_else(|| schema_failure(stage, field, reason, value.get("error_code")))?;
    if code != 0 {
        return Err(DirectoryFailure {
            stage,
            category: DirectoryCategory::Server,
            http_status: None,
            server_code: Some(code),
            parser: None,
        });
    }
    if value.get("data").is_none() {
        return Err(schema_failure(
            stage,
            if decrypted {
                wire::PreviewField::MessageCollections
            } else {
                wire::PreviewField::EncryptedData
            },
            if decrypted {
                FrameReason::BatchEnvelope
            } else {
                FrameReason::OuterData
            },
            None,
        ));
    }
    Ok(value.get_mut("data").expect("checked data").take())
}

async fn apply_entries<S: DirectorySink>(
    sink: &mut S,
    session: &VerifiedSession,
    entries: Vec<wire::DirectoryEntry>,
    observed_at_ms: i64,
    committed: &mut usize,
) -> Result<(), DirectoryFailure> {
    let records: Vec<_> = entries
        .into_iter()
        .map(|entry| DirectoryRecord {
            remote_id: entry.remote_id,
            is_group: entry.kind == crate::protocol::messaging::ThreadKind::Group,
            display_name: entry.display_name,
            avatar_url: entry.avatar_url,
            is_friend: entry.is_friend,
            is_blocked: entry.is_blocked,
            member_count: entry.member_count,
        })
        .collect();
    for entries in records.chunks(wire::DIRECTORY_BATCH_SIZE) {
        sink.apply(DirectoryBatch {
            account_id: session.account_id().into(),
            entries: entries.to_vec(),
            messages: Vec::new(),
            message_info: Vec::new(),
            message_media: Vec::new(),
            observed_at_ms,
        })
        .await?;
        *committed += entries.len();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
