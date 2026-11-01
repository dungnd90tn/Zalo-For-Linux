use super::*;
use crate::{
    app::directory::{DirectoryCategory, DirectoryFetcher, DirectorySink, DirectoryStage},
    protocol::messaging::{self as wire, DecodedFrame, FrameDecoder, Target, WireError},
    store::directory::DirectoryBatch,
    store::MessageStore,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    tungstenite::{self, client::IntoClientRequest, protocol::WebSocketConfig},
    MaybeTlsStream, WebSocketStream,
};
use url::Url;

#[cfg(test)]
mod integration_tests;
#[cfg(feature = "signal-crypto")]
pub(crate) mod signal_transport;

#[derive(Debug)]
struct NetworkFailure(LiveDiagnostic);
impl std::fmt::Display for NetworkFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for NetworkFailure {}
impl NetworkFailure {
    fn new(stage: LiveStage, category: LiveCategory) -> Self {
        Self(LiveDiagnostic {
            stage,
            category,
            http_status: None,
            server_code: None,
            parser_reason: None,
        })
    }
    fn socket(stage: LiveStage, error: tungstenite::Error) -> Self {
        let category = match &error {
            tungstenite::Error::Http(_) => LiveCategory::Http,
            tungstenite::Error::Tls(_) => LiveCategory::Tls,
            tungstenite::Error::Io(e)
                if stage == LiveStage::Connect
                    && matches!(e.kind(), std::io::ErrorKind::InvalidData) =>
            {
                LiveCategory::Tls
            }
            tungstenite::Error::Io(_) => LiveCategory::Network,
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                LiveCategory::Closed
            }
            tungstenite::Error::Capacity(_) | tungstenite::Error::WriteBufferFull(_) => {
                LiveCategory::Resource
            }
            _ => LiveCategory::Protocol,
        };
        let mut result = Self::new(stage, category);
        if let tungstenite::Error::Http(reply) = error {
            result.0.http_status = Some(reply.status().as_u16());
        }
        result
    }
}
fn wire_error(error: &anyhow::Error) -> Option<&WireError> {
    error
        .downcast_ref::<wire::FrameFailure>()
        .map(|failure| &failure.error)
        .or_else(|| error.downcast_ref::<WireError>())
}
fn diagnostic(error: &anyhow::Error, stage: LiveStage) -> LiveDiagnostic {
    if let Some(network) = error.downcast_ref::<NetworkFailure>() {
        record_failure(network.0, None);
        return network.0;
    }
    let frame_failure = error.downcast_ref::<wire::FrameFailure>();
    let wire_error = wire_error(error);
    let category = match wire_error {
        Some(WireError::Invalid) => LiveCategory::Protocol,
        Some(WireError::Oversize) => LiveCategory::Resource,
        Some(WireError::Crypto) => LiveCategory::Crypto,
        Some(WireError::Unsupported) => LiveCategory::Unsupported,
        Some(WireError::Authentication) => LiveCategory::Authentication,
        Some(WireError::Service(_)) => LiveCategory::Server,
        None => LiveCategory::Storage,
    };
    let result = LiveDiagnostic {
        stage,
        category,
        http_status: None,
        parser_reason: frame_failure.map(|failure| failure.reason),
        server_code: match wire_error {
            Some(WireError::Service(code)) => Some(*code),
            _ => None,
        },
    };
    record_failure(result, frame_failure.map(|failure| failure.context));
    result
}
fn record_failure(detail: LiveDiagnostic, context: Option<wire::FrameContext>) {
    use crate::diagnostics::{record, Event, Kind, Reason, Stage, ValueType};
    let stage = match detail.stage {
        LiveStage::Configuration | LiveStage::Connect => Stage::Connection,
        LiveStage::Authentication => Stage::Authentication,
        LiveStage::Send => Stage::Send,
        LiveStage::Receive => Stage::Receive,
        LiveStage::Synchronization => Stage::Synchronization,
        LiveStage::Heartbeat => Stage::Heartbeat,
        LiveStage::Storage => Stage::Storage,
    };
    let reason = match detail.parser_reason {
        Some(reason) => match reason {
            wire::FrameReason::MessageType => Reason::MessageType,
            wire::FrameReason::FrameLength => Reason::FrameLength,
            wire::FrameReason::OuterJson => Reason::OuterJson,
            wire::FrameReason::OuterEnvelope => Reason::OuterEnvelope,
            wire::FrameReason::AuthKey => Reason::AuthKey,
            wire::FrameReason::EncryptionMode => Reason::EncryptionMode,
            wire::FrameReason::OuterData => Reason::OuterData,
            wire::FrameReason::Base64 => Reason::Base64,
            wire::FrameReason::CipherIntegrity => Reason::CipherIntegrity,
            wire::FrameReason::Compression => Reason::Compression,
            wire::FrameReason::InnerJson => Reason::InnerJson,
            wire::FrameReason::InnerEnvelope => Reason::InnerEnvelope,
            wire::FrameReason::PlaintextAuth => Reason::PlaintextAuth,
            wire::FrameReason::QueueCommands => Reason::QueueCommands,
        },
        None => match detail.category {
            LiveCategory::Timeout => Reason::Timeout,
            LiveCategory::Http => Reason::HttpStatus,
            LiveCategory::Authentication | LiveCategory::Server => Reason::ServerCode,
            LiveCategory::Unsupported => Reason::Unsupported,
            LiveCategory::Protocol | LiveCategory::Crypto => Reason::Invalid,
            LiveCategory::Storage => Reason::Io,
            _ => Reason::Transport,
        },
    };
    let mut event = Event::new(Kind::Failure, stage, reason)
        .http_status(detail.http_status)
        .server_code(detail.server_code);
    if let Some(context) = context {
        let kind = |value| match value {
            wire::FrameValueType::Absent => ValueType::Absent,
            wire::FrameValueType::Null => ValueType::Null,
            wire::FrameValueType::Integer => ValueType::Integer,
            wire::FrameValueType::Number => ValueType::Number,
            wire::FrameValueType::String => ValueType::String,
            wire::FrameValueType::Boolean => ValueType::Boolean,
            wire::FrameValueType::Object => ValueType::Object,
            wire::FrameValueType::Array => ValueType::Array,
        };
        event = event
            .bytes(context.bytes)
            .envelope_types(kind(context.status_type), kind(context.data_type));
        if let (Some(command), Some(subcommand)) = (context.command, context.subcommand) {
            event = event.frame(command, subcommand, context.encryption)
        }
    }
    record(event);
}
fn trace(stage: crate::diagnostics::Stage, reason: crate::diagnostics::Reason) {
    crate::diagnostics::record(crate::diagnostics::Event::new(
        crate::diagnostics::Kind::Transition,
        stage,
        reason,
    ));
}
fn record_directory_failure(failure: &DirectoryFailure) {
    use crate::diagnostics::{record, Event, Kind, Reason, Stage};
    let reason = match failure.category {
        DirectoryCategory::Unavailable => Reason::Unsupported,
        DirectoryCategory::Network => Reason::Transport,
        DirectoryCategory::Timeout => Reason::Timeout,
        DirectoryCategory::Http => Reason::HttpStatus,
        DirectoryCategory::Server => Reason::ServerCode,
        DirectoryCategory::Crypto | DirectoryCategory::Schema => Reason::Invalid,
        DirectoryCategory::Resource => Reason::FrameLength,
        DirectoryCategory::Cancelled => Reason::Cancelled,
        DirectoryCategory::Storage => Reason::Io,
    };
    record(
        Event::new(Kind::Failure, Stage::Synchronization, reason)
            .http_status(failure.http_status)
            .server_code(failure.server_code),
    );
}
/// Deterministic transport failures require an explicit retry, while the local
/// account database and command receiver remain available.
fn terminal_transport_state(error: &anyhow::Error) -> Option<LiveConnection> {
    if let Some(wire) = wire_error(error) {
        return Some(if *wire == WireError::Authentication {
            LiveConnection::Expired
        } else {
            LiveConnection::Offline
        });
    }
    let failure = error.downcast_ref::<NetworkFailure>()?;
    match failure.0.category {
        LiveCategory::Http => match failure.0.http_status {
            Some(401) => Some(LiveConnection::Expired),
            Some(300..=499) if failure.0.http_status != Some(429) => Some(LiveConnection::Offline),
            _ => None,
        },
        LiveCategory::Tls | LiveCategory::Protocol | LiveCategory::Resource => {
            Some(LiveConnection::Offline)
        }
        LiveCategory::Closed => match failure.0.server_code {
            Some(3000) => Some(LiveConnection::Expired),
            Some(3001..=3003) => Some(LiveConnection::Offline),
            _ => None,
        },
        _ => None,
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Job = Box<dyn FnOnce(&mut MessageStore) + Send>;
struct Database {
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}
#[derive(Clone)]
struct DatabaseHandle {
    jobs: mpsc::Sender<Job>,
}
impl Database {
    async fn open(profile: Arc<Profile>, account: String) -> Result<Self> {
        let (tx, mut rx) = mpsc::channel::<Job>(16);
        let (ready, done) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("native-live-storage".into())
            .spawn(move || {
                let opened = profile
                    .bind_account(&account)
                    .and_then(|()| {
                        MessageStore::open(&profile.paths.data_dir.join("messages.sqlite"))
                    })
                    .and_then(|mut s| {
                        s.recover_outgoing(&account)?;
                        s.reconcile_outgoing(&account)?;
                        Ok(s)
                    });
                let mut store = match opened {
                    Ok(s) => {
                        let _ = ready.send(Ok(()));
                        s
                    }
                    Err(_) => {
                        let _ = ready.send(Err(anyhow::anyhow!(
                            "Cannot open this account's private message database"
                        )));
                        return;
                    }
                };
                while let Some(job) = rx.blocking_recv() {
                    job(&mut store)
                }
            })?;
        done.await??;
        Ok(Self {
            jobs: Some(tx),
            thread: Some(thread),
        })
    }
    async fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut MessageStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.handle()?.call(job).await
    }
    fn handle(&self) -> Result<DatabaseHandle> {
        Ok(DatabaseHandle {
            jobs: self
                .jobs
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Storage stopped"))?
                .clone(),
        })
    }
}
impl DatabaseHandle {
    async fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut MessageStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.jobs
            .send(Box::new(move |store| {
                let _ = tx.send(job(store));
            }))
            .await
            .map_err(|_| anyhow::anyhow!("Storage stopped"))?;
        rx.await.map_err(|_| anyhow::anyhow!("Storage stopped"))?
    }
}

