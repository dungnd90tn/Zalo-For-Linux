//! One bounded cooperative upload per account; only the actor grants final dispatch.
use super::*;
use crate::{
    domain::Attachment,
    protocol::{photo as wire_photo, upload as wire_upload},
    store::{
        attachments::{AttachmentResource, AttachmentSpec},
        photos::{AlbumPosition, PhotoResource, PhotoSpec},
    },
};
use md5::{Digest, Md5};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
};

#[derive(Clone, Copy)]
struct Progress {
    phase: FileTransferPhase,
    uploaded: u64,
    total: u64,
}
struct Authorization {
    final_dispatch: bool,
    id: u64,
    reply: oneshot::Sender<Result<Option<Vec<u8>>>>,
}
enum UploadInput {
    File(Attachment),
    Photo {
        photo: PreparedPhoto,
        caption: String,
    },
}
enum UploadJob {
    Single(UploadInput),
    Album(Vec<PreparedPhoto>),
}
impl UploadJob {
    fn kind(&self) -> TransferKind {
        match self {
            Self::Single(input) => input.kind(),
            Self::Album(_) => TransferKind::Album,
        }
    }
    fn size(&self) -> u64 {
        match self {
            Self::Single(input) => input.size(),
            Self::Album(photos) => photos.iter().map(PreparedPhoto::size_bytes).sum(),
        }
    }
}
impl UploadInput {
    fn kind(&self) -> TransferKind {
        match self {
            Self::File(_) => TransferKind::File,
            Self::Photo { .. } => TransferKind::Photo,
        }
    }
    fn size(&self) -> u64 {
        match self {
            Self::File(file) => file.size_bytes,
            Self::Photo { photo, .. } => photo.size_bytes(),
        }
    }
    fn name(&self) -> String {
        match self {
            Self::File(file) => file.name.clone(),
            Self::Photo { .. } => "photo.jpg".into(),
        }
    }
}
struct Running {
    kind: TransferKind,
    thread: String,
    request_id: u64,
    cancel: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<Outcome>,
    progress: watch::Receiver<Progress>,
    requests: mpsc::Receiver<Authorization>,
    cache: Arc<Mutex<ResourceCache>>,
    photo_cache: Arc<Mutex<ResourceCache<wire_photo::PhotoUrls>>>,
    emitted: Option<(FileTransferPhase, u64)>,
    pending: Option<PendingAuthorization>,
}
enum Verification {
    Direct {
        request_id: String,
        deadline: tokio::time::Instant,
        total_deadline: tokio::time::Instant,
        seen: std::collections::HashSet<String>,
        pages: usize,
    },
    Group(tokio::task::JoinHandle<Result<()>>),
}
struct PendingAuthorization {
    request: Authorization,
    target: Target,
    verification: Verification,
    result: Option<Result<()>>,
}
impl Drop for PendingAuthorization {
    fn drop(&mut self) {
        if let Verification::Group(task) = &self.verification {
            task.abort();
        }
    }
}
struct Outcome {
    phase: FileTransferPhase,
    accepted: Vec<(Message, Conversation)>,
    message: Option<String>,
}
struct SingleOutcome {
    phase: FileTransferPhase,
    accepted: Option<(Message, Conversation)>,
    message: Option<&'static str>,
}
struct ResourceCache<T = String> {
    entries: VecDeque<(String, Option<T>, tokio::time::Instant)>,
}
impl<T> Default for ResourceCache<T> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }
}
impl<T: Clone + PartialEq> ResourceCache<T> {
    fn insert(&mut self, id: String, url: T) {
        self.entries.retain(|(_, _, at)| {
            at.elapsed() < Duration::from_secs(wire_upload::RESOURCE_TTL_SECS)
        });
        if let Some((_, old, _)) = self.entries.iter_mut().find(|(key, _, _)| key == &id) {
            // Conflicting authenticated resource completions cannot authorize final dispatch.
            if old.as_ref() != Some(&url) {
                *old = None;
            }
            return;
        }
        if self.entries.len() == wire_upload::MAX_CONTROL_CACHE {
            self.entries.pop_front();
        }
        self.entries
            .push_back((id, Some(url), tokio::time::Instant::now()));
    }
    fn get(&self, id: &str) -> Option<T> {
        self.entries
            .iter()
            .find(|(key, _, at)| {
                key == id && at.elapsed() < Duration::from_secs(wire_upload::RESOURCE_TTL_SECS)
            })
            .and_then(|(_, url, _)| url.clone())
    }
}
static NEXT_WORKER_LEASE: AtomicU64 = AtomicU64::new(1);
fn next_worker_lease() -> u64 {
    loop {
        let current = NEXT_WORKER_LEASE.load(Ordering::Acquire);
        let Some(next) = current.checked_add(1) else {
            return 0;
        };
        if current == 0 {
            return 0;
        }
        if NEXT_WORKER_LEASE
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return current;
        }
    }
}
pub(super) struct Uploads {
    running: Option<Running>,
    lease: u64,
    original_probe: Option<tokio::task::JoinHandle<bool>>,
    original_epoch: Option<tokio::time::Instant>,
    original_available: bool,
}
impl Default for Uploads {
    fn default() -> Self {
        Self {
            running: None,
            lease: next_worker_lease(),
            original_probe: None,
            original_epoch: None,
            original_available: false,
        }
    }
}
impl Drop for Uploads {
    fn drop(&mut self) {
        if let Some(probe) = &self.original_probe {
            probe.abort();
        }
        if let Some(job) = &self.running {
            job.cancel.store(true, Ordering::Release);
            job.task.abort();
        }
    }
}
impl Uploads {
    pub fn lease(&self) -> u64 {
        self.lease
    }
    pub fn active(&self) -> bool {
        self.running.is_some()
    }
    pub fn album_max_count(&self, session: &VerifiedSession) -> usize {
        if self.lease != 0 && session.config().photo_upload_route.is_some() {
            (session.config().photo_album_max_count as usize).min(MAX_ALBUM_PHOTOS)
        } else {
            0
        }
    }
    pub fn original_limits(
        &self,
        session: &VerifiedSession,
    ) -> Option<crate::protocol::bootstrap::PhotoLimits> {
        session
            .config()
            .original_photo_limits
            .filter(|_| self.original_available)
    }
    pub async fn check_original_entitlement(
        &mut self,
        epoch: Option<tokio::time::Instant>,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
        events: &Events,
    ) -> Result<()> {
        if self.original_epoch == epoch || session.config().original_photo_limits.is_none() {
            return Ok(());
        }
        self.original_epoch = epoch;
        if let Some(probe) = self.original_probe.take() {
            probe.abort();
        }
        if self.original_available {
            self.set_original_entitlement(false, session, events)
                .await?;
        }
        if self.lease == 0 || epoch.is_none() {
            return Ok(());
        }
        let (session, client) = (session.clone(), client.clone());
        self.original_probe = Some(tokio::spawn(async move {
            original_photo::entitled(&client, &session)
                .await
                .unwrap_or(false)
        }));
        Ok(())
    }
    pub async fn original_entitlement(&mut self) -> bool {
        let Some(probe) = self.original_probe.as_mut() else {
            return std::future::pending().await;
        };
        let paid = probe.await.unwrap_or(false);
        self.original_probe = None;
        paid
    }
    pub async fn set_original_entitlement(
        &mut self,
        paid: bool,
        session: &VerifiedSession,
        events: &Events,
    ) -> Result<()> {
        self.original_available = paid;
        events
            .emit(LiveEvent::PhotoCapability {
                account_id: events.account.clone(),
                available: session.config().photo_upload_route.is_some() && self.lease() != 0,
                original_available: paid,
                album_max_count: self.album_max_count(session),
            })
            .await
    }
    pub fn control(&self, frame: &DecodedFrame) {
        if frame.cmd != 601 {
            return;
        }
        if let Some(job) = &self.running {
            match job.kind {
                TransferKind::File => {
                    if let Ok(mut cache) = job.cache.lock() {
                        for (id, url) in wire_upload::file_done(&frame.payload) {
                            cache.insert(id, url);
                        }
                    }
                }
                TransferKind::Photo | TransferKind::Album => {
                    if let Ok(mut cache) = job.photo_cache.lock() {
                        for (id, urls) in wire_photo::photo_success(&frame.payload) {
                            cache.insert(id, urls);
                        }
                    }
                }
            }
        }
    }
    pub async fn observe(
        &mut self,
        frame: &DecodedFrame,
        connection: &mut Connection,
    ) -> Result<()> {
        self.control(frame);
        let Some(pending) = self.running.as_mut().and_then(|job| job.pending.as_mut()) else {
            return Ok(());
        };
        if pending.result.is_some() {
            return Ok(());
        }
        let Verification::Direct {
            request_id,
            deadline,
            total_deadline,
            seen,
            pages,
        } = &mut pending.verification
        else {
            return Ok(());
        };
        if [551, 552, 535, 543, 544, 548, 561, 580, 581, 582].contains(&frame.cmd) {
            pending.result = Some(Err(anyhow::anyhow!(
                "Encryption state changed during upload capability probe"
            )));
            return Ok(());
        }
        if frame.cmd != 563 {
            return Ok(());
        }
        match wire::legacy_direct_allowed_detailed(
            &frame.payload,
            request_id,
            &pending.target.remote_id,
        ) {
            Ok((true, Some(next)))
                if *pages < 100
                    && seen.insert(next.clone())
                    && tokio::time::Instant::now() < *total_deadline =>
            {
                *request_id = connection.request();
                *pages += 1;
                *deadline =
                    (tokio::time::Instant::now() + Duration::from_secs(10)).min(*total_deadline);
                if let Err(error) = connection
                    .send(
                        563,
                        0,
                        json!({"reqId":request_id,"data":{"reqIid":1,"first":false,"lastId":next}}),
                    )
                    .await
                {
                    pending.result = Some(Err(error));
                }
            }
            Ok((true, None)) => pending.result = Some(Ok(())),
            Ok(_) => {
                pending.result = Some(Err(anyhow::anyhow!(
                    "Encrypted or incomplete upload capability"
                )))
            }
            Err(error) => pending.result = Some(Err(error.into())),
        }
        Ok(())
    }
    pub fn invalidate(&self) {
        if let Some(job) = &self.running {
            job.cancel.store(true, Ordering::Release);
        }
    }
    pub async fn command(
        &mut self,
        command: LiveCommand,
        profile: &Arc<Profile>,
        session: &Arc<VerifiedSession>,
        db: &Database,
        client: &reqwest::Client,
        events: &Events,
    ) -> Result<Option<LiveCommand>> {
        match command {
            LiveCommand::CancelFile { request_id } => {
                if let Some(job) = &self.running {
                    if job.kind != TransferKind::Album && job.request_id == request_id {
                        job.cancel.store(true, Ordering::Release);
                    }
                }
                Ok(None)
            }
            LiveCommand::CancelAlbum { thread, request_id } => {
                if let Some(job) = &self.running {
                    if job.kind == TransferKind::Album
                        && job.thread == thread
                        && job.request_id == request_id
                    {
                        job.cancel.store(true, Ordering::Release);
                    }
                }
                Ok(None)
            }
            LiveCommand::SendFile {
                thread,
                attachment,
                request_id,
                ..
            } => {
                self.start(
                    thread,
                    request_id,
                    UploadJob::Single(UploadInput::File(attachment)),
                    profile,
                    session,
                    db,
                    client,
                    events,
                )
                .await?;
                Ok(None)
            }
            LiveCommand::SendPhoto {
                thread,
                photo,
                caption,
                request_id,
                ..
            } => {
                let limits = match photo.resolution() {
                    PhotoResolution::Hd => session
                        .config()
                        .photo_upload_route
                        .map(|_| session.config().photo_limits),
                    PhotoResolution::Original => self.original_limits(session),
                };
                if !photo.permits(&events.account, &thread, self.lease)
                    || !limits.is_some_and(|limits| photo.fits_limits(limits))
                    || caption.len() > crate::domain::MAX_TEXT_BYTES
                {
                    emit(
                        events,
                        &thread,
                        request_id,
                        TransferKind::Photo,
                        Progress {
                            phase: FileTransferPhase::Failed,
                            uploaded: 0,
                            total: photo.size_bytes(),
                        },
                        Some("Bản xem trước đã hết hiệu lực; hãy chọn lại ảnh."),
                    )
                    .await?;
                    return Ok(None);
                }
                self.start(
                    thread,
                    request_id,
                    UploadJob::Single(UploadInput::Photo {
                        photo,
                        caption: crate::domain::canonical_plain_text(&caption).to_owned(),
                    }),
                    profile,
                    session,
                    db,
                    client,
                    events,
                )
                .await?;
                Ok(None)
            }
            LiveCommand::SendAlbum {
                thread,
                photos,
                request_id,
                ..
            } => {
                let limits = session.config().photo_limits;
                if !(2..=self.album_max_count(session)).contains(&photos.len())
                    || photos.iter().any(|photo| {
                        photo.resolution() != PhotoResolution::Hd
                            || !photo.permits(&events.account, &thread, self.lease)
                            || !photo.fits_limits(limits)
                    })
                {
                    emit(
                        events,
                        &thread,
                        request_id,
                        TransferKind::Album,
                        Progress {
                            phase: FileTransferPhase::Failed,
                            uploaded: 0,
                            total: photos.iter().map(PreparedPhoto::size_bytes).sum(),
                        },
                        Some("Bản xem trước album đã hết hiệu lực. Hãy chọn lại ảnh."),
                    )
                    .await?;
                    return Ok(None);
                }
                self.start(
                    thread,
                    request_id,
                    UploadJob::Album(photos),
                    profile,
                    session,
                    db,
                    client,
                    events,
                )
                .await?;
                Ok(None)
            }
            command => Ok(Some(command)),
        }
    }
    #[allow(clippy::too_many_arguments)]
    async fn start(
        &mut self,
        thread: String,
        request_id: u64,
        input: UploadJob,
        profile: &Arc<Profile>,
        session: &Arc<VerifiedSession>,
        db: &Database,
        client: &reqwest::Client,
        events: &Events,
    ) -> Result<()> {
        let kind = input.kind();
        let total = input.size();
        if self.running.is_some()
            || root_for(session, "file").is_err()
            || !events.privacy.read().is_ok_and(|p| p.permits(&thread))
        {
            emit(
                events,
                &thread,
                request_id,
                kind,
                Progress {
                    phase: FileTransferPhase::Failed,
                    uploaded: 0,
                    total,
                },
                Some("Chưa thể gửi: kiểm tra kết nối, quyền gửi hoặc tác vụ đang tải."),
            )
            .await?;
            return Ok(());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let (progress_tx, progress) = watch::channel(Progress {
            phase: FileTransferPhase::Preparing,
            uploaded: 0,
            total,
        });
        let (requests_tx, requests) = mpsc::channel(1);
        let cache = Arc::new(Mutex::new(ResourceCache::default()));
        let photo_cache = Arc::new(Mutex::new(ResourceCache::default()));
        let task = tokio::spawn(run(
            profile.clone(),
            session.clone(),
            db.handle()?,
            client.clone(),
            thread.clone(),
            input,
            cancel.clone(),
            progress_tx,
            requests_tx,
            cache.clone(),
            photo_cache.clone(),
        ));
        self.running = Some(Running {
            kind,
            thread,
            request_id,
            cancel,
            task,
            progress,
            requests,
            cache,
            photo_cache,
            emitted: None,
            pending: None,
        });
        Ok(())
    }
    pub async fn poll(
        &mut self,
        connection: &mut Option<Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        let Some(job) = self.running.as_mut() else {
            return Ok(());
        };
        if !events.privacy.read().is_ok_and(|p| p.permits(&job.thread))
            || !connection
                .as_ref()
                .is_some_and(|c| c.decoder.authenticated())
        {
            job.cancel.store(true, Ordering::Release);
        }
        if job.pending.is_none() {
            if let Ok(request) = job.requests.try_recv() {
                if job.cancel.load(Ordering::Acquire) {
                    let _ = request.reply.send(Err(anyhow::anyhow!("Upload cancelled")));
                } else {
                    let target = Target::from_thread(&job.thread)?;
                    let mut probe_error = None;
                    let verification =
                        match target.kind {
                            ThreadKind::Direct => {
                                let c = connection
                                    .as_mut()
                                    .ok_or_else(|| anyhow::anyhow!("offline"))?;
                                let request_id = c.request();
                                if let Err(error) = c.send(
                                563,
                                0,
                                json!({"reqId":request_id,"data":{"reqIid":1,"first":true}}),
                            )
                            .await { probe_error = Some(error); }
                                Verification::Direct {
                                    request_id,
                                    deadline: tokio::time::Instant::now() + Duration::from_secs(10),
                                    total_deadline: tokio::time::Instant::now()
                                        + Duration::from_secs(15),
                                    seen: Default::default(),
                                    pages: 1,
                                }
                            }
                            ThreadKind::Group => {
                                let client = client.clone();
                                let session = session.clone();
                                let target = target.clone();
                                Verification::Group(tokio::spawn(async move {
                                    verify_group(&client, &session, &target).await.map(|_| ())
                                }))
                            }
                        };
                    job.pending = Some(PendingAuthorization {
                        request,
                        target,
                        verification,
                        result: probe_error.map(Err),
                    });
                }
            }
        }
        if let Some(pending) = job.pending.as_mut() {
            if job.cancel.load(Ordering::Acquire) {
                pending.result = Some(Err(anyhow::anyhow!("Upload cancelled")));
            } else {
                match &mut pending.verification {
                    Verification::Direct { deadline, .. }
                        if *deadline <= tokio::time::Instant::now() =>
                    {
                        pending.result = Some(Err(anyhow::anyhow!("Upload capability timed out")))
                    }
                    Verification::Group(task) if task.is_finished() => {
                        pending.result =
                            Some(task.await.unwrap_or_else(|_| {
                                Err(anyhow::anyhow!("Capability task stopped"))
                            }))
                    }
                    _ => {}
                }
            }
        }
        if job.pending.as_ref().is_some_and(|p| p.result.is_some()) {
            let mut pending = job.pending.take().expect("completed authorization");
            let result = async {
                pending.result.take().expect("completed probe")?;
                anyhow::ensure!(
                    !job.cancel.load(Ordering::Acquire)
                        && events.privacy.read().is_ok_and(|p| p.permits(&job.thread))
                        && connection
                            .as_ref()
                            .is_some_and(|c| c.decoder.authenticated()),
                    "Upload cancelled"
                );
                if pending.request.final_dispatch {
                    let account = events.account.clone();
                    let id = pending.request.id;
                    let kind = job.kind;
                    Ok(Some(
                        db.call_with(StorageOperation::PrepareOutgoing, move |s| match kind {
                            TransferKind::File => s.begin_attachment_dispatch(&account, id),
                            TransferKind::Photo | TransferKind::Album => {
                                s.begin_photo_dispatch(&account, id)
                            }
                        })
                        .await?,
                    ))
                } else {
                    Ok(None)
                }
            }
            .await;
            // Move the sender out through replacement because PendingAuthorization aborts its group task on drop.
            let (dummy, _) = oneshot::channel();
            let reply = std::mem::replace(&mut pending.request.reply, dummy);
            let _ = reply.send(result);
        }
        let progress = *job.progress.borrow_and_update();
        if job.emitted != Some((progress.phase, progress.uploaded)) {
            emit(
                events,
                &job.thread,
                job.request_id,
                job.kind,
                progress,
                None,
            )
            .await?;
            job.emitted = Some((progress.phase, progress.uploaded));
        }
        if job.task.is_finished() {
            let job = self.running.take().expect("finished upload");
            let outcome = job.task.await.unwrap_or(Outcome {
                phase: FileTransferPhase::Unknown,
                accepted: Vec::new(),
                message: Some("Tác vụ gửi đã dừng. Kiểm tra hội thoại trước khi gửi lại.".into()),
            });
            for (message, conversation) in outcome.accepted {
                events
                    .emit(LiveEvent::FileAccepted {
                        kind: job.kind,
                        account_id: events.account.clone(),
                        thread: job.thread.clone(),
                        request_id: job.request_id,
                        message,
                        conversation,
                    })
                    .await?;
            }
            emit(
                events,
                &job.thread,
                job.request_id,
                job.kind,
                Progress {
                    phase: outcome.phase,
                    ..progress
                },
                outcome.message.as_deref(),
            )
            .await?;
        }
        Ok(())
    }
}
async fn emit(
    events: &Events,
    thread: &str,
    request_id: u64,
    kind: TransferKind,
    p: Progress,
    message: Option<&str>,
) -> Result<()> {
    events
        .emit(LiveEvent::FileTransfer {
            kind,
            account_id: events.account.clone(),
            thread: thread.into(),
            request_id,
            phase: p.phase,
            uploaded: p.uploaded,
            total: p.total,
            message: message.map(str::to_owned),
        })
        .await
}
async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
async fn authorize(
    tx: &mpsc::Sender<Authorization>,
    cancel: &AtomicBool,
    id: u64,
    final_dispatch: bool,
) -> Result<Option<Vec<u8>>> {
    let (reply, result) = oneshot::channel();
    tx.send(Authorization {
        final_dispatch,
        id,
        reply,
    })
    .await
    .map_err(|_| anyhow::anyhow!("Upload actor stopped"))?;
    if final_dispatch {
        return result
            .await
            .map_err(|_| anyhow::anyhow!("Upload actor stopped"))?;
    }
    tokio::select! {biased;result=result=>result.map_err(|_|anyhow::anyhow!("Upload actor stopped"))?,_=cancelled(cancel)=>anyhow::bail!("Upload cancelled")}
}
pub(super) struct Snapshot {
    pub file: File,
    pub checksum: String,
    pub size: u64,
}
fn identity(meta: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}
pub(super) fn snapshot(
    profile: &Profile,
    attachment: &Attachment,
    cancel: &AtomicBool,
) -> Result<Snapshot> {
    let inspected = Attachment::inspect(&attachment.path)?;
    anyhow::ensure!(inspected == *attachment, "Selected file changed");
    let mut input = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&attachment.path)?;
    let before = input.metadata()?;
    anyhow::ensure!(
        before.is_file()
            && before.len() == attachment.size_bytes
            && attachment.matches_metadata(&before),
        "Selected file changed"
    );
    let path = profile
        .paths
        .cache_dir
        .join(format!(".upload-{}", crate::domain::local_message_id()?));
    let mut output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&path)?;
    // Anonymous file-backed snapshot: cancellation, errors and crashes cannot leave file data in cache.
    std::fs::remove_file(&path)?;
    let mut hash = Md5::new();
    let mut total = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        anyhow::ensure!(!cancel.load(Ordering::Acquire), "Upload cancelled");
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        anyhow::ensure!(total <= attachment.size_bytes, "Selected file changed");
        hash.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    anyhow::ensure!(
        total == attachment.size_bytes && identity(&before) == identity(&input.metadata()?),
        "Selected file changed"
    );
    output.seek(SeekFrom::Start(0))?;
    Ok(Snapshot {
        file: output,
        checksum: format!("{:x}", hash.finalize()),
        size: total,
    })
}
#[allow(clippy::too_many_arguments)]
async fn run(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    db: DatabaseHandle,
    client: reqwest::Client,
    thread: String,
    input: UploadJob,
    cancel: Arc<AtomicBool>,
    progress: watch::Sender<Progress>,
    requests: mpsc::Sender<Authorization>,
    cache: Arc<Mutex<ResourceCache>>,
    photo_cache: Arc<Mutex<ResourceCache<wire_photo::PhotoUrls>>>,
) -> Outcome {
    let total = input.size();
    let photos = match input {
        UploadJob::Album(photos) => photos,
        UploadJob::Single(input) => {
            let result = run_one(
                profile,
                session,
                db,
                client,
                thread,
                input,
                cancel,
                progress,
                requests,
                cache,
                photo_cache,
                None,
                (0, total),
            )
            .await;
            return Outcome {
                phase: result.phase,
                accepted: result.accepted.into_iter().collect(),
                message: result.message.map(str::to_owned),
            };
        }
    };
    let count = photos.len();
    let specs = photos
        .iter()
        .map(|photo| photo_spec(photo, String::new()))
        .collect::<Vec<_>>();
    let account = session.account_id().to_owned();
    let scope = thread.clone();
    if let Err(error) = db
        .call_with(StorageOperation::PrepareOutgoing, move |store| {
            store.preflight_album(&account, &scope, &specs)
        })
        .await
    {
        let _ = diagnostic(&error, LiveStage::Storage);
        return Outcome {
            phase: FileTransferPhase::Failed,
            accepted: Vec::new(),
            message: Some(
                "Chưa thể gửi album. Kiểm tra lượt gửi ảnh trước đó và quyền gửi.".into(),
            ),
        };
    }
    let mut accepted = Vec::with_capacity(count);
    let mut layout_id = None;
    let mut offset = 0;
    for (index, photo) in photos.into_iter().enumerate() {
        let size = photo.size_bytes();
        let position = AlbumPosition {
            layout_id,
            index: index as u32,
            count: count as u32,
        };
        let result = run_one(
            profile.clone(),
            session.clone(),
            db.clone(),
            client.clone(),
            thread.clone(),
            UploadInput::Photo {
                photo,
                caption: String::new(),
            },
            cancel.clone(),
            progress.clone(),
            requests.clone(),
            cache.clone(),
            photo_cache.clone(),
            Some(position),
            (offset, total),
        )
        .await;
        if let Some((message, conversation)) = result.accepted {
            if layout_id.is_none() {
                layout_id = message
                    .source_client_id
                    .as_deref()
                    .and_then(|id| id.parse().ok());
            }
            accepted.push((message, conversation));
            offset += size;
        }
        if result.phase != FileTransferPhase::Accepted || layout_id.is_none() {
            let committed = accepted.len();
            let unknown = result.phase == FileTransferPhase::Unknown;
            let local_failure = result.phase == FileTransferPhase::AcceptedLocalFailure;
            let (phase, message) = if committed > 0 {
                let suffix = if unknown {
                    " Kết quả của ảnh đang gửi chưa rõ."
                } else if local_failure {
                    " Zalo đã nhận thêm một ảnh đang chờ lưu xác nhận."
                } else {
                    ""
                };
                (FileTransferPhase::Partial, format!("Đã lưu xác nhận {committed}/{count} ảnh.{suffix} Hãy kiểm tra hội thoại trước khi gửi lại."))
            } else if unknown {
                (FileTransferPhase::Unknown, "Chưa xác nhận được kết quả gửi album. Hãy kiểm tra hội thoại trước khi gửi lại.".into())
            } else if local_failure {
                (FileTransferPhase::AcceptedLocalFailure, "Zalo đã nhận một ảnh. Ứng dụng chưa lưu được xác nhận. Hãy kiểm tra hội thoại trước khi gửi lại.".into())
            } else if result.phase == FileTransferPhase::Cancelled {
                (
                    FileTransferPhase::Cancelled,
                    "Đã hủy gửi album. Các ảnh đã chọn được giữ lại.".into(),
                )
            } else {
                (
                    FileTransferPhase::Failed,
                    "Chưa gửi được album. Các ảnh đã chọn được giữ lại.".into(),
                )
            };
            return Outcome {
                phase,
                accepted,
                message: Some(message),
            };
        }
    }
    Outcome {
        phase: FileTransferPhase::Accepted,
        accepted,
        message: None,
    }
}

