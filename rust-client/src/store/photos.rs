//! Durable ordinary photo intent; independent from file and text correlation.
use super::*;
use crate::domain::{
    MediaDescriptor, MediaKind, MediaMessageContent, PhotoAlbumMember, PhotoResolution,
};
use anyhow::ensure;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const MAX_PHOTO_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_ALBUM_PHOTOS: usize = 6;
const MAX_FINAL_BYTES: usize = 128 * 1024;
// Before a final resource exists, this mode marker is the only permitted
// non-null parameters value. Ready rows carry the sealed final JSON instead.
const ORIGINAL_RESERVATION: &[u8] = b"original-photo";
const ALBUM_RESERVATION: &[u8] = b"album-photo:";

#[derive(Clone, Copy)]
pub(crate) struct AlbumPosition {
    pub(crate) layout_id: Option<u64>,
    pub(crate) index: u32,
    pub(crate) count: u32,
}

fn album_reservation(member: PhotoAlbumMember) -> Vec<u8> {
    let mut marker = ALBUM_RESERVATION.to_vec();
    marker.extend(
        format!(
            "{}/{}/{}",
            member.layout_id(),
            member.index(),
            member.count()
        )
        .bytes(),
    );
    marker
}

fn parse_album_reservation(marker: &[u8]) -> Option<PhotoAlbumMember> {
    let mut fields = std::str::from_utf8(marker.strip_prefix(ALBUM_RESERVATION)?)
        .ok()?
        .split('/');
    let member = PhotoAlbumMember::new(
        fields.next()?.parse().ok()?,
        fields.next()?.parse().ok()?,
        fields.next()?.parse().ok()?,
    )?;
    fields.next().is_none().then_some(member)
}

#[derive(Clone, PartialEq, Eq)]
pub struct PhotoSpec {
    pub caption: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
    pub checksum: String,
}
impl PhotoSpec {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.caption.len() <= crate::domain::MAX_TEXT_BYTES
                && self.width > 0
                && self.width <= 2560
                && self.height > 0
                && self.height <= 2560
                && self.size > 0
                && self.size <= MAX_PHOTO_BYTES
                && self.checksum.len() == 32
                && self.checksum.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid photo specification"
        );
        Ok(())
    }
}
pub struct PhotoResource {
    pub photo_id: String,
    pub parameters: Value,
    pub content: MediaMessageContent,
}
pub type PhotoState = super::attachments::AttachmentState;
pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS photo_outbox(
        account_id TEXT NOT NULL,client_id TEXT NOT NULL,thread_id TEXT NOT NULL,
        caption TEXT NOT NULL CHECK(length(CAST(caption AS BLOB))<=65536),
        width INTEGER NOT NULL CHECK(width>0 AND width<=2560),height INTEGER NOT NULL CHECK(height>0 AND height<=2560),
        size INTEGER NOT NULL CHECK(size>0 AND size<=5242880),checksum TEXT NOT NULL CHECK(length(checksum)=32),
        created_at_ms INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN ('uploading','resource_ready','interrupted','sending','unknown','acknowledged','accepted','rejected')),
        photo_id TEXT,parameters BLOB CHECK(parameters IS NULL OR length(parameters)<=131072),
        parameters_hash BLOB CHECK(parameters_hash IS NULL OR length(parameters_hash)=32),
        media_metadata TEXT CHECK(media_metadata IS NULL OR length(CAST(media_metadata AS BLOB))<=131072),
        server_id TEXT,server_ts INTEGER,echo_received INTEGER NOT NULL DEFAULT 0 CHECK(echo_received IN (0,1)),
        PRIMARY KEY(account_id,client_id));
        CREATE UNIQUE INDEX IF NOT EXISTS photo_active_account ON photo_outbox(account_id) WHERE state IN ('uploading','resource_ready','sending');
        CREATE UNIQUE INDEX IF NOT EXISTS photo_server_id ON photo_outbox(account_id,thread_id,server_id) WHERE server_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS photo_unresolved ON photo_outbox(account_id,state,thread_id,checksum);")?;
    Ok(())
}
fn plain_visible(store: &MessageStore, account: &str, thread: &str) -> Result<()> {
    super::attachments::plain_visible(store, account, thread)
}
fn valid_url(value: &str) -> Result<()> {
    let address = url::Url::parse(value)?;
    ensure!(
        value.len() <= 4096
            && address.scheme() == "https"
            && address.host_str().is_some()
            && address.username().is_empty()
            && address.password().is_none()
            && address.fragment().is_none()
            && address.port().is_none(),
        "Invalid photo resource URL"
    );
    Ok(())
}
fn metadata(raw: &Value) -> Result<Value> {
    let value = raw
        .get("params")
        .ok_or(FailureReason::ConflictingMetadata)?;
    Ok(if let Some(encoded) = value.as_str() {
        serde_json::from_str(encoded)?
    } else {
        value.clone()
    })
}

