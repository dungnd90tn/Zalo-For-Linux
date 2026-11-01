use super::*;
use crate::{
    protocol::messaging::{self as wire, DecodedFrame, FrameDecoder, Target, WireError},
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

#[derive(Debug)]
struct NetworkFailure;
impl std::fmt::Display for NetworkFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Messaging connection interrupted")
    }
}
impl std::error::Error for NetworkFailure {}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Job = Box<dyn FnOnce(&mut MessageStore) + Send>;
struct Database {
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
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
        let (tx, rx) = oneshot::channel();
        self.jobs
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Storage stopped"))?
            .send(Box::new(move |store| {
                let _ = tx.send(job(store));
            }))
            .await
            .map_err(|_| anyhow::anyhow!("Storage stopped"))?;
        rx.await.map_err(|_| anyhow::anyhow!("Storage stopped"))?
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
        self.emit(LiveEvent::Connection {
            account_id: self.account.clone(),
            state,
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
                .map_err(|_| NetworkFailure)?;
            let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
                request,
                tcp,
                Some(Connection::socket_config()),
                Some(tokio_tungstenite::Connector::Rustls(tls.clone())),
            )
            .await
            .map_err(|_| NetworkFailure)?;
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
    let http = match client() {
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
    tokio::select! {biased; _=cancel.changed()=>{},result=run_inner(profile,session,commands,&event,&transports)=>{if result.is_err(){
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
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 ZaloPC NativeLinux")
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
        .map_err(|_| NetworkFailure)?
        .map_err(|_| NetworkFailure)?;
        Ok(())
    }
    async fn next(&mut self) -> Result<Option<DecodedFrame>> {
        match self.socket.next().await {
            Some(Ok(tungstenite::Message::Binary(bytes))) => Ok(Some(self.decoder.decode(&bytes)?)),
            Some(Ok(tungstenite::Message::Ping(bytes))) => {
                tokio::time::timeout(
                    Duration::from_secs(5),
                    self.socket.send(tungstenite::Message::Pong(bytes)),
                )
                .await
                .map_err(|_| NetworkFailure)?
                .map_err(|_| NetworkFailure)?;
                Ok(None)
            }
            Some(Ok(tungstenite::Message::Pong(_))) => Ok(None),
            Some(Ok(tungstenite::Message::Close(_))) | None => Err(NetworkFailure.into()),
            Some(Ok(_)) => Err(WireError::Invalid.into()),
            Some(Err(_)) => Err(NetworkFailure.into()),
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
        let url = wire::socket_url(root, now_ms())?;
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
        request
            .headers_mut()
            .insert("Origin", "https://chat.zalo.me".parse()?);
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
        .map_err(|_| NetworkFailure)?
        .map_err(|_| NetworkFailure)?;
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
                self.authenticated_at = Some(tokio::time::Instant::now());
                let commands = frame
                    .payload
                    .pointer("/data/qCmds")
                    .and_then(Value::as_array)
                    .filter(|v| v.len() <= 64)
                    .ok_or(WireError::Invalid)?;
                for command in commands {
                    let cmd = command
                        .get("cmd")
                        .and_then(Value::as_u64)
                        .ok_or(WireError::Invalid)?;
                    let sub = command
                        .get("subCmd")
                        .and_then(Value::as_u64)
                        .ok_or(WireError::Invalid)?;
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
                if frame.payload.get("eventId").and_then(Value::as_i64) == self.last_ping {
                    self.last_ping = None;
                    self.last_pong = tokio::time::Instant::now();
                }
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
                        let cmd = cmd.parse::<u16>()?;
                        let sub = sub.parse::<u8>()?;
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
        .append_pair("zpw_type", wire::API_TYPE)
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
    let url = wire::send_url(&root, target.kind)?;
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
            Err(error) => last = Some(error),
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("Cannot store acknowledgement")))
}
async fn handle_command(
    command: LiveCommand,
    connection: &mut Option<Connection>,
    db: &Database,
    events: &Events,
    session: &VerifiedSession,
    client: &reqwest::Client,
) -> Result<bool> {
    match command {
        LiveCommand::Shutdown => return Ok(false),
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
                .call(move |s| s.open_conversation(&account, &thread, &title))
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
                    let message = if error
                        .downcast_ref::<WireError>()
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
                    return Ok(true);
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
                Err(_) => {
                    events.emit(LiveEvent::SendFailed{account_id:events.account.clone(),generation,thread,request_id,error:"Bản nháp đã có lượt gửi chưa xác định, hoặc không thể lưu khóa gửi an toàn. Không tự gửi lại.".into(),unknown_delivery:true,server_accepted:false}).await?;
                    return Ok(true);
                }
            };
            match send_text(client, session, &target, &text, id, visibility).await {
                Ok(ack) => {
                    let account = events.account.clone();
                    let result = persist_ack(db, &account, id, ack).await;
                    match result{Ok((message,conversation))=>events.emit(LiveEvent::Sent{account_id:events.account.clone(),generation,thread,request_id,message,conversation}).await?,Err(_)=>events.emit(LiveEvent::SendFailed{account_id:events.account.clone(),generation,thread,request_id,error:"Zalo đã nhận nhưng chưa lưu được xác nhận trên thiết bị. Không gửi lại.".into(),unknown_delivery:true,server_accepted:true}).await?,}
                }
                Err(error) => {
                    let known = error.downcast_ref::<WireError>().is_some_and(|e| {
                        matches!(e, WireError::Service(_) | WireError::Authentication)
                    });
                    let expired =
                        error.downcast_ref::<WireError>() == Some(&WireError::Authentication);
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
                        events.connection(LiveConnection::Expired).await?;
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
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
    let mut connection: Option<Connection> = None;
    let mut retries = 0_usize;
    let mut backoff = crate::reconnect::Backoff::default();
    let mut next_connect = tokio::time::Instant::now();
    let mut next_ping = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { return Ok(()) };
                if !handle_command(command, &mut connection, &db, events, &session, &client).await? {
                    return Ok(());
                }
            },
            _ = tokio::time::sleep_until(next_connect), if connection.is_none() => {
                events.connection(if retries == 0 { LiveConnection::Connecting } else { LiveConnection::Reconnecting }).await?;
                match transports.connect(&session, retries).await {
                    Ok(c) => {
                        connection = Some(c);
                        next_ping = tokio::time::Instant::now() + Duration::from_secs(15);
                    },
                    Err(error) => {
                        if let Some(wire) = error.downcast_ref::<WireError>() {
                            if *wire == WireError::Authentication {
                                events.connection(LiveConnection::Expired).await?;
                            } else {
                                events.emit(LiveEvent::Fault {
                                    account_id: events.account.clone(),
                                    error: "Máy chủ chưa cung cấp cấu hình nhắn tin được hỗ trợ.".into(),
                                }).await?;
                            }
                            return Ok(());
                        }
                        retries = retries.saturating_add(1);
                        next_connect = tokio::time::Instant::now() + retry_delay(None, &mut backoff);
                        events.connection(LiveConnection::Offline).await?;
                    }
                }
            },
            frame = async { connection.as_mut().expect("guarded connection").next().await }, if connection.is_some() => {
                let refresh_ping = frame.as_ref().ok().and_then(Option::as_ref).is_some_and(|frame| frame.cmd == 1 || frame.cmd == 2);
                let result = match frame {
                    Ok(Some(frame)) => connection.as_mut().unwrap().process(frame, &db, events).await,
                    Ok(None) => Ok(()),
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    if error.downcast_ref::<WireError>() == Some(&WireError::Authentication) {
                        events.connection(LiveConnection::Expired).await?;
                        return Ok(());
                    }
                    if error.downcast_ref::<NetworkFailure>().is_none() {
                        events.emit(LiveEvent::Fault {
                            account_id: events.account.clone(),
                            error: "Định dạng hoặc hàng đợi tin nhắn chưa được hỗ trợ. Đã dừng kết nối để giữ nguyên lịch sử.".into(),
                        }).await?;
                        return Ok(());
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
                let c = connection.as_mut().unwrap();
                if !c.decoder.authenticated() || c.last_ping.is_some() {
                    next_connect = tokio::time::Instant::now() + retry_delay(connection.as_ref(), &mut backoff);
                    connection = None;
                    retries = retries.saturating_add(1);
                    events.connection(LiveConnection::Reconnecting).await?;
                    continue;
                }
                let now = now_ms();
                c.last_ping = Some(now);
                if c.send(2, 1, json!({"eventId": now})).await.is_err() {
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
