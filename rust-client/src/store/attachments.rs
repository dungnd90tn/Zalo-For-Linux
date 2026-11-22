//! Durable ordinary single-file intent, separate from text drafts and crypto outboxes.
use super::*;
use crate::domain::{MediaDescriptor, MediaKind, MediaMessageContent};
use anyhow::ensure;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
const MAX_FINAL_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct AttachmentSpec {
    pub file_name: String,
    pub size: u64,
    pub checksum: String,
}
impl AttachmentSpec {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.file_name.trim().is_empty()
                && self.file_name.len() <= 1024
                && !self.file_name.contains('/')
                && !self.file_name.chars().any(char::is_control)
                && !matches!(self.file_name.as_str(), "." | "..")
                && self.size > 0
                && self.size <= MAX_ATTACHMENT_BYTES
                && self.checksum.len() == 32
                && self.checksum.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid attachment specification"
        );
        Ok(())
    }
}
pub struct AttachmentResource {
    pub file_id: String,
    pub file_url: String,
    pub checksum: String,
    pub parameters: Value,
    pub content: MediaMessageContent,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentState {
    Uploading,
    ResourceReady,
    Interrupted,
    Sending,
    Unknown,
    Acknowledged,
    Accepted,
    Rejected,
}
impl AttachmentState {
    pub(super) fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "uploading" => Self::Uploading,
            "resource_ready" => Self::ResourceReady,
            "interrupted" => Self::Interrupted,
            "sending" => Self::Sending,
            "unknown" => Self::Unknown,
            "acknowledged" => Self::Acknowledged,
            "accepted" => Self::Accepted,
            "rejected" => Self::Rejected,
            _ => bail!(FailureReason::ConflictingMetadata),
        })
    }
}
pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS attachment_outbox(
        account_id TEXT NOT NULL,client_id TEXT NOT NULL,thread_id TEXT NOT NULL,
        file_name TEXT NOT NULL CHECK(length(CAST(file_name AS BLOB))<=1024),
        size INTEGER NOT NULL CHECK(size>0 AND size<=104857600),checksum TEXT NOT NULL CHECK(length(checksum)=32),
        created_at_ms INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN ('uploading','resource_ready','interrupted','sending','unknown','acknowledged','accepted','rejected')),
        file_id TEXT,file_url TEXT,parameters BLOB CHECK(parameters IS NULL OR length(parameters)<=65536),
        parameters_hash BLOB CHECK(parameters_hash IS NULL OR length(parameters_hash)=32),
        media_metadata TEXT CHECK(media_metadata IS NULL OR length(CAST(media_metadata AS BLOB))<=65536),
        server_id TEXT,server_ts INTEGER,echo_received INTEGER NOT NULL DEFAULT 0 CHECK(echo_received IN (0,1)),
        PRIMARY KEY(account_id,client_id));
        CREATE UNIQUE INDEX IF NOT EXISTS attachment_active_account ON attachment_outbox(account_id) WHERE state IN ('uploading','resource_ready','sending');
        CREATE UNIQUE INDEX IF NOT EXISTS attachment_server_id ON attachment_outbox(account_id,thread_id,server_id) WHERE server_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS attachment_unresolved ON attachment_outbox(account_id,state,thread_id,checksum);")?;
    Ok(())
}
pub(super) fn plain_visible(store: &MessageStore, account: &str, thread: &str) -> Result<()> {
    validate_id(account)?;
    validate_id(thread)?;
    ensure!(
        store.live_thread_visible(account, thread)?
            && !store.group_write_blocked(account, thread)?,
        "Attachment conversation is not available"
    );
    let encrypted: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)",
        params![account, thread],
        |row| row.get(0),
    )?;
    ensure!(!encrypted, "Encrypted attachments are not supported");
    Ok(())
}