fn parameter_resolution(value: &Value) -> Result<PhotoResolution> {
    let Some(marker) = value.get("jcp") else {
        return Ok(PhotoResolution::Hd);
    };
    let encoded = marker
        .as_str()
        .filter(|marker| marker.len() <= 64)
        .ok_or(FailureReason::ConflictingMetadata)?;
    let mode: Value = serde_json::from_str(encoded)?;
    ensure!(
        mode == serde_json::json!({"is_original":1}),
        FailureReason::ConflictingMetadata
    );
    Ok(PhotoResolution::Original)
}

fn original_marker(metadata: &Value) -> bool {
    metadata.get("is_original").and_then(Value::as_u64) == Some(1)
}

fn bound_resource(
    spec: &PhotoSpec,
    thread: &str,
    client: u64,
    resource: &PhotoResource,
    resolution: PhotoResolution,
    album: Option<PhotoAlbumMember>,
) -> Result<Vec<u8>> {
    spec.validate()?;
    ensure!(
        !resource.photo_id.is_empty()
            && resource.photo_id.len() <= 128
            && resource
                .photo_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "Invalid photo resource ID"
    );
    let params = resource
        .parameters
        .as_object()
        .ok_or(FailureReason::Validation)?;
    let allowed = [
        "photoId",
        "clientId",
        "desc",
        "width",
        "height",
        "rawUrl",
        "thumbUrl",
        "hdUrl",
        "normalUrl",
        "oriUrl",
        "thumbSize",
        "fileSize",
        "hdSize",
        "language",
        "imei",
        "ttl",
        "toid",
        "grid",
        "jcp",
        "groupLayoutId",
        "idInGroup",
        "isGroupLayout",
        "totalItemInGroup",
        "extMsgProp",
    ];
    ensure!(
        params.keys().all(|key| allowed.contains(&key.as_str())),
        "Unsupported photo parameter"
    );
    let string = |key: &str| params.get(key).and_then(Value::as_str);
    let integer = |key: &str| params.get(key).and_then(Value::as_u64);
    ensure!(
        PhotoAlbumMember::from_message_parameters(&resource.parameters) == album
            && (album.is_some()
                || !crate::domain::media::has_album_message_fields(&resource.parameters))
            && resource.content.album_member() == album
            && (album.is_none() || resolution == PhotoResolution::Hd),
        "Photo album metadata differs from its intent"
    );
    ensure!(
        string("photoId") == Some(resource.photo_id.as_str())
            && integer("clientId") == Some(client)
            && string("desc") == Some(spec.caption.as_str())
            && integer("width") == Some(u64::from(spec.width))
            && integer("height") == Some(u64::from(spec.height))
            && integer("ttl") == Some(0)
            && string("language") == Some("VN")
            && parameter_resolution(&resource.parameters)? == resolution
            && string("imei").is_some_and(|v| !v.is_empty() && v.len() <= 256),
        "Photo parameters differ from their intent"
    );
    for key in ["rawUrl", "thumbUrl", "hdUrl"] {
        valid_url(string(key).ok_or(FailureReason::Validation)?)?;
    }
    for key in ["thumbSize", "fileSize", "hdSize"] {
        ensure!(
            params
                .get(key)
                .is_none_or(|value| value.as_u64().is_some_and(|n| n <= 104_857_600)),
            "Invalid photo resource size"
        );
    }
    if let Some(peer) = thread.strip_prefix("d:") {
        ensure!(
            string("toid") == Some(peer)
                && !params.contains_key("grid")
                && !params.contains_key("oriUrl")
                && string("normalUrl") == string("rawUrl"),
            "Photo direct scope mismatch"
        );
    } else if let Some(group) = thread.strip_prefix("g:") {
        ensure!(
            string("grid") == Some(group)
                && !params.contains_key("toid")
                && !params.contains_key("normalUrl")
                && string("oriUrl") == string("rawUrl"),
            "Photo group scope mismatch"
        );
    } else {
        bail!(FailureReason::Validation);
    }
    ensure!(
        resource.content.kind() == MediaKind::Photo
            && resource.content.as_json().len() <= MAX_FINAL_BYTES,
        "Photo echo kind differs from resource"
    );
    let raw: Value = serde_json::from_str(resource.content.as_json())?;
    let meta = metadata(&raw)?;
    ensure!(
        raw.get("title").and_then(Value::as_str) == Some("")
            && raw.get("description").and_then(Value::as_str) == Some(spec.caption.as_str())
            && raw.get("href").and_then(Value::as_str) == string("rawUrl")
            && raw.get("thumb").and_then(Value::as_str) == string("thumbUrl")
            && meta.get("hd").and_then(Value::as_str) == string("hdUrl")
            && meta.get("rawUrl").and_then(Value::as_str) == string("rawUrl")
            && original_marker(&meta) == (resolution == PhotoResolution::Original)
            && meta.get("width").and_then(Value::as_u64) == Some(u64::from(spec.width))
            && meta.get("height").and_then(Value::as_u64) == Some(u64::from(spec.height)),
        "Photo echo differs from resource"
    );
    let MediaDescriptor::Photo(photo) = resource.content.descriptor() else {
        bail!(FailureReason::Validation);
    };
    let original = if resolution == PhotoResolution::Original {
        "rawUrl"
    } else {
        "hdUrl"
    };
    ensure!(
        photo.width == Some(spec.width)
            && photo.height == Some(spec.height)
            && photo.thumbnail_url.as_deref() == string("thumbUrl")
            && photo.original_url.as_deref() == string(original),
        "Photo presentation differs from resource"
    );
    let bytes = serde_json::to_vec(&resource.parameters)?;
    ensure!(
        bytes.len() <= MAX_FINAL_BYTES,
        "Photo parameters exceed limit"
    );
    Ok(bytes)
}
fn verify_echo(
    expected: &MediaMessageContent,
    incoming: &MediaMessageContent,
    photo_id: &str,
    checksum: &str,
) -> Result<()> {
    ensure!(
        expected.kind() == MediaKind::Photo
            && incoming.kind() == MediaKind::Photo
            && expected.descriptor() == incoming.descriptor(),
        FailureReason::ConflictingEcho
    );
    let expected_raw: Value = serde_json::from_str(expected.as_json())?;
    let raw: Value = serde_json::from_str(incoming.as_json())?;
    ensure!(
        raw.get("description") == expected_raw.get("description"),
        FailureReason::ConflictingEcho
    );
    let meta = metadata(&raw)?;
    ensure!(
        original_marker(&meta) == original_marker(&metadata(&expected_raw)?),
        FailureReason::ConflictingEcho
    );
    ensure!(
        incoming.album_member() == expected.album_member(),
        FailureReason::ConflictingEcho
    );
    if let Some(value) = meta.get("checksum") {
        ensure!(
            value
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(checksum)),
            FailureReason::ConflictingEcho
        );
    }
    if let Some(value) = meta.get("photoId").or_else(|| raw.get("photoId")) {
        let value = value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.as_u64().map(|v| v.to_string()))
            .ok_or(FailureReason::ConflictingEcho)?;
        ensure!(value == photo_id, FailureReason::ConflictingEcho);
    }
    Ok(())
}