fn photo_spec(photo: &PreparedPhoto, caption: String) -> PhotoSpec {
    PhotoSpec {
        caption,
        width: photo.width(),
        height: photo.height(),
        size: photo.size_bytes(),
        checksum: photo.0.checksum.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_one(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    db: DatabaseHandle,
    client: reqwest::Client,
    thread: String,
    input: UploadInput,
    cancel: Arc<AtomicBool>,
    progress: watch::Sender<Progress>,
    requests: mpsc::Sender<Authorization>,
    cache: Arc<Mutex<ResourceCache>>,
    photo_cache: Arc<Mutex<ResourceCache<wire_photo::PhotoUrls>>>,
    position: Option<AlbumPosition>,
    (progress_offset, progress_total): (u64, u64),
) -> SingleOutcome {
    let mut reserved = None;
    let mut permitted = false;
    let mut dispatched = false;
    let mut acknowledged = false;
    let kind = input.kind();
    let report = |phase, uploaded, source_size| {
        let phase = if progress_offset + source_size < progress_total {
            FileTransferPhase::Uploading
        } else {
            phase
        };
        progress.send_replace(Progress {
            phase,
            uploaded: progress_offset + uploaded,
            total: progress_total,
        });
    };
    let result:Result<(Message,Conversation)>=async {
        let name=input.name();
        let original=matches!(&input, UploadInput::Photo{photo,..} if photo.resolution()==PhotoResolution::Original);
        if original {
            let eligible=tokio::select!{biased;
                result=original_photo::entitled(&client,&session)=>result.unwrap_or(false),
                _=cancelled(&cancel)=>false,
            };
            anyhow::ensure!(eligible,"Original photo is unavailable");
        }
        let mut source=match &input {
            UploadInput::File(attachment)=>{let copied=attachment.clone();let p=profile.clone();let c=cancel.clone();tokio::task::spawn_blocking(move||snapshot(&p,&copied,&c)).await??},
            UploadInput::Photo{photo,..}=>Snapshot{file:photo.0.file.try_clone()?,checksum:photo.0.checksum.clone(),size:photo.size_bytes()},
        };
        anyhow::ensure!(!cancel.load(Ordering::Acquire),"Upload cancelled");
        let account=session.account_id().to_owned();let t=thread.clone();
        let (id, album)=match &input {
            UploadInput::File(_)=>{let spec=AttachmentSpec{file_name:wire_upload::normalized_file_name(&name),size:source.size,checksum:source.checksum.clone()};(db.call_with(StorageOperation::PrepareOutgoing,move|s|s.reserve_attachment(&account,&t,&spec,now_ms())).await?, None)},
            UploadInput::Photo{photo,caption}=>{
                let spec = photo_spec(photo, caption.clone());
                let resolution = photo.resolution();
                db.call_with(StorageOperation::PrepareOutgoing, move |s| {
                    if let Some(position) = position {
                        let (id, member) = s.reserve_album_photo(&account, &t, &spec, position, now_ms())?;
                        Ok((id, Some(member)))
                    } else {
                        Ok((s.reserve_photo_resolution(&account, &t, &spec, resolution, now_ms())?, None))
                    }
                }).await?
            },
        };reserved=Some(id);
        authorize(&requests,&cancel,id,false).await?;
        let target=Target::from_thread(&thread)?;let final_root=root_for(&session,"file")?;
        use crate::protocol::bootstrap::PhotoUploadRoute;
        let photo_route=if original {Some(PhotoUploadRoute::Original)} else {session.config().photo_upload_route};
        let upload_root=match (kind,photo_route) {
            (TransferKind::Photo,Some(PhotoUploadRoute::Photo))=>root_for(&session,"up_file")?,
            (TransferKind::Photo,Some(PhotoUploadRoute::Original))=>root_for(&session,crate::protocol::original_photo::UPLOAD_SERVICE)?,
            _=>final_root.clone(),
        };
        let chunk_size=session.config().file_chunk_size;let total=source.size.div_ceil(chunk_size as u64);
        let mut file_resource=None;let mut photo_resource=None;let mut uploaded=0u64;
        for index in 1..=total {
            anyhow::ensure!(!cancel.load(Ordering::Acquire),"Upload cancelled");
            let wanted=(source.size-uploaded).min(chunk_size as u64) as usize;let mut bytes=vec![0;wanted];
            let offset=uploaded;
            let(next_source,bytes)=tokio::task::spawn_blocking(move||->Result<_>{source.file.read_exact_at(&mut bytes,offset)?;Ok((source,bytes))}).await??;source=next_source;
            let params=wire_upload::chunk_parameters(&target,&name,source.size,id,session.imei(),index,total)?;
            let mut url=if kind==TransferKind::File{wire_upload::endpoint(&upload_root,target.kind,session.client_type(),true)?}else{wire_photo::endpoint(&upload_root,target.kind,session.client_type(),Some(photo_route.ok_or(WireError::Unsupported)?))?};
            url.query_pairs_mut().append_pair("params",&session.encrypt_payload(&serde_json::to_vec(&params)?)?);
            let boundary=format!("zalo-native-{id}-{index}");let filename=name.replace('"',"%22");
            let mut body=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"chunkContent\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
            body.extend_from_slice(&bytes);body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            let cookie=session.authorization_cookie(&url).ok_or(WireError::Invalid)?;
            let request=async {let response=client.post(url).header("Cookie",cookie).header("Content-Type",format!("multipart/form-data; boundary={boundary}")).body(body).send().await?;upload_response(response,&session).await};
            let payload=tokio::select!{biased;r=request=>r?,_=cancelled(&cancel)=>anyhow::bail!("Upload cancelled")};
            uploaded += wanted as u64;
            report(FileTransferPhase::Uploading, uploaded, source.size);
            let finished=match kind {
                TransferKind::File=>match wire_upload::chunk_result(&payload)? {wire_upload::ChunkResult::Continue=>false,wire_upload::ChunkResult::Finished{file_id,url}=>{file_resource=Some((file_id,url));true}},
                TransferKind::Photo=>match wire_photo::chunk_result(&payload)? {wire_photo::PhotoChunkResult::Continue=>false,wire_photo::PhotoChunkResult::Finished{photo_id,urls}=>{photo_resource=Some((photo_id,urls));true}},
                TransferKind::Album => unreachable!("Only single members are uploaded here"),
            };
            if finished{anyhow::ensure!(index==total||payload.get("error_code").and_then(Value::as_i64)==Some(1),"Premature resource completion");break}
            anyhow::ensure!(index<total,"Missing resource completion");
        }
        report(FileTransferPhase::WaitingForResource, uploaded, source.size);
        let account=session.account_id().to_owned();
        match &input {
            UploadInput::File(_)=>{
                let(file_id,url)=file_resource.ok_or(WireError::Invalid)?;
                let url=if let Some(url)=url{url}else{tokio::time::timeout(Duration::from_secs(wire_upload::RESOURCE_TTL_SECS),async{loop{anyhow::ensure!(!cancel.load(Ordering::Acquire),"Upload cancelled");if let Some(url)=cache.lock().ok().and_then(|c|c.get(&file_id)){break Ok::<_,anyhow::Error>(url)}tokio::time::sleep(Duration::from_millis(50)).await;}}).await??};
                let extension=wire_upload::source_extension(&name);let parameters=wire_upload::message_parameters(&target,&name,&extension,source.size,&source.checksum,id,session.imei(),&file_id,&url)?;
                let content=wire_upload::media_content(&name,&extension,source.size,&source.checksum,&url)?;
                let resource=AttachmentResource{file_id,file_url:url,checksum:source.checksum.clone(),parameters,content};db.call_with(StorageOperation::PrepareOutgoing,move|s|s.attachment_resource_ready(&account,id,&resource)).await?;
            },
            UploadInput::Photo{photo,caption}=>{
                let(photo_id,urls)=photo_resource.ok_or(WireError::Invalid)?;
                let urls=if let Some(urls)=urls{urls}else{tokio::time::timeout(Duration::from_secs(wire_upload::RESOURCE_TTL_SECS),async{loop{anyhow::ensure!(!cancel.load(Ordering::Acquire),"Upload cancelled");if let Some(urls)=photo_cache.lock().ok().and_then(|c|c.get(&photo_id)){break Ok::<_,anyhow::Error>(urls)}tokio::time::sleep(Duration::from_millis(50)).await;}}).await??};
                let mut parameters=wire_photo::message_parameters(&target,&photo_id,id,caption,photo.width(),photo.height(),session.imei(),&urls)?;
                if original {
                    anyhow::ensure!(urls.matches_original(source.size),"Missing original photo resource proof");
                    parameters["jcp"]=json!("{\"is_original\":1}");
                }
                let mut content=wire_photo::content_for_resolution(caption,photo.width(),photo.height(),&urls,photo.resolution())?;
                if let Some(member) = album {
                    member.apply_to_message_parameters(&mut parameters, session.config().photo_album_extra_metadata)?;
                    content = member.apply_to_media_content(&content)?;
                }
                let resource=PhotoResource{photo_id,parameters,content};db.call_with(StorageOperation::PrepareOutgoing,move|s|s.photo_resource_ready(&account,id,&resource)).await?;
            },
        }
        report(FileTransferPhase::Finalizing, uploaded, source.size);
        let canonical=authorize(&requests,&cancel,id,true).await?.ok_or(WireError::Invalid)?;permitted=true;
        anyhow::ensure!(!cancel.load(Ordering::Acquire),"Upload cancelled after dispatch permit");
        let params=session.encrypt_payload(&canonical)?;let body=url::form_urlencoded::Serializer::new(String::new()).append_pair("params",&params).finish();
        let final_url=if kind==TransferKind::File{wire_upload::endpoint(&final_root,target.kind,session.client_type(),false)?}else{wire_photo::endpoint(&final_root,target.kind,session.client_type(),None)?};
        anyhow::ensure!(!cancel.load(Ordering::Acquire),"Upload cancelled before dispatch");
        dispatched=true;
        let payload=tokio::select!{biased;r=authenticated_request(&client,&session,final_url,Some(body))=>r?,_=cancelled(&cancel)=>anyhow::bail!("Upload cancelled after dispatch")};
        let ack=wire::send_ack(&payload)?;acknowledged=true;let account=session.account_id().to_owned();
        db.call_with(StorageOperation::PersistAck,move|s|match kind{TransferKind::File=>{s.record_attachment_ack(&account,id,&ack.server_id,ack.timestamp_ms)?;s.accept_attachment(&account,id)},TransferKind::Photo | TransferKind::Album=>{s.record_photo_ack(&account,id,&ack.server_id,ack.timestamp_ms)?;s.accept_photo(&account,id)}}).await
    }.await;
    match result {
        Ok(accepted) => SingleOutcome {
            phase: FileTransferPhase::Accepted,
            accepted: Some(accepted),
            message: None,
        },
        Err(error) => {
            // Error bodies, file paths, remote URLs and message contents never enter UI diagnostics.
            if error.downcast_ref::<StorageContext>().is_some() {
                let _ = diagnostic(&error, LiveStage::Storage);
            }
            let known_rejection = wire_error(&error)
                .is_some_and(|e| matches!(e, WireError::Service(_) | WireError::Authentication));
            let unknown = dispatched && !known_rejection;
            if let Some(id) = reserved {
                let account = session.account_id().to_owned();
                // Cleanup and confirmation share one serialized store job. A correlated own
                // push already accepted by receive cannot be downgraded by a lost HTTP result.
                let confirmed = db
                    .call_with(StorageOperation::FailOutgoing, move |store| {
                        use crate::store::attachments::AttachmentState;
                        if permitted {
                            match kind {
                                TransferKind::File => {
                                    store.fail_attachment_dispatch(&account, id, unknown)?
                                }
                                TransferKind::Photo | TransferKind::Album => {
                                    store.fail_photo_dispatch(&account, id, unknown)?
                                }
                            };
                        } else {
                            match kind {
                                TransferKind::File => store.interrupt_attachment(&account, id)?,
                                TransferKind::Photo | TransferKind::Album => {
                                    store.interrupt_photo(&account, id)?
                                }
                            };
                        }
                        match match kind {
                            TransferKind::File => store.attachment_state(&account, id)?,
                            TransferKind::Photo | TransferKind::Album => {
                                store.photo_state(&account, id)?
                            }
                        } {
                            Some(AttachmentState::Accepted | AttachmentState::Acknowledged) => {
                                Ok(Some(match kind {
                                    TransferKind::File => store.accept_attachment(&account, id),
                                    TransferKind::Photo | TransferKind::Album => {
                                        store.accept_photo(&account, id)
                                    }
                                }))
                            }
                            _ => Ok(None),
                        }
                    })
                    .await;
                match confirmed {
                    Ok(Some(Ok(accepted))) => {
                        return SingleOutcome {
                            phase: FileTransferPhase::Accepted,
                            accepted: Some(accepted),
                            message: None,
                        }
                    }
                    Ok(Some(Err(error))) => {
                        acknowledged = true;
                        let _ = diagnostic(
                            &storage_result::<()>(StorageOperation::PersistAck, Err(error))
                                .unwrap_err(),
                            LiveStage::Storage,
                        );
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = diagnostic(&error, LiveStage::Storage);
                    }
                }
            }
            let (phase, message) = if acknowledged {
                (
                    FileTransferPhase::AcceptedLocalFailure,
                    "Máy chủ đã nhận tệp; lịch sử trên máy sẽ được khôi phục. Không gửi lại.",
                )
            } else if unknown {
                (
                    FileTransferPhase::Unknown,
                    "Chưa xác nhận được kết quả gửi tệp. Kiểm tra hội thoại trước khi gửi lại.",
                )
            } else if cancel.load(Ordering::Acquire) {
                (
                    FileTransferPhase::Cancelled,
                    "Đã hủy gửi tệp. Tệp đã chọn được giữ lại.",
                )
            } else {
                (FileTransferPhase::Failed,"Chưa gửi được tệp. Tệp đã chọn được giữ lại; hãy kiểm tra kết nối và quyền gửi.")
            };
            SingleOutcome {
                phase,
                accepted: None,
                message: Some(message),
            }
        }
    }
}
async fn upload_response(
    mut response: reqwest::Response,
    session: &VerifiedSession,
) -> Result<Value> {
    anyhow::ensure!(
        response.status() == reqwest::StatusCode::OK,
        "Upload HTTP failure"
    );
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= wire::MAX_FRAME as u64),
        "Upload response too large"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= wire::MAX_FRAME,
            "Upload response too large"
        );
        bytes.extend_from_slice(&chunk)
    }
    let outer: Value = serde_json::from_slice(&bytes)?;
    wire::service_code(&outer)?;
    let plaintext = session.decrypt_payload(
        outer
            .get("data")
            .and_then(Value::as_str)
            .ok_or(WireError::Invalid)?,
    )?;
    anyhow::ensure!(
        plaintext.len() <= wire::MAX_INFLATED,
        "Upload response too large"
    );
    Ok(serde_json::from_slice(&plaintext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_cache_is_bounded_and_conflict_stays_invalid_until_expiration() {
        let mut cache: ResourceCache<String> = ResourceCache::default();
        cache.insert("7".into(), "https://example.test/a".into());
        cache.insert("7".into(), "https://example.test/b".into());
        cache.insert("7".into(), "https://example.test/a".into());
        assert_eq!(cache.get("7"), None);
        for id in 1..=100 {
            cache.insert(id.to_string(), "https://example.test/x".into())
        }
        assert_eq!(cache.entries.len(), wire_upload::MAX_CONTROL_CACHE);
    }
    #[test]
    fn snapshot_is_private_unlinked_hashes_exact_bytes_and_rejects_replaced_selection() {
        let root = std::env::temp_dir().join(format!(
            "zalo-upload-snapshot-{}",
            crate::domain::local_message_id().unwrap()
        ));
        let profile = Profile::at_root(&root, "synthetic").unwrap();
        let path = root.join("synthetic.txt");
        std::fs::write(&path, b"abc").unwrap();
        let selected = Attachment::inspect(&path).unwrap();
        let cancel = AtomicBool::new(false);
        let mut snap = snapshot(&profile, &selected, &cancel).unwrap();
        assert_eq!(snap.checksum, "900150983cd24fb0d6963f7d28e17f72");
        std::fs::write(&path, b"xyz").unwrap();
        let mut bytes = Vec::new();
        snap.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"abc");
        assert!(snapshot(&profile, &selected, &cancel).is_err());
        assert!(!std::fs::read_dir(&profile.paths.cache_dir)
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".upload-")));
        drop(snap);
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
}