pub(super) fn upload_active(conn: &Connection, account: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM attachment_outbox WHERE account_id=?1 AND state IN ('uploading','resource_ready','sending')
            UNION ALL SELECT 1 FROM photo_outbox WHERE account_id=?1 AND state IN ('uploading','resource_ready','sending')
            UNION ALL SELECT 1 FROM voice_outbox WHERE account_id=?1 AND state IN ('prepared','sending')
        )",
        [account],
        |row| row.get(0),
    )?)
}
fn bound_resource(
    spec: &AttachmentSpec,
    thread: &str,
    client: u64,
    resource: &AttachmentResource,
) -> Result<Vec<u8>> {
    spec.validate()?;
    validate_id(&resource.file_id)?;
    ensure!(
        resource.file_id.len() <= 256
            && resource.file_url.len() <= 4096
            && resource.checksum.eq_ignore_ascii_case(&spec.checksum),
        "Attachment resource differs from its intent"
    );
    let address = url::Url::parse(&resource.file_url)?;
    ensure!(
        address.scheme() == "https"
            && address.host_str().is_some()
            && address.username().is_empty()
            && address.password().is_none()
            && address.fragment().is_none()
            && address.port().is_none(),
        "Invalid attachment resource URL"
    );
    let params = resource
        .parameters
        .as_object()
        .ok_or(FailureReason::Validation)?;
    let allowed = [
        "fileId",
        "checksum",
        "checksumSha",
        "extension",
        "totalSize",
        "fileName",
        "clientId",
        "fType",
        "fileCount",
        "fdata",
        "fileUrl",
        "ttl",
        "toid",
        "grid",
        "imei",
    ];
    ensure!(
        params.keys().all(|key| allowed.contains(&key.as_str())),
        "Unsupported attachment parameter"
    );
    let string = |key: &str| params.get(key).and_then(Value::as_str);
    let integer = |key: &str| params.get(key).and_then(Value::as_u64);
    ensure!(
        string("fileId") == Some(resource.file_id.as_str())
            && string("fileUrl") == Some(resource.file_url.as_str())
            && string("fileName") == Some(spec.file_name.as_str())
            && string("checksum") == Some(spec.checksum.to_ascii_lowercase().as_str())
            && string("checksumSha") == Some("")
            && integer("clientId") == Some(client)
            && integer("totalSize") == Some(spec.size)
            && integer("fType") == Some(1)
            && integer("fileCount") == Some(0)
            && string("fdata") == Some("{}")
            && integer("ttl") == Some(0),
        "Attachment parameters differ from their intent"
    );
    let extension = string("extension").ok_or(FailureReason::Validation)?;
    ensure!(
        extension.len() <= 32 && !extension.chars().any(char::is_control),
        "Invalid attachment extension"
    );
    if let Some(peer) = thread.strip_prefix("d:") {
        ensure!(
            string("toid") == Some(peer)
                && !params.contains_key("grid")
                && string("imei").is_some_and(|value| !value.is_empty() && value.len() <= 256),
            "Attachment direct scope mismatch"
        );
    } else if let Some(group) = thread.strip_prefix("g:") {
        ensure!(
            string("grid") == Some(group)
                && !params.contains_key("toid")
                && !params.contains_key("imei"),
            "Attachment group scope mismatch"
        );
    } else {
        bail!(FailureReason::Validation);
    }
    let MediaDescriptor::File(file) = resource.content.descriptor() else {
        bail!(FailureReason::Validation);
    };
    ensure!(
        resource.content.kind() == MediaKind::File
            && !file.encrypted
            && !file.is_folder
            && file.title == spec.file_name
            && file.size_bytes == Some(spec.size)
            && file.url.as_deref() == Some(resource.file_url.as_str())
            && file.extension == extension.to_lowercase()
            && resource.content.as_json().len() <= MAX_FINAL_BYTES,
        "Attachment echo differs from its resource"
    );
    let raw: Value = serde_json::from_str(resource.content.as_json())?;
    let metadata = raw.get("params").ok_or(FailureReason::Validation)?;
    let metadata: Value = if let Some(encoded) = metadata.as_str() {
        serde_json::from_str(encoded)?
    } else {
        metadata.clone()
    };
    ensure!(
        metadata
            .get("checksum")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case(&spec.checksum)),
        "Attachment echo checksum mismatch"
    );
    let bytes = serde_json::to_vec(&resource.parameters)?;
    ensure!(
        bytes.len() <= MAX_FINAL_BYTES,
        "Attachment parameters exceed limit"
    );
    Ok(bytes)
}