/// Only an actual own `chat.photo` message can settle this typed ledger. The
/// placeholder body alone is never correlation evidence.
pub(super) fn reconcile_received(
    conn: &Connection,
    message: &mut Message,
    hint: Option<&str>,
    content: &MediaMessageContent,
) -> Result<bool> {
    if message.sender != message.account_id || content.kind() != MediaKind::Photo {
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
    let explicit=hint.map(|hint|conn.query_row("SELECT client_id,thread_id,state,server_id,media_metadata,photo_id,checksum,echo_received FROM photo_outbox WHERE account_id=?1 AND client_id=?2 AND media_metadata IS NOT NULL",params![message.account_id,hint],read).optional()).transpose()?.flatten();
    let bound=conn.query_row("SELECT client_id,thread_id,state,server_id,media_metadata,photo_id,checksum,echo_received FROM photo_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3",params![message.account_id,message.thread_id,message.id],read).optional()?;
    if explicit
        .as_ref()
        .zip(bound.as_ref())
        .is_some_and(|(a, b)| a.0 != b.0)
    {
        bail!(FailureReason::ConflictingEcho);
    }
    let Some((client, thread, state, server, raw, photo_id, checksum, received)) =
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
    let expected = MediaMessageContent::new(MediaKind::Photo, &serde_json::from_str(&raw)?)
        .map_err(|_| FailureReason::ConflictingMetadata)?;
    verify_echo(&expected, content, &photo_id, &checksum)?;
    let previous:Option<(String,String,i64)>=conn.query_row("SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![message.account_id,thread,message.id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    if let Some((owner, body, _)) = &previous {
        ensure!(
            owner == &message.account_id && body == MediaKind::Photo.legacy_placeholder(),
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
    conn.execute("UPDATE photo_outbox SET state='accepted',server_id=?3,server_ts=?4,echo_received=1 WHERE account_id=?1 AND client_id=?2",params![message.account_id,client,message.id,message.sent_at_ms])?;
    message.source_client_id = Some(client);
    message.reply_blocked = true;
    Ok(state != "accepted" || enriched || retimed)
}

struct FinalResource {
    thread: String,
    photo_id: String,
    checksum: String,
    parameters: Vec<u8>,
    content: MediaMessageContent,
}
fn final_resource(conn: &Connection, account: &str, client: u64) -> Result<FinalResource> {
    type Row = (
        String,
        String,
        u32,
        u32,
        u64,
        String,
        String,
        Vec<u8>,
        Vec<u8>,
        String,
    );
    let (thread,caption,width,height,size,checksum,photo_id,parameters,hash,raw):Row=conn.query_row(
        "SELECT thread_id,caption,width,height,size,checksum,photo_id,parameters,parameters_hash,media_metadata FROM photo_outbox WHERE account_id=?1 AND client_id=?2",
        params![account,client.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?)))?;
    ensure!(
        parameters.len() <= MAX_FINAL_BYTES
            && raw.len() <= MAX_FINAL_BYTES
            && hash.as_slice() == Sha256::digest(&parameters).as_slice(),
        "Photo resource changed"
    );
    let resource = PhotoResource {
        photo_id: photo_id.clone(),
        parameters: serde_json::from_slice(&parameters)?,
        content: MediaMessageContent::new(MediaKind::Photo, &serde_json::from_str(&raw)?)
            .map_err(|_| FailureReason::ConflictingMetadata)?,
    };
    ensure!(
        bound_resource(
            &PhotoSpec {
                caption,
                width,
                height,
                size,
                checksum: checksum.clone()
            },
            &thread,
            client,
            &resource,
            parameter_resolution(&resource.parameters)?,
            PhotoAlbumMember::from_message_parameters(&resource.parameters),
        )? == parameters,
        "Photo resource binding changed"
    );
    Ok(FinalResource {
        thread,
        photo_id,
        checksum,
        parameters,
        content: resource.content,
    })
}

fn check_reservations(
    conn: &Connection,
    account: &str,
    thread: &str,
    specs: &[PhotoSpec],
) -> Result<()> {
    let active:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM photo_outbox WHERE account_id=?1 AND state IN ('uploading','resource_ready','sending') UNION ALL SELECT 1 FROM attachment_outbox WHERE account_id=?1 AND state IN ('uploading','resource_ready','sending'))",[account],|row|row.get(0))?;
    ensure!(!active, "A photo upload is already active");
    let unresolved:i64=conn.query_row("SELECT count(*) FROM photo_outbox WHERE account_id=?1 AND state IN ('unknown','acknowledged')",[account],|row|row.get(0))?;
    ensure!(unresolved < 100, "Too many unresolved photo sends");
    for spec in specs {
        let duplicate:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM photo_outbox WHERE account_id=?1 AND thread_id=?2 AND width=?3 AND height=?4 AND checksum=?5 AND state IN ('unknown','acknowledged'))",params![account,thread,spec.width,spec.height,spec.checksum.to_ascii_lowercase()],|row|row.get(0))?;
        ensure!(!duplicate, "This photo has an unresolved send");
    }
    Ok(())
}