fn directory_failure(category: DirectoryCategory) -> DirectoryFailure {
    DirectoryFailure {
        stage: DirectoryStage::Storage,
        category,
        http_status: None,
        server_code: None,
    }
}
struct RuntimeDirectorySink {
    account: String,
    db: DatabaseHandle,
    updates: watch::Sender<u64>,
}
impl DirectorySink for RuntimeDirectorySink {
    async fn apply(&mut self, batch: DirectoryBatch) -> std::result::Result<(), DirectoryFailure> {
        if batch.account_id != self.account {
            return Err(directory_failure(DirectoryCategory::Storage));
        }
        self.db
            .call(move |store| {
                store.apply_directory_batch(&batch)?;
                Ok(())
            })
            .await
            .map_err(|_| directory_failure(DirectoryCategory::Storage))?;
        // Coalesce durable changes while the UI is slow. The directory task
        // never reserves critical WebSocket event capacity.
        self.updates
            .send_modify(|revision| *revision = revision.wrapping_add(1));
        Ok(())
    }
}
struct DirectoryRun {
    generation: u64,
    task: tokio::task::JoinHandle<std::result::Result<DirectoryReport, DirectoryFailure>>,
    cancel: watch::Sender<bool>,
}
impl Drop for DirectoryRun {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
        self.task.abort();
    }
}
struct DirectorySync {
    running: Option<DirectoryRun>,
    pending: Option<u64>,
    updates_tx: watch::Sender<u64>,
    updates_rx: watch::Receiver<u64>,
    status: Option<(u64, bool, DirectoryReport)>,
    conversations_changed: bool,
}
impl DirectorySync {
    fn new() -> Self {
        let (updates_tx, updates_rx) = watch::channel(0);
        Self {
            running: None,
            pending: None,
            updates_tx,
            updates_rx,
            status: None,
            conversations_changed: false,
        }
    }
    fn request(
        &mut self,
        generation: u64,
        session: Arc<VerifiedSession>,
        db: DatabaseHandle,
        transports: &Transports,
    ) {
        if self.running.is_some() {
            self.pending = Some(generation);
            self.status = Some((generation, true, DirectoryReport::default()));
            return;
        }
        self.start(generation, session, db, transports);
    }
    fn start(
        &mut self,
        generation: u64,
        session: Arc<VerifiedSession>,
        db: DatabaseHandle,
        transports: &Transports,
    ) {
        let fetcher = {
            #[cfg(test)]
            {
                if transports.local.is_some() {
                    Ok(DirectoryFetcher::with_client(transports.http.clone()))
                } else {
                    DirectoryFetcher::new()
                }
            }
            #[cfg(not(test))]
            {
                let _ = transports;
                DirectoryFetcher::new()
            }
        };
        let fetcher = match fetcher {
            Ok(fetcher) => fetcher,
            Err(error) => {
                self.status = Some((
                    generation,
                    false,
                    DirectoryReport {
                        failures: vec![error],
                        ..Default::default()
                    },
                ));
                return;
            }
        };
        let (cancel, rx) = watch::channel(false);
        let mut sink = RuntimeDirectorySink {
            account: session.account_id().into(),
            db,
            updates: self.updates_tx.clone(),
        };
        let task = tokio::spawn(async move { fetcher.synchronize(&session, &mut sink, rx).await });
        self.running = Some(DirectoryRun {
            generation,
            task,
            cancel,
        });
        self.status = Some((generation, true, DirectoryReport::default()));
    }
    fn completed(
        &mut self,
        result: std::result::Result<
            std::result::Result<DirectoryReport, DirectoryFailure>,
            tokio::task::JoinError,
        >,
        session: Arc<VerifiedSession>,
        db: DatabaseHandle,
        transports: &Transports,
    ) {
        let generation = self
            .running
            .take()
            .expect("completed directory sync")
            .generation;
        let report = match result {
            Ok(Ok(report)) => report,
            Ok(Err(error)) => DirectoryReport {
                failures: vec![error],
                ..Default::default()
            },
            Err(_) => DirectoryReport {
                failures: vec![DirectoryFailure {
                    stage: DirectoryStage::Synchronization,
                    category: DirectoryCategory::Cancelled,
                    http_status: None,
                    server_code: None,
                }],
                ..Default::default()
            },
        };
        if let Some(generation) = self.pending.take() {
            self.start(generation, session, db, transports);
        } else {
            self.status = Some((generation, false, report));
        }
    }
    fn notifications_pending(&self) -> bool {
        self.status.is_some() || self.conversations_changed
    }
    async fn publish_notification(
        &mut self,
        permit: mpsc::Permit<'_, LiveEvent>,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        if let Some((generation, syncing, report)) = self.status.take() {
            for failure in &report.failures {
                record_directory_failure(failure);
            }
            permit.send(LiveEvent::DirectoryStatus {
                account_id: events.account.clone(),
                generation,
                syncing,
                report,
            });
        } else {
            let account = events.account.clone();
            // Read after obtaining UI capacity: earlier Received/Sent commits
            // are included, and later socket events cannot overtake this read.
            let conversations = db
                .call(move |store| store.conversations(&account, crate::domain::MAX_PAGE_SIZE))
                .await?;
            self.conversations_changed = false;
            permit.send(LiveEvent::DirectoryChanged {
                account_id: events.account.clone(),
                conversations,
            });
        }
        (events.repaint)();
        Ok(())
    }
    fn changed(&mut self) {
        self.updates_rx.borrow_and_update();
        self.conversations_changed = true;
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        self.jobs.take();
        self.thread.take();
    }
}
struct Events {
    account: String,
    tx: mpsc::Sender<LiveEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}