fn verify_echo(
    expected: &MediaMessageContent,
    incoming: &MediaMessageContent,
    file_id: &str,
    checksum: &str,
) -> Result<()> {
    ensure!(
        expected.kind() == MediaKind::File
            && incoming.kind() == MediaKind::File
            && expected.descriptor() == incoming.descriptor(),
        FailureReason::ConflictingEcho
    );
    let raw: Value = serde_json::from_str(incoming.as_json())?;
    let metadata = raw
        .get("params")
        .ok_or(FailureReason::ConflictingMetadata)?;
    let metadata: Value = if let Some(encoded) = metadata.as_str() {
        serde_json::from_str(encoded)?
    } else {
        metadata.clone()
    };
    if let Some(value) = metadata.get("checksum") {
        ensure!(
            value
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case(checksum)),
            FailureReason::ConflictingEcho
        );
    }
    if let Some(value) = metadata.get("fileId").or_else(|| raw.get("fileId")) {
        let value = value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.as_u64().map(|value| value.to_string()))
            .ok_or(FailureReason::ConflictingEcho)?;
        ensure!(value == file_id, FailureReason::ConflictingEcho);
    }
    Ok(())
}

/// Only an actual own `share.file` message can settle this typed ledger. The
/// placeholder body alone is never correlation evidence.
pub(super) fn reconcile_received(
    conn: &Connection,
    message: &mut Message,
    hint: Option<&str>,
    content: &MediaMessageContent,
) -> Result<bool> {
    if message.sender != message.account_id || content.kind() != MediaKind::File {
        return Ok(false);
    }
    type Binding = (
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        bool,
    );
    let read = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Binding> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
        ))
    };
    let explicit=hint.map(|hint|conn.query_row("SELECT client_id,thread_id,state,server_id,media_metadata,file_id,checksum,echo_received FROM attachment_outbox WHERE account_id=?1 AND client_id=?2 AND media_metadata IS NOT NULL",params![message.account_id,hint],read).optional()).transpose()?.flatten();
    let bound=conn.query_row("SELECT client_id,thread_id,state,server_id,media_metadata,file_id,checksum,echo_received FROM attachment_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3",params![message.account_id,message.thread_id,message.id],read).optional()?;
    if explicit
        .as_ref()
        .zip(bound.as_ref())
        .is_some_and(|(a, b)| a.0 != b.0)
    {
        bail!(FailureReason::ConflictingEcho);
    }
    let Some((client, thread, state, server, raw, file_id, checksum, received)) =
        explicit.or(bound)
    else {
        return Ok(false);
    };
    ensure!(
        thread == message.thread_id
            && matches!(
                state.as_str(),
                "sending" | "unknown" | "acknowledged" | "accepted"
            )
            && server.as_deref().is_none_or(|server| server == message.id),
        FailureReason::ConflictingEcho
    );
    ensure!(
        raw.len() <= MAX_FINAL_BYTES,
        FailureReason::ConflictingMetadata
    );
    let expected = MediaMessageContent::new(MediaKind::File, &serde_json::from_str(&raw)?)
        .map_err(|_| FailureReason::ConflictingMetadata)?;
    verify_echo(&expected, content, &file_id, &checksum)?;
    let previous:Option<(String,String,i64)>=conn.query_row("SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![message.account_id,thread,message.id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    if let Some((owner, body, _)) = &previous {
        ensure!(
            owner == &message.account_id && body == MediaKind::File.legacy_placeholder(),
            FailureReason::ConflictingEcho
        );
    }
    let old = super::message_media::load(conn, &message.account_id, &thread, &message.id)?;
    let enriched = if let Some(old) = old {
        if old.as_json() != content.as_json() {
            ensure!(
                !received && old.as_json() == expected.as_json(),
                FailureReason::ConflictingMetadata
            );
            conn.execute(
                "DELETE FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![message.account_id, thread, message.id],
            )?;
            true
        } else {
            false
        }
    } else {
        false
    };
    let retimed = previous
        .as_ref()
        .is_some_and(|(_, _, time)| *time != message.sent_at_ms);
    if retimed {
        conn.execute("UPDATE messages SET sent_at_ms=?4 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![message.account_id,thread,message.id,message.sent_at_ms])?;
    }
    conn.execute("UPDATE attachment_outbox SET state='accepted',server_id=?3,server_ts=?4,echo_received=1 WHERE account_id=?1 AND client_id=?2",params![message.account_id,client,message.id,message.sent_at_ms])?;
    message.source_client_id = Some(client);
    message.reply_blocked = true;
    Ok(state != "accepted" || enriched || retimed)
}