impl MessageStore {
    pub fn reserve_photo(
        &mut self,
        account: &str,
        thread: &str,
        spec: &PhotoSpec,
        now_ms: i64,
    ) -> Result<u64> {
        self.reserve_photo_resolution(account, thread, spec, PhotoResolution::Hd, now_ms)
    }
    pub(crate) fn reserve_photo_resolution(
        &mut self,
        account: &str,
        thread: &str,
        spec: &PhotoSpec,
        resolution: PhotoResolution,
        now_ms: i64,
    ) -> Result<u64> {
        Ok(self
            .reserve_photo_intent(account, thread, spec, resolution, None, now_ms)?
            .0)
    }
    #[cfg(feature = "messaging")]
    pub(crate) fn preflight_album(
        &self,
        account: &str,
        thread: &str,
        specs: &[PhotoSpec],
    ) -> Result<()> {
        plain_visible(self, account, thread)?;
        ensure!(
            (2..=MAX_ALBUM_PHOTOS).contains(&specs.len()),
            FailureReason::Validation
        );
        for spec in specs {
            spec.validate()?;
            ensure!(spec.caption.is_empty(), FailureReason::Validation);
        }
        check_reservations(&self.conn, account, thread, specs)
    }
    #[cfg(feature = "messaging")]
    pub(crate) fn reserve_album_photo(
        &mut self,
        account: &str,
        thread: &str,
        spec: &PhotoSpec,
        position: AlbumPosition,
        now_ms: i64,
    ) -> Result<(u64, PhotoAlbumMember)> {
        let (id, album) = self.reserve_photo_intent(
            account,
            thread,
            spec,
            PhotoResolution::Hd,
            Some(position),
            now_ms,
        )?;
        Ok((id, album.ok_or(FailureReason::Validation)?))
    }
    fn reserve_photo_intent(
        &mut self,
        account: &str,
        thread: &str,
        spec: &PhotoSpec,
        resolution: PhotoResolution,
        position: Option<AlbumPosition>,
        now_ms: i64,
    ) -> Result<(u64, Option<PhotoAlbumMember>)> {
        plain_visible(self, account, thread)?;
        spec.validate()?;
        ensure!(now_ms > 0, "Invalid photo time");
        let checksum = spec.checksum.to_ascii_lowercase();
        let tx = self.conn.transaction()?;
        check_reservations(&tx, account, thread, std::slice::from_ref(spec))?;
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
        ensure!(next <= 9_007_199_254_740_991, "Photo client ID exhausted");
        tx.execute("INSERT INTO outgoing_counters VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",params![account,next])?;
        let album = position
            .map(|position| -> Result<_> {
                ensure!(
                    (position.index == 0) == position.layout_id.is_none()
                        && position.count <= MAX_ALBUM_PHOTOS as u32
                        && resolution == PhotoResolution::Hd
                        && spec.caption.is_empty(),
                    FailureReason::Validation
                );
                if let Some(layout_id) = position.layout_id {
                    let accepted: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM photo_outbox WHERE account_id=?1 AND thread_id=?2 AND client_id=?3 AND state='accepted')",
                        params![account, thread, layout_id.to_string()],
                        |row| row.get(0),
                    )?;
                    ensure!(accepted, FailureReason::ConflictingMetadata);
                    let first = final_resource(&tx, account, layout_id)?;
                    ensure!(
                        first.content.album_member()
                            == PhotoAlbumMember::new(layout_id, 0, position.count),
                        FailureReason::ConflictingMetadata
                    );
                }
                PhotoAlbumMember::new(
                    position.layout_id.unwrap_or(next as u64),
                    position.index,
                    position.count,
                )
                .ok_or_else(|| FailureReason::Validation.into())
            })
            .transpose()?;
        let album_marker = album.map(album_reservation);
        let intent = album_marker
            .as_deref()
            .or_else(|| (resolution == PhotoResolution::Original).then_some(ORIGINAL_RESERVATION));
        tx.execute("INSERT INTO photo_outbox(account_id,client_id,thread_id,caption,width,height,size,checksum,created_at_ms,state,parameters) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'uploading',?10)",params![account,next.to_string(),thread,spec.caption,spec.width,spec.height,spec.size as i64,checksum,now_ms,intent])?;
        tx.commit()?;
        Ok((next as u64, album))
    }
    pub fn photo_state(&self, account: &str, client: u64) -> Result<Option<PhotoState>> {
        validate_id(account)?;
        let state: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM photo_outbox WHERE account_id=?1 AND client_id=?2",
                params![account, client.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        state.as_deref().map(PhotoState::parse).transpose()
    }
    pub fn photo_resource_ready(
        &mut self,
        account: &str,
        client: u64,
        resource: &PhotoResource,
    ) -> Result<()> {
        validate_id(account)?;
        type Row = (
            String,
            String,
            u32,
            u32,
            u64,
            String,
            String,
            Option<Vec<u8>>,
        );
        let (thread,caption,width,height,size,checksum,state,old):Row=self.conn.query_row("SELECT thread_id,caption,width,height,size,checksum,state,parameters FROM photo_outbox WHERE account_id=?1 AND client_id=?2",params![account,client.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?)))?;
        plain_visible(self, account, &thread)?;
        let (resolution, album) = match (state.as_str(), old.as_deref()) {
            ("uploading", Some(marker)) if marker == ORIGINAL_RESERVATION => {
                (PhotoResolution::Original, None)
            }
            ("uploading", Some(marker)) => (
                PhotoResolution::Hd,
                Some(parse_album_reservation(marker).ok_or(FailureReason::ConflictingMetadata)?),
            ),
            ("uploading", None) => (PhotoResolution::Hd, None),
            ("resource_ready", Some(previous)) => {
                let previous = serde_json::from_slice(previous)?;
                (
                    parameter_resolution(&previous)?,
                    PhotoAlbumMember::from_message_parameters(&previous),
                )
            }
            _ => bail!(FailureReason::ConflictingMetadata),
        };
        let bytes = bound_resource(
            &PhotoSpec {
                caption,
                width,
                height,
                size,
                checksum,
            },
            &thread,
            client,
            resource,
            resolution,
            album,
        )?;
        let hash = Sha256::digest(&bytes);
        if state == "resource_ready" {
            ensure!(
                old.as_deref() == Some(bytes.as_slice()),
                "Photo resource is immutable"
            );
            return Ok(());
        }
        ensure!(state == "uploading", "Photo is not awaiting a resource");
        ensure!(self.conn.execute("UPDATE photo_outbox SET state='resource_ready',photo_id=?3,parameters=?4,parameters_hash=?5,media_metadata=?6 WHERE account_id=?1 AND client_id=?2 AND state='uploading'",params![account,client.to_string(),resource.photo_id,bytes,hash.as_slice(),resource.content.as_json()])?==1,"Photo resource transition conflict");
        Ok(())
    }
    /// The actor must freshly verify its current auth/encryption generation first.
    /// A durable one-shot claim is required immediately before the final HTTP POST.
    pub fn begin_photo_dispatch(&mut self, account: &str, client: u64) -> Result<Vec<u8>> {
        validate_id(account)?;
        let thread: String = self.conn.query_row(
            "SELECT thread_id FROM photo_outbox WHERE account_id=?1 AND client_id=?2",
            params![account, client.to_string()],
            |row| row.get(0),
        )?;
        plain_visible(self, account, &thread)?;
        let tx = self.conn.transaction()?;
        let resource = final_resource(&tx, account, client)?;
        ensure!(resource.thread == thread, "Photo scope changed");
        ensure!(tx.execute("UPDATE photo_outbox SET state='sending' WHERE account_id=?1 AND client_id=?2 AND state='resource_ready'",params![account,client.to_string()])?==1,"Photo dispatch conflict");
        tx.commit()?;
        Ok(resource.parameters)
    }
    pub fn interrupt_photo(&self, account: &str, client: u64) -> Result<()> {
        validate_id(account)?;
        self.conn.execute("UPDATE photo_outbox SET state='interrupted' WHERE account_id=?1 AND client_id=?2 AND state IN ('uploading','resource_ready')",params![account,client.to_string()])?;
        Ok(())
    }
    pub fn fail_photo_dispatch(&self, account: &str, client: u64, unknown: bool) -> Result<()> {
        validate_id(account)?;
        self.conn.execute("UPDATE photo_outbox SET state=?3 WHERE account_id=?1 AND client_id=?2 AND state='sending'",params![account,client.to_string(),if unknown{"unknown"}else{"rejected"}])?;
        Ok(())
    }
    pub fn recover_photos(&self, account: &str) -> Result<()> {
        validate_id(account)?;
        self.conn.execute("UPDATE photo_outbox SET state=CASE WHEN state='sending' THEN 'unknown' ELSE 'interrupted' END WHERE account_id=?1 AND state IN ('uploading','resource_ready','sending')",[account])?;
        Ok(())
    }
    pub fn record_photo_ack(
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
            "Invalid photo acknowledgement time"
        );
        ensure!(self.conn.execute("UPDATE photo_outbox SET state=CASE WHEN state='accepted' THEN state ELSE 'acknowledged' END,server_id=?3,server_ts=COALESCE(server_ts,?4) WHERE account_id=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','accepted') AND (server_id IS NULL OR server_id=?3)
          AND NOT EXISTS(SELECT 1 FROM forward_outbox WHERE account_id=?1 AND thread_id=photo_outbox.thread_id AND server_id=?3)
          AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=photo_outbox.thread_id AND message_id=?3 AND kind='forwarded')",params![account,client.to_string(),server,timestamp])?==1,"Photo acknowledgement conflict");
        Ok(())
    }
    pub fn accept_photo(&mut self, account: &str, client: u64) -> Result<(Message, Conversation)> {
        validate_id(account)?;
        let tx = self.conn.transaction()?;
        let (thread,server,created,timestamp,raw):(String,String,i64,Option<i64>,String)=tx.query_row("SELECT thread_id,server_id,created_at_ms,server_ts,media_metadata FROM photo_outbox WHERE account_id=?1 AND client_id=?2 AND state IN ('acknowledged','accepted')",params![account,client.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
        ensure!(raw.len() <= MAX_FINAL_BYTES, "Photo echo exceeds limit");
        ensure!(
            !super::forward::server_reserved(&tx, account, &thread, &server)?,
            FailureReason::ConflictingEcho
        );
        let resource = final_resource(&tx, account, client)?;
        ensure!(
            resource.thread == thread && resource.content.as_json() == raw,
            "Photo acknowledgement scope changed"
        );
        let mut content = resource.content;
        if let Some(wire) = super::message_media::load(&tx, account, &thread, &server)? {
            verify_echo(&content, &wire, &resource.photo_id, &resource.checksum)?;
            content = wire;
        }
        let existing:Option<i64>=tx.query_row("SELECT sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![account,thread,server],|row|row.get(0)).optional()?;
        let mut message = Message {
            account_id: account.into(),
            thread_id: thread.clone(),
            id: server,
            sender: account.into(),
            text: MediaKind::Photo.legacy_placeholder().into(),
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
        ensure!(tx.execute("UPDATE photo_outbox SET state='accepted' WHERE account_id=?1 AND client_id=?2 AND state IN ('acknowledged','accepted')",params![account,client.to_string()])?==1,"Photo acceptance conflict");
        let conversation = tx.query_row(
            CONVERSATION_SQL,
            params![account, thread, MAX_PREVIEW_CHARS as i64],
            conversation_row,
        )?;
        tx.commit()?;
        Ok((message, conversation))
    }
    pub fn reconcile_photos(&mut self, account: &str) -> Result<()> {
        validate_id(account)?;
        let ids = {
            let mut query=self.conn.prepare("SELECT client_id FROM photo_outbox WHERE account_id=?1 AND state='acknowledged' ORDER BY client_id LIMIT 100")?;
            let values = query.query_map([account], |row| row.get::<_, String>(0))?;
            values.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in ids {
            self.accept_photo(account, id.parse()?)?;
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
    fn album_member_metadata_is_bound_through_resource_and_receipt_recovery() {
        let previous = MessageStore::in_memory().unwrap();
        previous.conn.execute_batch(
            "PRAGMA user_version=20; DELETE FROM store_markers WHERE name='media_photo_album_v1';",
        ).unwrap();
        let mut store = MessageStore::initialize(previous.conn).unwrap();
        assert_eq!(
            store
                .conn
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            crate::store::SCHEMA_VERSION
        );
        let spec = PhotoSpec {
            caption: String::new(),
            width: 80,
            height: 60,
            size: 1000,
            checksum: "a".repeat(32),
        };
        store
            .preflight_album("11", "d:22", &[spec.clone(), spec.clone()])
            .unwrap();
        let (first, member) = store
            .reserve_album_photo(
                "11",
                "d:22",
                &spec,
                AlbumPosition {
                    layout_id: None,
                    index: 0,
                    count: 3,
                },
                1000,
            )
            .unwrap();
        assert_eq!(member.layout_id(), first);
        let resource = |client, member: PhotoAlbumMember| {
            let url = "https://files.zdn.vn/album.jpg";
            let mut parameters = json!({
                "photoId":"synthetic-photo","clientId":client,"desc":"",
                "width":80,"height":60,"rawUrl":url,"thumbUrl":url,"hdUrl":url,
                "normalUrl":url,"language":"VN","imei":"synthetic-device","ttl":0,"toid":"22"
            });
            member
                .apply_to_message_parameters(&mut parameters, true)
                .unwrap();
            let content = MediaMessageContent::new(
                MediaKind::Photo,
                &json!({
                    "title":"","description":"","href":url,"thumb":url,
                    "params":{"width":80,"height":60,"hd":url,"rawUrl":url}
                }),
            )
            .unwrap();
            PhotoResource {
                photo_id: "synthetic-photo".into(),
                parameters,
                content: member.apply_to_media_content(&content).unwrap(),
            }
        };
        let other = PhotoAlbumMember::new(first, 1, 3).unwrap();
        assert!(store
            .photo_resource_ready("11", first, &resource(first, other))
            .is_err());
        let rendered = resource(first, member);
        store.photo_resource_ready("11", first, &rendered).unwrap();
        let canonical = store.begin_photo_dispatch("11", first).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&canonical).unwrap(),
            rendered.parameters
        );
        store
            .record_photo_ack("11", first, "901", Some(1020))
            .unwrap();
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        reopened.recover_photos("11").unwrap();
        reopened.reconcile_photos("11").unwrap();
        let messages = reopened.page("11", "d:22", None, 10).unwrap();
        assert_eq!(messages.len(), 1);
        let Some(MediaDescriptor::Photo(photo)) = &messages[0].media else {
            panic!("Expected photo")
        };
        assert_eq!(photo.album, Some(member));
        assert!(reopened
            .reserve_album_photo(
                "11",
                "d:22",
                &spec,
                AlbumPosition {
                    layout_id: Some(first),
                    index: 1,
                    count: 4
                },
                1030
            )
            .is_err());
        let (second, second_member) = reopened
            .reserve_album_photo(
                "11",
                "d:22",
                &spec,
                AlbumPosition {
                    layout_id: Some(first),
                    index: 1,
                    count: 3,
                },
                1030,
            )
            .unwrap();
        assert_eq!(second_member, other);
        reopened
            .photo_resource_ready("11", second, &resource(second, other))
            .unwrap();
        reopened.begin_photo_dispatch("11", second).unwrap();
        reopened.recover_photos("11").unwrap();
        assert_eq!(
            reopened.photo_state("11", second).unwrap(),
            Some(PhotoState::Unknown)
        );
        assert!(reopened
            .preflight_album("11", "d:22", &[spec.clone(), spec])
            .is_err());
    }
    #[test]
    fn final_photo_dispatch_rechecks_private_visibility_and_encryption() {
        for privacy in 0..3 {
            let mut store = MessageStore::in_memory().unwrap();
            let spec = PhotoSpec {
                caption: "Ảnh tổng hợp".into(),
                width: 800,
                height: 600,
                size: 1000,
                checksum: "a".repeat(32),
            };
            let id = store.reserve_photo("11", "d:22", &spec, 1000).unwrap();
            let url = "https://files.zdn.vn/photo.jpg";
            let resource=PhotoResource{photo_id:"synthetic-photo".into(),parameters:json!({"photoId":"synthetic-photo","clientId":id,"desc":spec.caption,"width":800,"height":600,"rawUrl":url,"thumbUrl":url,"hdUrl":url,"normalUrl":url,"language":"VN","imei":"synthetic-device","ttl":0,"toid":"22"}),content:MediaMessageContent::new(MediaKind::Photo,&json!({"title":"","description":spec.caption,"href":url,"thumb":url,"params":{"width":800,"height":600,"hd":url,"rawUrl":url}})).unwrap()};
            store.photo_resource_ready("11", id, &resource).unwrap();
            match privacy {
                0 => store.begin_live_privacy("11").unwrap(),
                1 => store
                    .replace_live_privacy("11", &["d:22".into()], None)
                    .unwrap(),
                _ => {
                    store
                        .record_encrypted_threads("11", &["22".into()])
                        .unwrap();
                }
            }
            assert!(store.begin_photo_dispatch("11", id).is_err());
            assert_eq!(
                store.photo_state("11", id).unwrap(),
                Some(PhotoState::ResourceReady)
            );
        }
    }

    #[test]
    fn original_photo_mode_is_bound_before_upload_and_through_ack_recovery() {
        let mut store = MessageStore::in_memory().unwrap();
        let spec = PhotoSpec {
            caption: "Ảnh tổng hợp".into(),
            width: 80,
            height: 60,
            size: 1000,
            checksum: "a".repeat(32),
        };
        let raw_url = "https://files.zdn.vn/original.jpg";
        let thumb_url = "https://files.zdn.vn/thumb.jpg";
        let hd_url = "https://files.zdn.vn/hd.jpg";
        let resource = |client, original| {
            let mut parameters = json!({
                "photoId":"synthetic-photo","clientId":client,"desc":spec.caption,
                "width":80,"height":60,"rawUrl":raw_url,"thumbUrl":thumb_url,"hdUrl":hd_url,
                "normalUrl":raw_url,"language":"VN","imei":"synthetic-device","ttl":0,"toid":"22"
            });
            let mut content = json!({
                "title":"","description":spec.caption,"href":raw_url,"thumb":thumb_url,
                "params":{"width":80,"height":60,"hd":hd_url,"rawUrl":raw_url}
            });
            if original {
                parameters["jcp"] = json!("{\"is_original\":1}");
                content["params"]["is_original"] = json!(1);
            }
            PhotoResource {
                photo_id: "synthetic-photo".into(),
                parameters,
                content: MediaMessageContent::new(MediaKind::Photo, &content).unwrap(),
            }
        };
        let client = store.reserve_photo("11", "d:22", &spec, 1000).unwrap();
        assert!(store
            .photo_resource_ready("11", client, &resource(client, true))
            .is_err());
        store.interrupt_photo("11", client).unwrap();

        let client = store
            .reserve_photo_resolution("11", "d:22", &spec, PhotoResolution::Original, 1010)
            .unwrap();
        let hd = resource(client, false);
        let original = resource(client, true);
        assert!(store.photo_resource_ready("11", client, &hd).is_err());
        assert!(verify_echo(
            &original.content,
            &hd.content,
            &original.photo_id,
            &spec.checksum
        )
        .is_err());
        let mut wrong_meta: Value = serde_json::from_str(original.content.as_json()).unwrap();
        wrong_meta["params"]["is_original"] = json!(0);
        assert!(store
            .photo_resource_ready(
                "11",
                client,
                &PhotoResource {
                    photo_id: original.photo_id.clone(),
                    parameters: original.parameters.clone(),
                    content: MediaMessageContent::new(MediaKind::Photo, &wrong_meta).unwrap(),
                }
            )
            .is_err());
        store.photo_resource_ready("11", client, &original).unwrap();
        let parameters = store.begin_photo_dispatch("11", client).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&parameters).unwrap(),
            original.parameters
        );
        store
            .record_photo_ack("11", client, "901", Some(1020))
            .unwrap();
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        reopened.reconcile_photos("11").unwrap();
        let messages = reopened.page("11", "d:22", None, 2).unwrap();
        let Some(MediaDescriptor::Photo(photo)) = &messages[0].media else {
            panic!("expected a persisted photo");
        };
        assert_eq!(photo.original_url.as_deref(), Some(raw_url));
        assert_eq!(
            reopened.photo_state("11", client).unwrap(),
            Some(PhotoState::Accepted)
        );
    }
}