impl Events {
    async fn emit(&self, event: LiveEvent) -> Result<()> {
        (self.repaint)();
        self.tx
            .send(event)
            .await
            .map_err(|_| anyhow::anyhow!("UI stopped"))?;
        (self.repaint)();
        Ok(())
    }
    async fn connection(&self, state: LiveConnection) -> Result<()> {
        use crate::diagnostics::{Reason, Stage};
        trace(
            Stage::Connection,
            match state {
                LiveConnection::Connecting | LiveConnection::Reconnecting => Reason::Requested,
                LiveConnection::Online => Reason::Online,
                LiveConnection::Offline | LiveConnection::Expired => Reason::Offline,
            },
        );
        self.emit(LiveEvent::Connection {
            account_id: self.account.clone(),
            state,
        })
        .await
    }
    async fn diagnostic(&self, diagnostic: LiveDiagnostic) -> Result<()> {
        self.emit(LiveEvent::Diagnostic {
            account_id: self.account.clone(),
            diagnostic,
        })
        .await
    }
}
struct Transports {
    http: reqwest::Client,
    #[cfg(test)]
    local: Option<(std::net::SocketAddr, Arc<rustls::ClientConfig>)>,
}
impl Transports {
    async fn connect(&self, session: &VerifiedSession, index: usize) -> Result<Connection> {
        #[cfg(test)]
        if let Some((address, tls)) = &self.local {
            let request = Connection::handshake_request(session, index)?;
            let tcp = tokio::net::TcpStream::connect(address)
                .await
                .map_err(|_| NetworkFailure::new(LiveStage::Connect, LiveCategory::Network))?;
            let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
                request,
                tcp,
                Some(Connection::socket_config()),
                Some(tokio_tungstenite::Connector::Rustls(tls.clone())),
            )
            .await
            .map_err(|e| NetworkFailure::socket(LiveStage::Connect, e))?;
            return Ok(Connection::connected(socket));
        }
        Connection::connect(session, index).await
    }
}
pub(super) async fn run(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    commands: mpsc::Receiver<LiveCommand>,
    events: mpsc::Sender<LiveEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    cancel: watch::Receiver<bool>,
) {
    let http = match client(session.client_type()) {
        Ok(client) => client,
        Err(_) => {
            let _ = events.try_send(LiveEvent::Fault {
                account_id: session.account_id().into(),
                error: "Cannot start messaging transport".into(),
            });
            repaint();
            return;
        }
    };
    let transports = Transports {
        http,
        #[cfg(test)]
        local: None,
    };
    run_with_transports(
        profile, session, commands, events, repaint, cancel, transports,
    )
    .await;
}
async fn run_with_transports(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    commands: mpsc::Receiver<LiveCommand>,
    events: mpsc::Sender<LiveEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    mut cancel: watch::Receiver<bool>,
    transports: Transports,
) {
    let account = session.account_id().to_owned();
    let event = Events {
        account,
        tx: events,
        repaint,
    };
    if *cancel.borrow() {
        return;
    }
    tokio::select! {biased; _=cancel.changed()=>{trace(crate::diagnostics::Stage::Shutdown,crate::diagnostics::Reason::Cancelled)},result=run_inner(profile,session,commands,&event,&transports)=>{if let Err(error)=result{
        let detail=diagnostic(&error,LiveStage::Storage);
        tokio::select!{biased;_=cancel.changed()=>{},_=event.diagnostic(detail)=>{}}
        tokio::select!{biased;_=cancel.changed()=>{},_=event.emit(LiveEvent::Fault{account_id:event.account.clone(),error:"Messaging stopped. Your local history and drafts are preserved.".into()})=>{}}
    }}}
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn client(client_type: crate::protocol::client::ClientType) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .user_agent(client_type.user_agent().ok_or(WireError::Unsupported)?)
        .build()?)
}
fn root_for(session: &VerifiedSession, name: &str) -> Result<Url> {
    session
        .config()
        .services
        .get(name)
        .and_then(|v| v.first())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("The server did not supply this messaging service"))
}
const MAX_OFFLINE_PAGES: u32 = 10_000;
#[derive(Default)]
struct OfflineProgress {
    last_id: u64,
    pages: u32,
}
fn safe_cursor(text: &str) -> Result<u64> {
    let value = text.parse::<u64>().map_err(|_| WireError::Invalid)?;
    if value == 0 || value > 9_007_199_254_740_991 {
        return Err(WireError::Invalid.into());
    }
    Ok(value)
}
struct Connection {
    socket: Socket,
    decoder: FrameDecoder,
    queues: std::collections::HashSet<(u16, u8)>,
    offline_progress: std::collections::HashMap<(u16, u8), OfflineProgress>,
    request_id: u64,
    last_pong: tokio::time::Instant,
    last_ping: Option<i64>,
    authenticated_at: Option<tokio::time::Instant>,
}
impl Connection {
    async fn send(&mut self, cmd: u16, sub: u8, data: Value) -> Result<()> {
        let frame = wire::encode_frame(cmd, sub, &data)?;
        tokio::time::timeout(
            Duration::from_secs(5),
            self.socket.send(tungstenite::Message::Binary(frame)),
        )
        .await
        .map_err(|_| NetworkFailure::new(LiveStage::Receive, LiveCategory::Timeout))?
        .map_err(|e| NetworkFailure::socket(LiveStage::Receive, e))?;
        Ok(())
    }
    async fn next(&mut self) -> Result<Option<DecodedFrame>> {
        match self.socket.next().await {
            Some(Ok(tungstenite::Message::Binary(bytes))) => {
                Ok(Some(self.decoder.decode_detailed(&bytes)?))
            }
            Some(Ok(tungstenite::Message::Ping(bytes))) => {
                tokio::time::timeout(
                    Duration::from_secs(5),
                    self.socket.send(tungstenite::Message::Pong(bytes)),
                )
                .await
                .map_err(|_| NetworkFailure::new(LiveStage::Heartbeat, LiveCategory::Timeout))?
                .map_err(|e| NetworkFailure::socket(LiveStage::Heartbeat, e))?;
                Ok(None)
            }
            Some(Ok(tungstenite::Message::Pong(_))) => Ok(None),
            Some(Ok(tungstenite::Message::Close(frame))) => {
                let mut failure = NetworkFailure::new(LiveStage::Receive, LiveCategory::Closed);
                failure.0.server_code = frame.map(|frame| i64::from(u16::from(frame.code)));
                Err(failure.into())
            }
            None => Err(NetworkFailure::new(LiveStage::Receive, LiveCategory::Closed).into()),
            Some(Ok(_)) => Err(wire::FrameFailure::new(
                WireError::Invalid,
                wire::FrameReason::MessageType,
            )
            .into()),
            Some(Err(error)) => Err(NetworkFailure::socket(LiveStage::Receive, error).into()),
        }
    }
    fn handshake_request(
        session: &VerifiedSession,
        index: usize,
    ) -> Result<tungstenite::http::Request<()>> {
        let roots = &session.config().websocket;
        let root = roots
            .get(index % roots.len().max(1))
            .ok_or(WireError::Unsupported)?;
        let url = wire::socket_url(root, now_ms(), session.client_type())?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| WireError::Invalid)?;
        let mut cookie_url = url.clone();
        cookie_url
            .set_scheme("https")
            .map_err(|_| WireError::Invalid)?;
        if let Some(cookie) = session.authorization_cookie(&cookie_url) {
            request
                .headers_mut()
                .insert("Cookie", cookie.parse().map_err(|_| WireError::Invalid)?);
        }
        request.headers_mut().insert("Origin", "file://".parse()?);
        request.headers_mut().insert(
            "User-Agent",
            session
                .client_type()
                .user_agent()
                .ok_or(WireError::Unsupported)?
                .parse()?,
        );
        Ok(request)
    }
    fn socket_config() -> WebSocketConfig {
        WebSocketConfig {
            write_buffer_size: 32 * 1024,
            max_write_buffer_size: wire::MAX_FRAME + 32 * 1024,
            max_message_size: Some(wire::MAX_FRAME),
            max_frame_size: Some(wire::MAX_FRAME),
            accept_unmasked_frames: false,
            ..Default::default()
        }
    }
    async fn connect(session: &VerifiedSession, index: usize) -> Result<Self> {
        let request = Self::handshake_request(session, index)?;
        let config = Self::socket_config();
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(15),
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| NetworkFailure::new(LiveStage::Connect, LiveCategory::Timeout))?
        .map_err(|e| NetworkFailure::socket(LiveStage::Connect, e))?;
        Ok(Self::connected(socket))
    }
    fn connected(socket: Socket) -> Self {
        Self {
            socket,
            decoder: FrameDecoder::default(),
            queues: Default::default(),
            offline_progress: Default::default(),
            request_id: 0,
            last_pong: tokio::time::Instant::now(),
            last_ping: None,
            authenticated_at: None,
        }
    }
    fn request(&mut self) -> String {
        self.request_id = self.request_id.wrapping_add(1);
        format!("native_req_{}", self.request_id)
    }
    async fn offline(
        &mut self,
        db: &Database,
        account: &str,
        cmd: u16,
        sub: u8,
        first: bool,
        continuation: Option<&str>,
    ) -> Result<()> {
        let account = account.to_owned();
        let queue = format!("{cmd}_{sub}");
        let id = match continuation {
            Some(id) => id.to_owned(),
            None => db.call(move |s| s.receive_cursor(&account, &queue)).await?,
        };
        let numeric = safe_cursor(&id)?;
        let progress = self.offline_progress.entry((cmd, sub)).or_default();
        if first {
            progress.pages = 0;
        } else if numeric <= progress.last_id {
            return Err(WireError::Invalid.into());
        }
        if progress.pages >= MAX_OFFLINE_PAGES {
            return Err(WireError::Oversize.into());
        }
        progress.last_id = numeric;
        progress.pages += 1;
        let req = self.request();
        self.send(
            cmd,
            sub,
            json!({"first":first,"reqId":req,"lastId":numeric,"preIds":[]}),
        )
        .await
    }
    async fn process(&mut self, frame: DecodedFrame, db: &Database, events: &Events) -> Result<()> {
        match frame.cmd {
            1 => {
                trace(
                    crate::diagnostics::Stage::Authentication,
                    crate::diagnostics::Reason::Ready,
                );
                self.authenticated_at = Some(tokio::time::Instant::now());
                let commands = frame
                    .payload
                    .pointer("/data/qCmds")
                    .and_then(Value::as_array)
                    .filter(|v| !v.is_empty() && v.len() <= 64)
                    .ok_or_else(|| {
                        wire::FrameFailure::new(
                            WireError::Invalid,
                            wire::FrameReason::QueueCommands,
                        )
                    })?;
                for command in commands {
                    let cmd = command.get("cmd").and_then(Value::as_u64).ok_or_else(|| {
                        wire::FrameFailure::new(
                            WireError::Invalid,
                            wire::FrameReason::QueueCommands,
                        )
                    })?;
                    let sub = command
                        .get("subCmd")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            wire::FrameFailure::new(
                                WireError::Invalid,
                                wire::FrameReason::QueueCommands,
                            )
                        })?;
                    if [510, 511].contains(&cmd) && sub <= 1 {
                        self.queues.insert((cmd as u16, sub as u8));
                    }
                }
                if self.queues.len() > 8 {
                    return Err(WireError::Oversize.into());
                }
                events.connection(LiveConnection::Online).await?;
                for (cmd, sub) in self.queues.clone() {
                    self.offline(db, &events.account, cmd, sub, true, None)
                        .await?;
                }
            }
            2 => {
                if frame
                    .payload
                    .get("eventId")
                    .and_then(Value::as_i64)
                    .is_some_and(|id| self.last_ping == Some(id))
                {
                    self.last_ping = None;
                    self.last_pong = tokio::time::Instant::now();
                    trace(
                        crate::diagnostics::Stage::Heartbeat,
                        crate::diagnostics::Reason::Ready,
                    );
                }
            }
            4 => {
                crate::diagnostics::record(
                    crate::diagnostics::Event::new(
                        crate::diagnostics::Kind::Debug,
                        crate::diagnostics::Stage::Heartbeat,
                        crate::diagnostics::Reason::Ready,
                    )
                    .frame(4, frame.sub_cmd, None),
                );
            }
            501 | 521 | 510 | 511 | 534 | 570 => {
                let parsed = wire::incoming_messages(&events.account, &frame.payload)?;
                let messages: Vec<_> = parsed
                    .into_iter()
                    .map(|i| (i.message, i.client_id))
                    .collect();
                let account = events.account.clone();
                let mut cursors = Vec::new();
                if let Some(status) = frame.payload.pointer("/data/queueStatus") {
                    let map = status
                        .as_object()
                        .filter(|m| m.len() <= 64)
                        .ok_or(WireError::Invalid)?;
                    for (queue, value) in map {
                        let (cmd, sub) = queue.split_once('_').ok_or(WireError::Invalid)?;
                        let cmd = cmd.parse::<u16>().map_err(|_| WireError::Invalid)?;
                        let sub = sub.parse::<u8>().map_err(|_| WireError::Invalid)?;
                        if ![510, 511].contains(&cmd) || sub > 1 {
                            continue;
                        }
                        if value.get("evict").and_then(Value::as_u64) == Some(1) {
                            return Err(WireError::Unsupported.into());
                        }
                        let id = wire::id_string(value.get("lastId").ok_or(WireError::Invalid)?)?;
                        cursors.push((queue.clone(), id));
                    }
                }
                let offline_queue = self.queues.contains(&(frame.cmd, frame.sub_cmd));
                let more = match frame.payload.pointer("/data/more") {
                    None => false,
                    Some(Value::Bool(value)) => *value,
                    Some(Value::Number(value)) if value.as_u64() == Some(0) => false,
                    Some(Value::Number(value)) if value.as_u64() == Some(1) => true,
                    _ => return Err(WireError::Invalid.into()),
                };
                let current_queue = format!("{}_{}", frame.cmd, frame.sub_cmd);
                // The PC client explicitly passes lastActionId to the next offline request.
                // queueStatus.lastId remains the normal reconnect cursor when available.
                let continuation = if offline_queue {
                    frame
                        .payload
                        .pointer("/data/lastActionId")
                        .map(wire::id_string)
                        .transpose()?
                        .or_else(|| {
                            cursors
                                .iter()
                                .find(|(queue, _)| queue == &current_queue)
                                .map(|(_, id)| id.clone())
                        })
                } else {
                    None
                };
                if offline_queue && more {
                    let next = safe_cursor(continuation.as_deref().ok_or(WireError::Invalid)?)?;
                    let previous = self
                        .offline_progress
                        .get(&(frame.cmd, frame.sub_cmd))
                        .ok_or(WireError::Invalid)?;
                    if next <= previous.last_id {
                        return Err(WireError::Invalid.into());
                    }
                    if previous.pages >= MAX_OFFLINE_PAGES {
                        return Err(WireError::Oversize.into());
                    }
                }
                if offline_queue && !cursors.iter().any(|(queue, _)| queue == &current_queue) {
                    if let Some(id) = &continuation {
                        safe_cursor(id)?;
                        cursors.push((current_queue, id.clone()));
                    }
                }
                let cursor = cursors.first().cloned();
                let stored = db
                    .call(move |s| {
                        s.receive_batch(
                            &account,
                            &messages,
                            cursor.as_ref().map(|(q, id)| (q.as_str(), id.as_str())),
                        )
                    })
                    .await?;
                for (queue, id) in cursors.into_iter().skip(1) {
                    let account = events.account.clone();
                    db.call(move |s| s.receive_batch(&account, &[], Some((&queue, &id))))
                        .await?;
                }
                for (message, conversation) in stored {
                    trace(
                        crate::diagnostics::Stage::Receive,
                        crate::diagnostics::Reason::Ready,
                    );
                    events
                        .emit(LiveEvent::Received {
                            account_id: events.account.clone(),
                            message,
                            conversation,
                        })
                        .await?;
                }
                if offline_queue && more {
                    self.offline(
                        db,
                        &events.account,
                        frame.cmd,
                        frame.sub_cmd,
                        false,
                        continuation.as_deref(),
                    )
                    .await?;
                }
            }
            // These encrypted queues are intentionally not consumed or acknowledged.
            551 | 552 | 535 | 543 | 544 | 548 | 561 | 580 | 581 | 582 => {}
            _ => {}
        }
        Ok(())
    }
    async fn verify_direct(
        &mut self,
        target: &Target,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        let total_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut last = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let request = self.request();
            let data = if let Some(id) = &last {
                json!({"reqIid":1,"first":false,"lastId":id})
            } else {
                json!({"reqIid":1,"first":true})
            };
            self.send(563, 0, json!({"reqId":request,"data":data}))
                .await?;
            let deadline =
                (tokio::time::Instant::now() + Duration::from_secs(10)).min(total_deadline);
            loop {
                let frame = tokio::time::timeout_at(deadline, self.next())
                    .await
                    .map_err(|_| WireError::Unsupported)??;
                let Some(frame) = frame else { continue };
                if frame.cmd == 563 {
                    let (allowed, next) =
                        wire::legacy_direct_allowed(&frame.payload, &request, &target.remote_id)?;
                    if !allowed {
                        return Err(WireError::Unsupported.into());
                    }
                    match next {
                        None => return Ok(()),
                        Some(id) => {
                            if !seen.insert(id.clone()) {
                                return Err(WireError::Invalid.into());
                            }
                            last = Some(id);
                            break;
                        }
                    }
                }
                // Process all intervening data before checking the final capability snapshot.
                if [551, 552, 535, 543, 544, 548, 561, 580, 581, 582].contains(&frame.cmd) {
                    return Err(WireError::Unsupported.into());
                }
                self.process(frame, db, events).await?;
            }
        }
        Err(WireError::Oversize.into())
    }
}
async fn read_response(
    mut response: reqwest::Response,
    session: &VerifiedSession,
) -> Result<Value> {
    if response.status() != reqwest::StatusCode::OK {
        anyhow::bail!("Messaging service returned an HTTP error")
    }
    if response
        .content_length()
        .is_some_and(|n| n > wire::MAX_FRAME as u64)
    {
        return Err(WireError::Oversize.into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Messaging response interrupted"))?
    {
        if bytes.len() + chunk.len() > wire::MAX_FRAME {
            return Err(WireError::Oversize.into());
        }
        bytes.extend_from_slice(&chunk)
    }
    let outer: Value = serde_json::from_slice(&bytes).map_err(|_| WireError::Invalid)?;
    wire::service_code(&outer)?;
    let encrypted = outer
        .get("data")
        .and_then(Value::as_str)
        .ok_or(WireError::Invalid)?;
    let bytes = session.decrypt_payload(encrypted)?;
    if bytes.len() > wire::MAX_INFLATED {
        return Err(WireError::Oversize.into());
    }
    let inner = serde_json::from_slice(&bytes).map_err(|_| WireError::Invalid)?;
    wire::service_code(&inner)?;
    Ok(inner)
}
async fn authenticated_request(
    client: &reqwest::Client,
    session: &VerifiedSession,
    url: Url,
    body: Option<String>,
) -> Result<Value> {
    let cookie = session
        .authorization_cookie(&url)
        .ok_or_else(|| anyhow::anyhow!("Untrusted messaging origin"))?;
    let request = if let Some(body) = body {
        client
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
    } else {
        client.get(url)
    };
    let response = request
        .header(
            "User-Agent",
            session
                .client_type()
                .user_agent()
                .ok_or(WireError::Unsupported)?,
        )
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Messaging request failed"))?;
    read_response(response, session).await
}
async fn verify_group(
    client: &reqwest::Client,
    session: &VerifiedSession,
    target: &Target,
) -> Result<u64> {
    let mut url = root_for(session, "group")?;
    wire::validate_origin(&url, "https")?;
    url.set_path("/api/group/getgi/v2");
    let params=session.encrypt_payload(&serde_json::to_vec(&json!({"grid":target.remote_id,"avatar_size":120,"member_avatar_size":120,"mpage":1,"mcount":100,"imei":session.imei()}))?)?;
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", session.client_type().api_type())
        .append_pair("params", &params);
    let payload = tokio::time::timeout(
        Duration::from_secs(15),
        authenticated_request(client, session, url, None),
    )
    .await??;
    Ok(wire::legacy_group_allowed(&payload, &target.remote_id)?)
}
async fn send_text(
    client: &reqwest::Client,
    session: &VerifiedSession,
    target: &Target,
    text: &str,
    client_id: u64,
    visibility: Option<u64>,
) -> Result<wire::SendAck> {
    let root = root_for(
        session,
        match target.kind {
            ThreadKind::Direct => "chat",
            ThreadKind::Group => "group",
        },
    )?;
    let url = wire::send_url(&root, target.kind, session.client_type())?;
    let mut parameters = wire::text_parameters(target, text, client_id, session.imei())?;
    if target.kind == ThreadKind::Group {
        parameters["visibility"] = json!(visibility.ok_or(WireError::Invalid)?);
    }
    let params = session.encrypt_payload(&serde_json::to_vec(&parameters)?)?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &params)
        .finish();
    let response = tokio::time::timeout(
        Duration::from_secs(15),
        authenticated_request(client, session, url, Some(body)),
    )
    .await??;
    Ok(wire::send_ack(&response)?)
}
async fn persist_ack(
    db: &Database,
    account: &str,
    id: u64,
    ack: wire::SendAck,
) -> Result<(Message, Conversation)> {
    let mut last = None;
    for attempt in 0..3 {
        let account = account.to_owned();
        let server = ack.server_id.clone();
        let timestamp = ack.timestamp_ms;
        let result = db
            .call(move |s| {
                s.record_outgoing_ack(&account, id, &server)?;
                s.accept_outgoing(&account, id, &server, timestamp)
            })
            .await;
        match result {
            Ok(value) => return Ok(value),
            Err(error) => {
                let _ = diagnostic(&error, LiveStage::Storage);
                last = Some(error)
            }
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("Cannot store acknowledgement")))
}
enum CommandOutcome {
    Continue,
    PauseTransport,
    Shutdown,
}
async fn handle_command(
    command: LiveCommand,
    connection: &mut Option<Connection>,
    db: &Database,
    events: &Events,
    session: &VerifiedSession,
    client: &reqwest::Client,
) -> Result<CommandOutcome> {
    match command {
        LiveCommand::Reconnect | LiveCommand::RefreshDirectory { .. } => {
            return Ok(CommandOutcome::Continue)
        }
        LiveCommand::Shutdown => return Ok(CommandOutcome::Shutdown),
        LiveCommand::Directory {
            generation,
            is_group,
            query,
            after,
        } => {
            const PAGE_SIZE: usize = 100;
            let account = events.account.clone();
            let search = query.clone();
            let result = db
                .call(move |store| {
                    store.directory_page(&account, is_group, &search, after.as_ref(), PAGE_SIZE + 1)
                })
                .await;
            let event = match result {
                Ok(mut entries) => {
                    let next = if entries.len() > PAGE_SIZE {
                        entries.pop();
                        entries.last().map(DirectoryCursor::from_record)
                    } else {
                        None
                    };
                    LiveEvent::DirectoryPage {
                        account_id: events.account.clone(),
                        generation,
                        is_group,
                        query,
                        entries,
                        next,
                    }
                }
                Err(_) => LiveEvent::DirectoryFailed {
                    account_id: events.account.clone(),
                    generation,
                    error: directory_failure(DirectoryCategory::Storage),
                },
            };
            events.emit(event).await?;
        }
        LiveCommand::Open {
            generation,
            kind,
            remote_id,
        } => {
            let target = Target::new(kind, &remote_id)?;
            let account = events.account.clone();
            let thread = target.thread_id();
            let title = match kind {
                ThreadKind::Direct => format!("Zalo ID {remote_id}"),
                ThreadKind::Group => format!("Nhóm {remote_id}"),
            };
            let result = db
                .call(move |s| {
                    let title = s
                        .directory_record(&account, &thread)?
                        .map(|entry| entry.display_name)
                        .unwrap_or(title);
                    s.open_conversation(&account, &thread, &title)
                })
                .await;
            match result {
                Ok(conversation) => {
                    events
                        .emit(LiveEvent::Opened {
                            account_id: events.account.clone(),
                            generation,
                            conversation,
                        })
                        .await?
                }
                Err(_) => {
                    events
                        .emit(LiveEvent::Fault {
                            account_id: events.account.clone(),
                            error: "Cannot open this conversation".into(),
                        })
                        .await?
                }
            }
        }
        LiveCommand::Load {
            generation,
            thread,
            before,
        } => {
            let account = events.account.clone();
            let requested = thread.clone();
            let result = db
                .call(move |s| {
                    s.page(
                        &account,
                        &requested,
                        before.as_ref(),
                        super::super::PAGE_SIZE + 1,
                    )
                })
                .await;
            let event = match result {
                Ok(mut messages) => {
                    let has_older = messages.len() > super::super::PAGE_SIZE;
                    if has_older {
                        messages.remove(0);
                    }
                    LiveEvent::Page {
                        account_id: events.account.clone(),
                        generation,
                        thread,
                        messages,
                        has_older,
                    }
                }
                Err(_) => LiveEvent::RequestFailed {
                    account_id: events.account.clone(),
                    generation,
                    thread,
                    error: "Cannot load this conversation".into(),
                },
            };
            events.emit(event).await?;
        }
        LiveCommand::Send {
            generation,
            thread,
            text,
            request_id,
        } => {
            trace(
                crate::diagnostics::Stage::Send,
                crate::diagnostics::Reason::Requested,
            );
            let target = Target::from_thread(&thread)?;
            let eligible = match connection.as_mut() {
                Some(c) if c.decoder.authenticated() => match target.kind {
                    ThreadKind::Direct => c.verify_direct(&target, db, events).await.map(|()| None),
                    ThreadKind::Group => verify_group(client, session, &target).await.map(Some),
                },
                _ => Err(anyhow::anyhow!("offline")),
            };
            let visibility = match eligible {
                Ok(visibility) => visibility,
                Err(error) => {
                    let _ = diagnostic(&error, LiveStage::Send);
                    let message = if wire_error(&error)
                        .is_some_and(|e| matches!(e, WireError::Unsupported))
                    {
                        "Hội thoại mã hóa đầu cuối hoặc chưa xác minh được trạng thái mã hóa. Bản nháp được giữ lại."
                    } else {
                        "Chưa xác minh được hội thoại. Hãy kiểm tra kết nối; bản nháp được giữ lại."
                    };
                    events
                        .emit(LiveEvent::SendFailed {
                            account_id: events.account.clone(),
                            generation,
                            thread,
                            request_id,
                            error: message.into(),
                            unknown_delivery: false,
                            server_accepted: false,
                        })
                        .await?;
                    if wire_error(&error) == Some(&WireError::Authentication) {
                        events
                            .diagnostic(diagnostic(&error, LiveStage::Authentication))
                            .await?;
                        events.connection(LiveConnection::Expired).await?;
                        return Ok(CommandOutcome::PauseTransport);
                    }
                    return Ok(CommandOutcome::Continue);
                }
            };
            let account = events.account.clone();
            let thread_copy = thread.clone();
            let text_copy = text.clone();
            let prepared = db
                .call(move |s| s.prepare_outgoing(&account, &thread_copy, &text_copy, now_ms()))
                .await;
            let id = match prepared {
                Ok(id) => id,
                Err(error) => {
                    let _ = diagnostic(&error, LiveStage::Storage);
                    events.emit(LiveEvent::SendFailed{account_id:events.account.clone(),generation,thread,request_id,error:"Bản nháp đã có lượt gửi chưa xác định, hoặc không thể lưu khóa gửi an toàn. Không tự gửi lại.".into(),unknown_delivery:true,server_accepted:false}).await?;
                    return Ok(CommandOutcome::Continue);
                }
            };
            match send_text(client, session, &target, &text, id, visibility).await {
                Ok(ack) => {
                    trace(
                        crate::diagnostics::Stage::Send,
                        crate::diagnostics::Reason::Ready,
                    );
                    let account = events.account.clone();
                    let result = persist_ack(db, &account, id, ack).await;
                    match result{Ok((message,conversation))=>events.emit(LiveEvent::Sent{account_id:events.account.clone(),generation,thread,request_id,message,conversation}).await?,Err(_)=>events.emit(LiveEvent::SendFailed{account_id:events.account.clone(),generation,thread,request_id,error:"Zalo đã nhận nhưng chưa lưu được xác nhận trên thiết bị. Không gửi lại.".into(),unknown_delivery:true,server_accepted:true}).await?,}
                }
                Err(error) => {
                    let _ = diagnostic(&error, LiveStage::Send);
                    let known = wire_error(&error).is_some_and(|e| {
                        matches!(e, WireError::Service(_) | WireError::Authentication)
                    });
                    let expired = wire_error(&error) == Some(&WireError::Authentication);
                    let account = events.account.clone();
                    db.call(move |s| s.fail_outgoing(&account, id, !known))
                        .await?;
                    events
                        .emit(LiveEvent::SendFailed {
                            account_id: events.account.clone(),
                            generation,
                            thread,
                            request_id,
                            error: if known {
                                "Zalo từ chối lượt gửi này. Bản nháp được giữ lại."
                            } else {
                                "Chưa có xác nhận từ Zalo. Không tự gửi lại."
                            }
                            .into(),
                            unknown_delivery: !known,
                            server_accepted: false,
                        })
                        .await?;
                    if expired {
                        events
                            .diagnostic(diagnostic(&error, LiveStage::Authentication))
                            .await?;
                        events.connection(LiveConnection::Expired).await?;
                        return Ok(CommandOutcome::PauseTransport);
                    }
                }
            }
        }
    }
    Ok(CommandOutcome::Continue)
}
fn retry_delay(
    connection: Option<&Connection>,
    backoff: &mut crate::reconnect::Backoff,
) -> Duration {
    if let Some(uptime) = connection
        .and_then(|c| c.authenticated_at)
        .map(|at| at.elapsed())
    {
        backoff.record_healthy_connection(uptime);
    }
    backoff.next_delay() + Duration::from_millis(u64::from(rand::random::<u8>()))
}
async fn run_inner(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    mut commands: mpsc::Receiver<LiveCommand>,
    events: &Events,
    transports: &Transports,
) -> Result<()> {
    let db = Database::open(profile, events.account.clone()).await?;
    let account = events.account.clone();
    let conversations = db
        .call(move |s| s.conversations(&account, crate::domain::MAX_PAGE_SIZE))
        .await?;
    events
        .emit(LiveEvent::Ready {
            account_id: events.account.clone(),
            conversations,
        })
        .await?;
    let client = transports.http.clone();
    let mut directory = DirectorySync::new();
    directory.request(0, session.clone(), db.handle()?, transports);
    let mut connection: Option<Connection> = None;
    let mut network_paused = false;
    let mut retries = 0_usize;
    let mut backoff = crate::reconnect::Backoff::default();
    let mut next_connect = tokio::time::Instant::now();
    let mut next_ping = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! {
            result = async { (&mut directory.running.as_mut().expect("guarded directory sync").task).await }, if directory.running.is_some() => {
                directory.completed(result,session.clone(),db.handle()?,transports);
            },
            changed = directory.updates_rx.changed() => {
                changed.map_err(|_|anyhow::anyhow!("Directory stopped"))?;
                directory.changed();
            },
            permit = events.tx.reserve(), if directory.notifications_pending() => {
                directory.publish_notification(permit.map_err(|_|anyhow::anyhow!("UI stopped"))?,&db,events).await?;
            },
            command = commands.recv() => {
                let Some(command) = command else { return Ok(()) };
                if let LiveCommand::RefreshDirectory{generation}=command {
                    directory.request(generation,session.clone(),db.handle()?,transports);
                    continue;
                }
                if matches!(command, LiveCommand::Reconnect) {
                    trace(crate::diagnostics::Stage::Connection,crate::diagnostics::Reason::Requested);
                    connection = None;
                    network_paused = false;
                    retries = 0;
                    backoff = crate::reconnect::Backoff::default();
                    next_connect = tokio::time::Instant::now();
                    continue;
                }
                match handle_command(command, &mut connection, &db, events, &session, &client).await? {
                    CommandOutcome::Shutdown => return Ok(()),
                    CommandOutcome::PauseTransport => { connection=None;network_paused=true; },
                    CommandOutcome::Continue => {},
                }
            },
            _ = tokio::time::sleep_until(next_connect), if connection.is_none() && !network_paused => {
                events.connection(if retries == 0 { LiveConnection::Connecting } else { LiveConnection::Reconnecting }).await?;
                let connecting = transports.connect(&session, retries);
                tokio::pin!(connecting);
                let result = loop {
                    tokio::select! {
                        result = async { (&mut directory.running.as_mut().expect("guarded directory sync").task).await }, if directory.running.is_some() => {
                            directory.completed(result,session.clone(),db.handle()?,transports);
                        },
                        changed = directory.updates_rx.changed() => {
                            changed.map_err(|_|anyhow::anyhow!("Directory stopped"))?;
                            directory.changed();
                        },
                        permit = events.tx.reserve(), if directory.notifications_pending() => {
                            directory.publish_notification(permit.map_err(|_|anyhow::anyhow!("UI stopped"))?,&db,events).await?;
                        },
                        result = &mut connecting => break Some(result),
                        command = commands.recv() => {
                            let Some(command) = command else { return Ok(()) };
                            if let LiveCommand::RefreshDirectory{generation}=command {
                                directory.request(generation,session.clone(),db.handle()?,transports);
                                continue;
                            }
                            if matches!(command, LiveCommand::Reconnect) {
                                trace(crate::diagnostics::Stage::Connection,crate::diagnostics::Reason::Requested);
                                break None;
                            }
                            match handle_command(command, &mut connection, &db, events, &session, &client).await? {
                                CommandOutcome::Shutdown => return Ok(()),
                                CommandOutcome::PauseTransport => { network_paused=true;break None; },
                                CommandOutcome::Continue => {},
                            }
                        }
                    }
                };
                let Some(result) = result else {
                    retries = 0;
                    backoff = crate::reconnect::Backoff::default();
                    next_connect = tokio::time::Instant::now();
                    continue;
                };
                match result {
                    Ok(c) => {
                        connection = Some(c);
                        next_ping = tokio::time::Instant::now() + Duration::from_secs(15);
                    },
                    Err(error) => {
                        events.diagnostic(diagnostic(&error, LiveStage::Configuration)).await?;
                        if let Some(state) = terminal_transport_state(&error) {
                            network_paused = true;
                            events.connection(state).await?;
                            continue;
                        }
                        retries = retries.saturating_add(1);
                        next_connect = tokio::time::Instant::now() + retry_delay(None, &mut backoff);
                        events.connection(LiveConnection::Offline).await?;
                    }
                }
            },
            frame = async { connection.as_mut().expect("guarded connection").next().await }, if connection.is_some() => {
                let stage = if frame.as_ref().ok().and_then(Option::as_ref).is_some_and(|frame| frame.cmd == 1) || !connection.as_ref().is_some_and(|c| c.decoder.authenticated()) {LiveStage::Authentication} else {LiveStage::Synchronization};
                let refresh_ping = frame.as_ref().ok().and_then(Option::as_ref).is_some_and(|frame| frame.cmd == 1 || (frame.cmd == 2 && frame.payload.get("eventId").and_then(Value::as_i64).is_some_and(|id| connection.as_ref().is_some_and(|c| c.last_ping == Some(id)))));
                let result = match frame {
                    Ok(Some(frame)) => connection.as_mut().unwrap().process(frame, &db, events).await,
                    Ok(None) => Ok(()),
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    events.diagnostic(diagnostic(&error, stage)).await?;
                    if let Some(state) = terminal_transport_state(&error) {
                        connection = None;
                        network_paused = true;
                        events.connection(state).await?;
                        continue;
                    }
                    if error.downcast_ref::<NetworkFailure>().is_none() {
                        return Err(error);
                    }
                    next_connect = tokio::time::Instant::now() + retry_delay(connection.as_ref(), &mut backoff);
                    connection = None;
                    retries = retries.saturating_add(1);
                    events.connection(LiveConnection::Reconnecting).await?;
                } else if refresh_ping && connection.as_ref().is_some_and(|c|c.last_ping.is_none()) {
                    next_ping = tokio::time::Instant::now() + Duration::from_secs(30);
                }
            },
            _ = tokio::time::sleep_until(next_ping), if connection.is_some() => {
                trace(crate::diagnostics::Stage::Heartbeat,crate::diagnostics::Reason::Requested);
                let c = connection.as_mut().unwrap();
                if !c.decoder.authenticated() || c.last_ping.is_some() {
                    let stage=if c.decoder.authenticated(){LiveStage::Heartbeat}else{LiveStage::Authentication};
                    events.diagnostic(NetworkFailure::new(stage,LiveCategory::Timeout).0).await?;
                    next_connect = tokio::time::Instant::now() + retry_delay(connection.as_ref(), &mut backoff);
                    connection = None;
                    retries = retries.saturating_add(1);
                    events.connection(LiveConnection::Reconnecting).await?;
                    continue;
                }
                let now = now_ms();
                c.last_ping = Some(now);
                if let Err(error)=c.send(2, 1, json!({"eventId": now})).await {
                    events.diagnostic(diagnostic(&error,LiveStage::Heartbeat)).await?;
                    next_connect = tokio::time::Instant::now() + retry_delay(connection.as_ref(), &mut backoff);
                    connection = None;
                    retries = retries.saturating_add(1);
                }
                // A matching server pong schedules the normal 30-second interval.
                next_ping = tokio::time::Instant::now() + Duration::from_secs(5);
            }
        }
    }
}