struct FinalResource {
    thread: String,
    file_id: String,
    checksum: String,
    parameters: Vec<u8>,
    content: MediaMessageContent,
}
fn final_resource(conn: &Connection, account: &str, client: u64) -> Result<FinalResource> {
    type Row = (
        String,
        String,
        u64,
        String,
        String,
        String,
        Vec<u8>,
        Vec<u8>,
        String,
    );
    let (thread,name,size,checksum,file_id,file_url,parameters,hash,raw):Row=conn.query_row("SELECT thread_id,file_name,size,checksum,file_id,file_url,parameters,parameters_hash,media_metadata FROM attachment_outbox WHERE account_id=?1 AND client_id=?2",params![account,client.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?)))?;
    ensure!(
        parameters.len() <= MAX_FINAL_BYTES
            && raw.len() <= MAX_FINAL_BYTES
            && hash.as_slice() == Sha256::digest(&parameters).as_slice(),
        "Attachment resource changed"
    );
    let resource = AttachmentResource {
        file_id: file_id.clone(),
        file_url,
        checksum: checksum.clone(),
        parameters: serde_json::from_slice(&parameters)?,
        content: MediaMessageContent::new(MediaKind::File, &serde_json::from_str(&raw)?)
            .map_err(|_| FailureReason::ConflictingMetadata)?,
    };
    ensure!(
        bound_resource(
            &AttachmentSpec {
                file_name: name,
                size,
                checksum: checksum.clone()
            },
            &thread,
            client,
            &resource
        )? == parameters,
        "Attachment resource binding changed"
    );
    Ok(FinalResource {
        thread,
        file_id,
        checksum,
        parameters,
        content: resource.content,
    })
}

impl MessageStore {
    pub fn reserve_attachment(
        &mut self,
        account: &str,
        thread: &str,
        spec: &AttachmentSpec,
        now_ms: i64,
    ) -> Result<u64> {
        plain_visible(self, account, thread)?;
        spec.validate()?;
        ensure!(now_ms > 0, "Invalid attachment time");
        let checksum = spec.checksum.to_ascii_lowercase();
        let tx = self.conn.transaction()?;
        ensure!(
            !upload_active(&tx, account)?,
            "An attachment upload is already active"
        );
        let unresolved:i64=tx.query_row("SELECT count(*) FROM attachment_outbox WHERE account_id=?1 AND state IN ('unknown','acknowledged')",[account],|row|row.get(0))?;
        ensure!(unresolved < 100, "Too many unresolved attachment sends");
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attachment_outbox WHERE account_id=?1 AND thread_id=?2 AND file_name=?3 AND size=?4 AND checksum=?5 AND state IN ('unknown','acknowledged'))",params![account,thread,spec.file_name,spec.size as i64,checksum],|row|row.get(0))?;
        ensure!(!duplicate, "This attachment has an unresolved send");
        let previous: Option<i64> = tx
            .query_row(
                "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                [account],
                |row| row.get(0),
            )
            .optional()?;
        let next = now_ms.max(
            previous
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(FailureReason::Validation)?,
        );
        ensure!(
            next <= 9_007_199_254_740_991,
            "Attachment client ID exhausted"
        );
        tx.execute("INSERT INTO outgoing_counters VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",params![account,next])?;
        tx.execute("INSERT INTO attachment_outbox(account_id,client_id,thread_id,file_name,size,checksum,created_at_ms,state) VALUES(?1,?2,?3,?4,?5,?6,?7,'uploading')",params![account,next.to_string(),thread,spec.file_name,spec.size as i64,checksum,now_ms])?;
        tx.commit()?;
        Ok(next as u64)
    }
    pub fn attachment_state(&self, account: &str, client: u64) -> Result<Option<AttachmentState>> {
        validate_id(account)?;
        let state: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM attachment_outbox WHERE account_id=?1 AND client_id=?2",
                params![account, client.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        state.as_deref().map(AttachmentState::parse).transpose()
    }
    pub fn attachment_resource_ready(
        &mut self,
        account: &str,
        client: u64,
        resource: &AttachmentResource,
    ) -> Result<()> {
        validate_id(account)?;
        let (thread,name,size,checksum,state,old): (String,String,u64,String,String,Option<Vec<u8>>)=self.conn.query_row("SELECT thread_id,file_name,size,checksum,state,parameters FROM attachment_outbox WHERE account_id=?1 AND client_id=?2",params![account,client.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)))?;
        plain_visible(self, account, &thread)?;
        let bytes = bound_resource(
            &AttachmentSpec {
                file_name: name,
                size,
                checksum,
            },
            &thread,
            client,
            resource,
        )?;
        let hash = Sha256::digest(&bytes);
        if state == "resource_ready" {
            ensure!(
                old.as_deref() == Some(bytes.as_slice()),
                "Attachment resource is immutable"
            );
            return Ok(());
        }
        ensure!(
            state == "uploading",
            "Attachment is not awaiting a resource"
        );
        ensure!(self.conn.execute("UPDATE attachment_outbox SET state='resource_ready',file_id=?3,file_url=?4,parameters=?5,parameters_hash=?6,media_metadata=?7 WHERE account_id=?1 AND client_id=?2 AND state='uploading'",params![account,client.to_string(),resource.file_id,resource.file_url,bytes,hash.as_slice(),resource.content.as_json()])?==1,"Attachment resource transition conflict");
        Ok(())
    }
    /// The actor must freshly verify its current auth/encryption generation first.
    /// A durable one-shot claim is required immediately before the final HTTP POST.
    pub fn begin_attachment_dispatch(&mut self, account: &str, client: u64) -> Result<Vec<u8>> {
        validate_id(account)?;
        let thread: String = self.conn.query_row(
            "SELECT thread_id FROM attachment_outbox WHERE account_id=?1 AND client_id=?2",
            params![account, client.to_string()],
            |row| row.get(0),
        )?;
        plain_visible(self, account, &thread)?;
        let tx = self.conn.transaction()?;
        let resource = final_resource(&tx, account, client)?;
        ensure!(resource.thread == thread, "Attachment scope changed");
        ensure!(tx.execute("UPDATE attachment_outbox SET state='sending' WHERE account_id=?1 AND client_id=?2 AND state='resource_ready'",params![account,client.to_string()])?==1,"Attachment dispatch conflict");
        tx.commit()?;
        Ok(resource.parameters)
    }
    pub fn interrupt_attachment(&self, account: &str, client: u64) -> Result<()> {
        validate_id(account)?;
        self.conn.execute("UPDATE attachment_outbox SET state='interrupted' WHERE account_id=?1 AND client_id=?2 AND state IN ('uploading','resource_ready')",params![account,client.to_string()])?;
        Ok(())
    }
    pub fn fail_attachment_dispatch(
        &self,
        account: &str,
        client: u64,
        unknown: bool,
    ) -> Result<()> {
        validate_id(account)?;
        self.conn.execute("UPDATE attachment_outbox SET state=?3 WHERE account_id=?1 AND client_id=?2 AND state='sending'",params![account,client.to_string(),if unknown{"unknown"}else{"rejected"}])?;
        Ok(())
    }
    pub fn recover_attachments(&self, account: &str) -> Result<()> {
        validate_id(account)?;
        self.conn.execute("UPDATE attachment_outbox SET state=CASE WHEN state='sending' THEN 'unknown' ELSE 'interrupted' END WHERE account_id=?1 AND state IN ('uploading','resource_ready','sending')",[account])?;
        Ok(())
    }
    pub fn record_attachment_ack(
        &self,
        account: &str,
        client: u64,
        server: &str,
        timestamp: Option<i64>,
    ) -> Result<()> {
        validate_id(account)?;
        validate_id(server)?;
        ensure!(
            timestamp.is_none_or(|value| value >= 0),
            "Invalid attachment acknowledgement time"
        );
        ensure!(self.conn.execute("UPDATE attachment_outbox SET state=CASE WHEN state='accepted' THEN state ELSE 'acknowledged' END,server_id=?3,server_ts=COALESCE(server_ts,?4) WHERE account_id=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','accepted') AND (server_id IS NULL OR server_id=?3)
          AND NOT EXISTS(SELECT 1 FROM forward_outbox WHERE account_id=?1 AND thread_id=attachment_outbox.thread_id AND server_id=?3)
          AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=attachment_outbox.thread_id AND message_id=?3 AND kind='forwarded')",params![account,client.to_string(),server,timestamp])?==1,"Attachment acknowledgement conflict");
        Ok(())
    }
    pub fn accept_attachment(
        &mut self,
        account: &str,
        client: u64,
    ) -> Result<(Message, Conversation)> {
        validate_id(account)?;
        let tx = self.conn.transaction()?;
        let (thread,server,created,timestamp,raw):(String,String,i64,Option<i64>,Option<String>)=tx.query_row("SELECT thread_id,server_id,created_at_ms,server_ts,media_metadata FROM attachment_outbox WHERE account_id=?1 AND client_id=?2 AND state IN ('acknowledged','accepted')",params![account,client.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
        let Some(raw) = raw else {
            return super::recall::accepted_media_replay(&tx, account, &thread, &server, client);
        };
        ensure!(
            raw.len() <= MAX_FINAL_BYTES,
            "Attachment echo exceeds limit"
        );
        ensure!(
            !super::forward::server_reserved(&tx, account, &thread, &server)?,
            FailureReason::ConflictingEcho
        );
        let resource = final_resource(&tx, account, client)?;
        ensure!(
            resource.thread == thread && resource.content.as_json() == raw,
            "Attachment acknowledgement scope changed"
        );
        let mut content = resource.content;
        if let Some(wire) = super::message_media::load(&tx, account, &thread, &server)? {
            verify_echo(&content, &wire, &resource.file_id, &resource.checksum)?;
            content = wire;
        }
        let existing:Option<i64>=tx.query_row("SELECT sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![account,thread,server],|row|row.get(0)).optional()?;
        let mut message = Message {
            account_id: account.into(),
            thread_id: thread.clone(),
            id: server,
            sender: account.into(),
            text: MediaKind::File.legacy_placeholder().into(),
            source_client_id: Some(client.to_string()),
            mentions: Vec::new(),
            reply_blocked: true,
            recalled: false,
            quote: None,
            sticker: None,
            media: Some(content.descriptor().clone()),
            sent_at_ms: existing.or(timestamp).unwrap_or(created),
        };
        message.validate()?;
        insert_validated_with_media(&tx, std::iter::once((&message, None, Some(&content))))?;
        super::message_reply::hydrate(&tx, &mut message)?;
        ensure!(tx.execute("UPDATE attachment_outbox SET state='accepted' WHERE account_id=?1 AND client_id=?2 AND state IN ('acknowledged','accepted')",params![account,client.to_string()])?==1,"Attachment acceptance conflict");
        #[cfg(feature = "messaging")]
        if let Some(change) = super::recall::materialize_pending_media(&tx, &message)? {
            let recalled = change
                .message
                .zip(change.conversation)
                .ok_or(FailureReason::ConflictingEcho)?;
            tx.commit()?;
            return Ok(recalled);
        }
        let conversation = tx.query_row(
            CONVERSATION_SQL,
            params![account, thread, MAX_PREVIEW_CHARS as i64],
            conversation_row,
        )?;
        tx.commit()?;
        Ok((message, conversation))
    }
    pub fn reconcile_attachments(&mut self, account: &str) -> Result<()> {
        validate_id(account)?;
        let ids = {
            let mut query=self.conn.prepare("SELECT client_id FROM attachment_outbox WHERE account_id=?1 AND state='acknowledged' ORDER BY client_id LIMIT 100")?;
            let values = query.query_map([account], |row| row.get::<_, String>(0))?;
            values.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in ids {
            self.accept_attachment(account, id.parse()?)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(feature = "messaging")]
    #[test]
    fn file_recall_scrubs_payload_and_dominates_typed_replay() {
        use crate::protocol::recall::RecallNotice;

        let mut store = MessageStore::in_memory().unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        let spec = AttachmentSpec {
            file_name: "synthetic-recall.txt".into(),
            size: 6,
            checksum: "a".repeat(32),
        };
        let client = store.reserve_attachment("11", "d:22", &spec, 1000).unwrap();
        let url = "https://files.zdn.vn/synthetic-recall.txt";
        let resource = AttachmentResource {
            file_id: "synthetic-file".into(),
            file_url: url.into(),
            checksum: spec.checksum.clone(),
            parameters: json!({
                "fileId":"synthetic-file","checksum":spec.checksum,"checksumSha":"",
                "extension":"txt","totalSize":6,"fileName":spec.file_name,"clientId":client,
                "fType":1,"fileCount":0,"fdata":"{}","fileUrl":url,"ttl":0,"toid":"22",
                "imei":"synthetic-device"
            }),
            content: MediaMessageContent::new(
                MediaKind::File,
                &json!({
                    "title":spec.file_name,"href":url,
                    "params":{"fileSize":6,"fileExt":"txt","checksum":spec.checksum,"fType":1}
                }),
            )
            .unwrap(),
        };
        store
            .attachment_resource_ready("11", client, &resource)
            .unwrap();
        store.begin_attachment_dispatch("11", client).unwrap();
        store
            .record_attachment_ack("11", client, "901", Some(1020))
            .unwrap();
        let (message, _) = store.accept_attachment("11", client).unwrap();
        let bad_notice = RecallNotice {
            thread_id: "d:22".into(),
            message_id: "901".into(),
            actor_id: "11".into(),
            client_id: Some("999".into()),
            event_id: "902".into(),
            occurred_at_ms: 1040,
        };
        let (_, _, changes) = store
            .receive_batch_with_recalls(
                "11",
                &[],
                None,
                &[],
                ReceiveContext::OfflineSync,
                &[bad_notice],
            )
            .unwrap();
        assert!(changes.is_empty());
        assert_eq!(
            store
                .search_messages("11", "d:22", "synthetic-recall", 5)
                .unwrap()
                .hits
                .len(),
            1
        );

        let recall = store
            .prepare_recall("11", "d:22", "901", 1050, 1000)
            .unwrap();
        assert_eq!(recall.reference.client_id, client.to_string());
        store.record_recall_ack("11", recall.client_id).unwrap();
        assert!(store
            .accept_recall_ack("11", recall.client_id)
            .unwrap()
            .message
            .unwrap()
            .is_recalled());

        store
            .record_attachment_ack("11", client, "901", Some(1020))
            .unwrap();
        assert!(store
            .accept_attachment("11", client)
            .unwrap()
            .0
            .is_recalled());
        store
            .receive_batch_with_media(
                "11",
                &[(
                    message,
                    Some(client.to_string()),
                    None,
                    Some(resource.content),
                )],
                None,
            )
            .unwrap();
        let saved = store
            .refresh_messages("11", "d:22", &["901".into()])
            .unwrap();
        assert!(saved[0].is_recalled());
        assert!(saved[0].media.is_none());
        assert!(store.message_media("11", "d:22", "901").unwrap().is_none());
        assert!(store
            .search_messages("11", "d:22", "synthetic-recall", 5)
            .unwrap()
            .hits
            .is_empty());
        assert!(store.conn.query_row(
            "SELECT file_name='' AND file_id IS NULL AND file_url IS NULL AND parameters IS NULL
                AND parameters_hash IS NULL AND media_metadata IS NULL
             FROM attachment_outbox WHERE account_id='11' AND client_id=?1",
            [client.to_string()], |row| row.get::<_, bool>(0),
        ).unwrap());
    }

    #[test]
    fn final_file_dispatch_rechecks_hidden_policy_and_encryption_without_claiming_bytes() {
        for mode in 0..3 {
            let mut store = MessageStore::in_memory().unwrap();
            let spec = AttachmentSpec {
                file_name: "Tổng hợp.txt".into(),
                size: 6,
                checksum: "a".repeat(32),
            };
            let id = store.reserve_attachment("11", "d:22", &spec, 1000).unwrap();
            let url = "https://files.zdn.vn/synthetic.txt";
            let resource=AttachmentResource{file_id:"synthetic-file".into(),file_url:url.into(),checksum:spec.checksum.clone(),parameters:json!({"fileId":"synthetic-file","checksum":spec.checksum,"checksumSha":"","extension":"txt","totalSize":6,"fileName":spec.file_name,"clientId":id,"fType":1,"fileCount":0,"fdata":"{}","fileUrl":url,"ttl":0,"toid":"22","imei":"synthetic-device"}),content:MediaMessageContent::new(MediaKind::File,&json!({"title":spec.file_name,"href":url,"params":{"fileSize":6,"fileExt":"txt","checksum":spec.checksum,"fType":1}})).unwrap()};
            store
                .attachment_resource_ready("11", id, &resource)
                .unwrap();
            match mode {
                0 => store.begin_live_privacy("11").unwrap(),
                1 => store
                    .replace_live_privacy("11", &["d:22".into()], None)
                    .unwrap(),
                _ => {
                    store
                        .conn
                        .execute("INSERT INTO e2ee_threads VALUES('11','d:22')", [])
                        .unwrap();
                }
            }
            assert!(store.begin_attachment_dispatch("11", id).is_err());
            assert_eq!(
                store.attachment_state("11", id).unwrap(),
                Some(AttachmentState::ResourceReady)
            );
            match mode {
                0 => store.replace_live_privacy("11", &[], None).unwrap(),
                1 => store.unlock_live_thread("11", Some("d:22")).unwrap(),
                _ => {
                    store.conn.execute("DELETE FROM e2ee_threads", []).unwrap();
                }
            }
            assert_eq!(
                serde_json::from_slice::<Value>(
                    &store.begin_attachment_dispatch("11", id).unwrap()
                )
                .unwrap(),
                resource.parameters
            );
        }
    }
}
