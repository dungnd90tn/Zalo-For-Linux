//! Native QR authentication with isolated transport, bounded trust worker and durable commit.
use super::{
    bootstrap::{self, LoginInfoInput, PendingCookies, ServerInfoInput},
    http::HttpAdapter,
    qr::{self, HttpReply, QrRequestInput, RequestSpec, MAX_RESPONSE_BYTES},
    session::{self, AuthRecord, SessionProof, StoredSession, StoredTrust, VerifiedSession},
    trust::{CompanionProof, IdentityKeys, LinkingSecret, TrustHost},
};
use crate::profile::{prepare_secret, Profile};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::{rngs::OsRng, RngCore};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot, watch};
use url::Url;

const COMMITTING: u64 = 1 << 63;
const GENERATION_MASK: u64 = COMMITTING - 1;
const OVERALL_SECONDS: u64 = 300;
const CLOCK_SKEW_MS: u64 = 120_000;
const EVENT_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStage {
    CreatingQr,
    WaitingForScan,
    WaitingForPhone,
    VerifyingTrust,
    Bootstrapping,
    Restoring,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginFailure {
    Busy,
    Cancelled,
    Expired,
    Network,
    Rejected,
    Unsupported,
    Malformed,
    Trust,
    AccountMismatch,
    Storage,
    Internal,
}
impl std::fmt::Display for LoginFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "Đang xử lý đăng nhập; vui lòng thử lại.",
            Self::Cancelled => "Đã hủy đăng nhập.",
            Self::Expired => "Mã QR đã hết hạn. Hãy tạo mã mới.",
            Self::Network => "Không thể kết nối máy chủ đăng nhập.",
            Self::Rejected => "Máy chủ từ chối đăng nhập.",
            Self::Unsupported => "Máy chủ yêu cầu giao thức đăng nhập chưa được hỗ trợ.",
            Self::Malformed => "Phản hồi đăng nhập không hợp lệ.",
            Self::Trust => "Không xác minh được liên kết thiết bị.",
            Self::AccountMismatch => "Tài khoản không khớp hồ sơ. Hãy dùng hồ sơ khác.",
            Self::Storage => "Không lưu được phiên đăng nhập an toàn.",
            Self::Internal => "Không khởi tạo được đăng nhập.",
        })
    }
}
impl std::error::Error for LoginFailure {}
pub struct QrPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
pub enum LoginEvent {
    Progress {
        generation: u64,
        stage: LoginStage,
    },
    Qr {
        generation: u64,
        pixels: QrPixels,
    },
    Ready {
        generation: u64,
        session: Arc<VerifiedSession>,
    },
    SignedOut {
        generation: u64,
    },
    Failed {
        generation: u64,
        error: LoginFailure,
    },
}
enum Control {
    Begin(u64),
    Resume(u64),
    Cancel(Cancellation),
    Logout(u64),
    Stop,
}
#[derive(Clone, Copy)]
struct Cancellation {
    generation: u64,
    attempt: u64,
}
pub struct LoginController {
    controls: mpsc::Sender<Control>,
    epoch: Arc<AtomicU64>,
    changed: watch::Sender<u64>,
    cancelled: watch::Sender<Cancellation>,
    attempt: AtomicU64,
    pub events: mpsc::Receiver<LoginEvent>,
}
impl LoginController {
    pub fn new(
        profile: Arc<Profile>,
        trust_module: Option<PathBuf>,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        if trust_module.as_ref().is_some_and(|p| !p.is_absolute()) {
            bail!("trust module override must be absolute");
        }
        let (controls, rx) = mpsc::channel(8);
        let (tx, events) = mpsc::channel(EVENT_CAPACITY);
        let epoch = Arc::new(AtomicU64::new(0));
        let (changed, notice) = watch::channel(0);
        let (cancelled, cancel_notice) = watch::channel(Cancellation {
            generation: 0,
            attempt: 0,
        });
        let worker_epoch = epoch.clone();
        thread::Builder::new()
            .name("zalo-auth".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(runtime) = runtime {
                    runtime.block_on(controller_loop(
                        profile,
                        trust_module,
                        ControllerChannels {
                            rx,
                            tx,
                            epoch: worker_epoch,
                            notice,
                            cancel_notice,
                            repaint,
                        },
                    ));
                }
            })?;
        Ok(Self {
            controls,
            epoch,
            changed,
            cancelled,
            attempt: AtomicU64::new(0),
            events,
        })
    }
    fn advance(&self) -> u64 {
        let mut old = self.epoch.load(Ordering::Acquire);
        loop {
            let next = ((old & GENERATION_MASK).wrapping_add(1) & GENERATION_MASK).max(1);
            match self
                .epoch
                .compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    self.changed.send_replace(next);
                    return next;
                }
                Err(value) => old = value,
            }
        }
    }
    pub fn begin(&self) -> Result<u64> {
        let permit = self
            .controls
            .try_reserve()
            .map_err(|_| LoginFailure::Busy)?;
        let generation = self.advance();
        self.attempt.fetch_max(generation, Ordering::AcqRel);
        permit.send(Control::Begin(generation));
        Ok(generation)
    }
    pub fn resume(&self) -> Result<u64> {
        let permit = self
            .controls
            .try_reserve()
            .map_err(|_| LoginFailure::Busy)?;
        let generation = self.advance();
        self.attempt.fetch_max(generation, Ordering::AcqRel);
        permit.send(Control::Resume(generation));
        Ok(generation)
    }
    pub fn cancel(&self) -> u64 {
        let generation = self.advance();
        // Repeated cancellation retains the attempt identity even when watch coalesces.
        // A later Begin/Resume replaces that identity; stale starts cannot overwrite it.
        let attempt = self.attempt.load(Ordering::Acquire);
        self.cancelled.send_replace(Cancellation {
            generation,
            attempt: if attempt <= generation { attempt } else { 0 },
        });
        generation
    }
    pub fn logout(&self) -> Result<u64> {
        let permit = self
            .controls
            .try_reserve()
            .map_err(|_| LoginFailure::Busy)?;
        let generation = self.advance();
        permit.send(Control::Logout(generation));
        Ok(generation)
    }
}
impl Drop for LoginController {
    fn drop(&mut self) {
        self.advance();
        let _ = self.controls.try_send(Control::Stop);
    }
}
fn emit(tx: &mpsc::Sender<LoginEvent>, event: LoginEvent, repaint: &Arc<dyn Fn() + Send + Sync>) {
    let _ = tx.try_send(event);
    repaint();
}
async fn deliver(
    tx: &mpsc::Sender<LoginEvent>,
    event: LoginEvent,
    generation: u64,
    epoch: &AtomicU64,
    notice: &watch::Receiver<u64>,
    repaint: &Arc<dyn Fn() + Send + Sync>,
) {
    let mut cancellation = notice.clone();
    let _ = *cancellation.borrow_and_update();
    if epoch.load(Ordering::Acquire) & GENERATION_MASK != generation {
        return;
    }
    tokio::select! {biased;_=cancellation.changed()=>{},result=tx.send(event)=>{if result.is_ok(){repaint();}}}
}
struct ControllerChannels {
    rx: mpsc::Receiver<Control>,
    tx: mpsc::Sender<LoginEvent>,
    epoch: Arc<AtomicU64>,
    notice: watch::Receiver<u64>,
    cancel_notice: watch::Receiver<Cancellation>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}
async fn controller_loop(
    profile: Arc<Profile>,
    module: Option<PathBuf>,
    channels: ControllerChannels,
) {
    let ControllerChannels {
        mut rx,
        tx,
        epoch,
        mut notice,
        mut cancel_notice,
        repaint,
    } = channels;
    let last_commit = AtomicU64::new(0);
    loop {
        let command = tokio::select! {biased;changed=cancel_notice.changed()=>{if changed.is_err(){break;}Control::Cancel(*cancel_notice.borrow_and_update())},command=rx.recv()=>{let Some(command)=command else{break};command}};
        let (generation, resume) = match command {
            Control::Stop => break,
            Control::Cancel(cancel) => {
                let g = cancel.generation;
                let result = cancel_committed(&profile, &last_commit, cancel);
                let event = match result {
                    Ok(()) => LoginEvent::SignedOut { generation: g },
                    Err(_) => LoginEvent::Failed {
                        generation: g,
                        error: LoginFailure::Storage,
                    },
                };
                deliver(&tx, event, g, &epoch, &notice, &repaint).await;
                continue;
            }
            Control::Logout(g) => {
                let result = logout_record(&profile);
                deliver(
                    &tx,
                    match result {
                        Ok(()) => LoginEvent::SignedOut { generation: g },
                        Err(_) => LoginEvent::Failed {
                            generation: g,
                            error: LoginFailure::Storage,
                        },
                    },
                    g,
                    &epoch,
                    &notice,
                    &repaint,
                )
                .await;
                continue;
            }
            Control::Begin(g) => (g, false),
            Control::Resume(g) => (g, true),
        };
        if epoch.load(Ordering::Acquire) & GENERATION_MASK != generation {
            continue;
        }
        let _ = *notice.borrow_and_update();
        let context = Attempt {
            generation,
            tx: &tx,
            epoch: &epoch,
            last_commit: &last_commit,
            repaint: &repaint,
        };
        let work = authenticate(profile.clone(), module.clone(), resume, &context);
        let result = tokio::select! {biased;_ = notice.changed()=>Err(LoginFailure::Cancelled),result=tokio::time::timeout(Duration::from_secs(OVERALL_SECONDS),work)=>result.unwrap_or(Err(LoginFailure::Expired))};
        if epoch.load(Ordering::Acquire) & GENERATION_MASK == generation {
            if let Err(error) = result {
                deliver(
                    &tx,
                    LoginEvent::Failed { generation, error },
                    generation,
                    &epoch,
                    &notice,
                    &repaint,
                )
                .await;
            }
        }
    }
}

fn cancel_committed(
    profile: &Profile,
    last_commit: &AtomicU64,
    cancel: Cancellation,
) -> Result<()> {
    let commit = last_commit.load(Ordering::Acquire);
    if commit != 0 && commit == cancel.attempt {
        logout_record(profile)?;
    }
    Ok(())
}
fn logout_record(profile: &Profile) -> Result<()> {
    if let Some(mut record) = AuthRecord::load(profile)? {
        record.active = None;
        record.write(profile)?;
    }
    Ok(())
}
fn now_ms() -> std::result::Result<u64, LoginFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .map_err(|_| LoginFailure::Internal)
}
fn nonce() -> String {
    let mut bytes = [0; 6];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn device_id() -> String {
    let mut bytes = [0; 16];
    OsRng.fill_bytes(&mut bytes);
    let text: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &text[..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..]
    )
}
async fn fetch(
    http: &mut HttpAdapter,
    request: &RequestSpec,
) -> std::result::Result<HttpReply, LoginFailure> {
    http.fetch(request).await.map_err(|_| LoginFailure::Network)
}
fn envelope(reply: HttpReply) -> std::result::Result<(i64, Value), LoginFailure> {
    if reply.status != 200 || reply.body.len() > MAX_RESPONSE_BYTES {
        return Err(LoginFailure::Network);
    }
    let value: Value = serde_json::from_slice(&reply.body).map_err(|_| LoginFailure::Malformed)?;
    let code = value
        .get("error_code")
        .and_then(Value::as_i64)
        .ok_or(LoginFailure::Malformed)?;
    Ok((code, value.get("data").cloned().unwrap_or(Value::Null)))
}
fn bounded_string(
    value: &Value,
    key: &str,
    limit: usize,
) -> std::result::Result<String, LoginFailure> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(LoginFailure::Malformed)
}
fn bytes64(value: &Value, key: &str, limit: usize) -> std::result::Result<Vec<u8>, LoginFailure> {
    let text = bounded_string(value, key, limit * 2)?;
    let bytes = STANDARD.decode(text).map_err(|_| LoginFailure::Malformed)?;
    if bytes.len() > limit {
        return Err(LoginFailure::Malformed);
    }
    Ok(bytes)
}
fn integer(value: &Value, key: &str) -> std::result::Result<u64, LoginFailure> {
    value
        .get(key)
        .and_then(|v| {
            v.as_u64().or_else(|| {
                v.as_str()
                    .filter(|s| s.len() <= 20)
                    .and_then(|s| s.parse().ok())
            })
        })
        .ok_or(LoginFailure::Malformed)
}
struct QrDescriptor {
    scan: Url,
    confirm: Url,
    token: String,
    timeout: u64,
    retries: u64,
    uid_hint: Option<String>,
    master_hint: Option<String>,
}
fn check_optional_png(value: &Value) -> std::result::Result<(), LoginFailure> {
    let Some(text) = value
        .get("base64_qr")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };
    if text.len() > 700_000 {
        return Err(LoginFailure::Malformed);
    }
    let png = STANDARD.decode(text).map_err(|_| LoginFailure::Malformed)?;
    if png.len() > 512 * 1024 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(LoginFailure::Malformed);
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_limits(png::Limits {
        bytes: 4 * 1024 * 1024,
    });
    let reader = decoder.read_info().map_err(|_| LoginFailure::Malformed)?;
    let info = reader.info();
    if info.width == 0
        || info.height == 0
        || info.width > 1024
        || info.height > 1024
        || u64::from(info.width) * u64::from(info.height) > 512 * 512
        || info.animation_control.is_some()
    {
        return Err(LoginFailure::Malformed);
    }
    // Server PNG is never rendered in trusted mode. QR pixels are regenerated
    // from locally verified linking content, so compressed pixels are not decoded.
    Ok(())
}
fn reject_unexpected_linking(value: &Value) -> std::result::Result<(), LoginFailure> {
    for key in ["enc_link_data", "enc_link_hmac"] {
        if let Some(field) = value.pointer(&format!("/link_data/{key}")) {
            if !matches!(field,Value::String(text)if text.is_empty()) {
                return Err(LoginFailure::Trust);
            }
        }
    }
    Ok(())
}
fn server_qr_pixels(value: &Value) -> std::result::Result<QrPixels, LoginFailure> {
    check_optional_png(value)?;
    let text = bounded_string(value, "base64_qr", 700_000)?;
    let bytes = STANDARD.decode(text).map_err(|_| LoginFailure::Malformed)?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: 4 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| LoginFailure::Malformed)?;
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= 1024 * 1024)
        .ok_or(LoginFailure::Malformed)?;
    let mut output = vec![0; size];
    let info = reader
        .next_frame(&mut output)
        .map_err(|_| LoginFailure::Malformed)?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(LoginFailure::Malformed);
    }
    let count = info.width as usize * info.height as usize;
    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in output[..info.buffer_size()].chunks_exact(match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => return Err(LoginFailure::Malformed),
    }) {
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(pixel),
            png::ColorType::Rgb => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            png::ColorType::GrayscaleAlpha => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]])
            }
            png::ColorType::Grayscale => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255])
            }
            _ => return Err(LoginFailure::Malformed),
        }
    }
    if rgba.len() != count * 4 {
        return Err(LoginFailure::Malformed);
    }
    Ok(QrPixels {
        width: info.width,
        height: info.height,
        rgba,
    })
}
impl QrDescriptor {
    #[cfg(test)]
    fn parse(value: &Value, second: bool) -> std::result::Result<Self, LoginFailure> {
        Self::parse_mode(value, second, true)
    }
    fn parse_mode(
        value: &Value,
        second: bool,
        trusted: bool,
    ) -> std::result::Result<Self, LoginFailure> {
        check_optional_png(value)?;
        let timeout = integer(value, "qr_timeout")?;
        let retries = integer(value, "qr_retry")?;
        if !(1..=180).contains(&timeout) || !(1..=120).contains(&retries) {
            return Err(LoginFailure::Malformed);
        }
        let scan = qr::validate_poll_url(&bounded_string(value, "chk_wait_scan", 8192)?)
            .map_err(|_| LoginFailure::Malformed)?;
        let confirm = qr::validate_poll_url(&bounded_string(value, "chk_wait_cfirm", 8192)?)
            .map_err(|_| LoginFailure::Malformed)?;
        if confirm.query_pairs().any(|(key, _)| {
            matches!(
                key.as_ref(),
                "encLinkingMetaData" | "encAccountSign" | "encDeviceSign" | "encPK"
            )
        }) {
            return Err(LoginFailure::Malformed);
        }
        let token = if trusted {
            bounded_string(value, if second { "ntk" } else { "token_id" }, 512)?
        } else {
            String::new()
        };
        let uid_hint = value
            .get("uid")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if uid_hint
            .as_ref()
            .is_some_and(|s| !session::valid_account(s))
        {
            return Err(LoginFailure::Malformed);
        }
        let master_hint = value
            .get("master_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if master_hint
            .as_ref()
            .is_some_and(|s| s.len() > 128 || s.chars().any(char::is_control))
        {
            return Err(LoginFailure::Malformed);
        }
        Ok(Self {
            scan,
            confirm,
            token,
            timeout,
            retries,
            uid_hint,
            master_hint,
        })
    }
    fn request(&self, confirm: bool) -> RequestSpec {
        let mut url = if confirm {
            self.confirm.clone()
        } else {
            self.scan.clone()
        };
        if !confirm {
            let retained: Vec<_> = url
                .query_pairs()
                .filter(|(k, _)| k != "lang")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            url.set_query(None);
            url.query_pairs_mut()
                .extend_pairs(retained)
                .append_pair("lang", "vi");
        }
        RequestSpec {
            url,
            timeout_ms: self.timeout * 1000,
            max_response_bytes: MAX_RESPONSE_BYTES,
            include_cookies: true,
            follow_redirects: false,
        }
    }
}
fn qr_pixels(text: &str) -> std::result::Result<QrPixels, LoginFailure> {
    if text.len() > 2048 {
        return Err(LoginFailure::Malformed);
    }
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|_| LoginFailure::Malformed)?;
    let width = code.width();
    if width > 177 {
        return Err(LoginFailure::Malformed);
    }
    let scale = (512 / (width + 8)).clamp(1, 4);
    let side = (width + 8) * scale;
    let mut rgba = vec![255; side * side * 4];
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == qrcode::types::Color::Dark {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let index = ((y + 4) * scale + dy) * side * 4 + ((x + 4) * scale + dx) * 4;
                        rgba[index..index + 3].fill(0);
                    }
                }
            }
        }
    }
    Ok(QrPixels {
        width: side as u32,
        height: side as u32,
        rgba,
    })
}

mod trust_worker;
use trust_worker::{TrustKeys, TrustWorker};
struct SignedList {
    encoded: Vec<u8>,
    master: String,
    primary: Vec<u8>,
    timestamp: u64,
}
fn varint(mut value: u64, out: &mut Vec<u8>) {
    while value > 127 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn field_bytes(tag: u64, bytes: &[u8], out: &mut Vec<u8>) {
    varint(tag * 8 + 2, out);
    varint(bytes.len() as u64, out);
    out.extend_from_slice(bytes);
}
fn field_int(tag: u64, value: u64, out: &mut Vec<u8>) {
    if value != 0 {
        varint(tag * 8, out);
        varint(value, out);
    }
}
fn signed_list(
    devices: &Value,
    proof: &CompanionProof,
    companion: &[u8],
    earliest: u64,
    now: u64,
    previous: Option<u64>,
) -> std::result::Result<SignedList, LoginFailure> {
    let master = bounded_string(devices, "masterId", 128)?;
    let primary = bytes64(devices, "encIdentity", 33)?;
    let signature = bytes64(devices, "encSignature", 64)?;
    let timestamp = integer(devices, "lastUpdateTs")?;
    if master != proof.master_id
        || primary != proof.primary_public_key
        || primary.len() != 33
        || signature.len() != 64
        || timestamp < earliest.saturating_sub(CLOCK_SKEW_MS)
        || timestamp < proof.linking_timestamp_ms
        || timestamp > now.saturating_add(CLOCK_SKEW_MS)
        || previous.is_some_and(|last| timestamp <= last)
    {
        return Err(LoginFailure::Trust);
    }
    let companions = devices
        .get("companions")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or(LoginFailure::Malformed)?;
    let mut wrapper = Vec::new();
    let mut selected = 0;
    for item in companions {
        let id = integer(item, "id")?;
        let link = integer(item, "linkId")?;
        let public = bytes64(item, "encIdentity", 33)?;
        let platform = integer(item, "platform")?;
        let status = integer(item, "status")?;
        let expiration = integer(item, "expireTs")?;
        let account = bytes64(item, "encAccSign", 64)?;
        let device = bytes64(item, "encDeviceSign", 64)?;
        if id > u32::MAX as u64
            || public.len() != 33
            || account.len() != 64
            || device.len() != 64
            || !(1..=2).contains(&platform)
            || status > 2
        {
            return Err(LoginFailure::Malformed);
        }
        if public == companion {
            selected += 1;
            if id != proof.companion_id as u64
                || link != proof.link_id
                || platform != 2
                || status == 2
                || expiration < now
                || account != proof.account_signature
                || device != proof.device_signature
            {
                return Err(LoginFailure::Trust);
            }
        }
        let mut record = Vec::new();
        field_int(1, id, &mut record);
        field_bytes(2, &public, &mut record);
        field_int(3, link, &mut record);
        field_int(4, platform - 1, &mut record);
        field_int(5, status, &mut record);
        field_bytes(6, &account, &mut record);
        field_bytes(7, &device, &mut record);
        field_int(8, expiration, &mut record);
        field_bytes(1, &record, &mut wrapper);
    }
    if selected != 1 {
        return Err(LoginFailure::Trust);
    }
    let mut encoded = Vec::new();
    field_bytes(1, master.as_bytes(), &mut encoded);
    field_bytes(2, &primary, &mut encoded);
    field_bytes(3, &wrapper, &mut encoded);
    encoded.push(0x21);
    encoded.extend_from_slice(&timestamp.to_le_bytes());
    field_bytes(5, &signature, &mut encoded);
    Ok(SignedList {
        encoded,
        master,
        primary,
        timestamp,
    })
}
struct PollHttp<'a>(&'a mut HttpAdapter);
impl qr::QrTransport for PollHttp<'_> {
    async fn get(&mut self, request: &RequestSpec) -> Result<HttpReply> {
        self.0.fetch_poll(request).await.map_err(anyhow::Error::new)
    }
}
async fn poll<T: qr::QrTransport>(
    http: &mut T,
    descriptor: &QrDescriptor,
    confirm: bool,
) -> std::result::Result<(i64, Value), LoginFailure> {
    let mut last_timeout = false;
    for attempt in 0..descriptor.retries {
        match http.get(&descriptor.request(confirm)).await {
            Ok(reply) => {
                let (code, data) = envelope(reply)?;
                if matches!(code, 0 | -20 | -21 | 18010) {
                    return Ok((code, data));
                }
                last_timeout = code == 408;
            }
            Err(_) if attempt + 1 == descriptor.retries => return Err(LoginFailure::Network),
            Err(_) => {}
        }
        if attempt + 1 < descriptor.retries {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    Err(if last_timeout {
        LoginFailure::Expired
    } else {
        LoginFailure::Rejected
    })
}

async fn bootstrap_authenticated(
    http: &mut HttpAdapter,
    imei: &str,
    pending: &PendingCookies,
) -> std::result::Result<bootstrap::LoginInfo, LoginFailure> {
    http.stage_verified_confirmation(pending)
        .map_err(|_| LoginFailure::Network)?;
    let time = now_ms()?;
    let random = nonce();
    let (request, cipher) = bootstrap::login_info_request(&LoginInfoInput {
        imei,
        computer_name: "Zalo Linux",
        language: "vi",
        timestamp_ms: time,
        nonce_hex: &random,
    })
    .map_err(|_| LoginFailure::Internal)?;
    let reply = fetch(http, &request).await?;
    let result = bootstrap::decode_login_reply(reply.status, &reply.body, &cipher)
        .map_err(|_| LoginFailure::Rejected)?;
    if result.account_id != pending.account_id {
        return Err(LoginFailure::AccountMismatch);
    }
    Ok(result)
}
struct Attempt<'a> {
    generation: u64,
    tx: &'a mpsc::Sender<LoginEvent>,
    epoch: &'a AtomicU64,
    last_commit: &'a AtomicU64,
    repaint: &'a Arc<dyn Fn() + Send + Sync>,
}
fn validate_saved_context(
    saved: &StoredTrust,
    record: &AuthRecord,
    now: u64,
) -> std::result::Result<(), LoginFailure> {
    if saved.primary_key.len() != 33
        || saved.primary_key[0] != 5
        || saved.companion_public.len() != 33
        || saved.companion_public[0] != 5
        || saved.companion_private.len() != 32
        || saved.linking_secret.len() != 32
        || saved.signed_list.len() > 1024 * 1024
        || saved.list_timestamp > now.saturating_add(CLOCK_SKEW_MS)
        || saved.list_timestamp < saved.linking_timestamp
        || record.replay.get(&saved.master_id) != Some(&saved.list_timestamp)
    {
        return Err(LoginFailure::Storage);
    }
    // Version1 stores the canonical protobuf emitted by signed_list(). Bind its
    // actual signed master/key/timestamp to the separately stored resume context.
    let mut prefix = Vec::new();
    field_bytes(1, saved.master_id.as_bytes(), &mut prefix);
    field_bytes(2, &saved.primary_key, &mut prefix);
    let rest = saved
        .signed_list
        .strip_prefix(prefix.as_slice())
        .ok_or(LoginFailure::Storage)?;
    if rest.first() != Some(&0x1a) {
        return Err(LoginFailure::Storage);
    }
    let mut length = 0usize;
    let mut consumed = 1;
    for shift in (0..=28).step_by(7) {
        let byte = *rest.get(consumed).ok_or(LoginFailure::Storage)?;
        consumed += 1;
        length |= ((byte & 127) as usize) << shift;
        if byte & 128 == 0 {
            break;
        }
        if shift == 28 {
            return Err(LoginFailure::Storage);
        }
    }
    let suffix = rest
        .get(consumed.checked_add(length).ok_or(LoginFailure::Storage)?..)
        .ok_or(LoginFailure::Storage)?;
    if suffix.len() != 75
        || suffix[0] != 0x21
        || suffix[1..9] != saved.list_timestamp.to_le_bytes()
        || suffix[9..11] != [0x2a, 64]
    {
        return Err(LoginFailure::Storage);
    }
    Ok(())
}
async fn authenticate(
    profile: Arc<Profile>,
    module: Option<PathBuf>,
    resume: bool,
    context: &Attempt<'_>,
) -> std::result::Result<(), LoginFailure> {
    let http = HttpAdapter::new().map_err(|_| LoginFailure::Network)?;
    authenticate_with_http(profile, module, resume, context, http).await
}
async fn authenticate_with_http(
    profile: Arc<Profile>,
    module: Option<PathBuf>,
    resume: bool,
    context: &Attempt<'_>,
    mut http: HttpAdapter,
) -> std::result::Result<(), LoginFailure> {
    let Attempt {
        generation,
        tx,
        repaint,
        ..
    } = *context;
    let mut record = AuthRecord::load(&profile).map_err(|_| LoginFailure::Storage)?;
    let (saved, config) = if resume {
        emit(
            tx,
            LoginEvent::Progress {
                generation,
                stage: LoginStage::Restoring,
            },
            repaint,
        );
        let saved = record
            .as_mut()
            .and_then(|record| record.active.take())
            .ok_or(LoginFailure::Rejected)?;
        profile
            .check_account(&saved.account_id)
            .map_err(|_| LoginFailure::AccountMismatch)?;
        if let SessionProof::TrustedDevice(trust) = &saved.proof {
            validate_saved_context(
                trust,
                record.as_ref().ok_or(LoginFailure::Storage)?,
                now_ms()?,
            )?;
            let worker = TrustWorker::start(
                module,
                Some(TrustKeys {
                    public: trust.companion_public.clone(),
                    private: trust.companion_private.clone(),
                    secret: trust.linking_secret.clone(),
                }),
            )
            .await?;
            worker
                .verify(
                    trust.signed_list.clone(),
                    trust.primary_key.clone(),
                    now_ms()?,
                )
                .await?;
        }
        let mut pending_value = serde_json::json!({"uid":saved.account_id,"dkey":saved.storage_key,"zpw_sek":saved.cookie});
        if let Some(token) = &saved.refresh_token {
            pending_value["zpw_kt"] = Value::String(token.clone());
        }
        let pending =
            PendingCookies::from_confirmation(&pending_value).map_err(|_| LoginFailure::Storage)?;
        let info = bootstrap_authenticated(&mut http, &saved.imei, &pending).await?;
        let mut saved = saved;
        saved.session_key = session::encode_key(&info.session_cipher);
        (saved, info.config)
    } else {
        emit(
            tx,
            LoginEvent::Progress {
                generation,
                stage: LoginStage::CreatingQr,
            },
            repaint,
        );
        let imei = device_id();
        let local_time = now_ms()?;
        let request = bootstrap::server_info_request(&ServerInfoInput {
            imei: &imei,
            computer_name: "Zalo Linux",
            support_maintenance: None,
            bucket_id: None,
        })
        .map_err(|_| LoginFailure::Internal)?;
        let reply = fetch(&mut http, &request).await?;
        let server = bootstrap::decode_server_info(reply.status, &reply.body)
            .map_err(|_| LoginFailure::Unsupported)?;
        let trusted = match server.qr_mode.ok_or(LoginFailure::Unsupported)? {
            bootstrap::QrMode::TrustedDevice => true,
            bootstrap::QrMode::PhoneApproved => false,
        };
        let server_now = server
            .server_time_seconds
            .and_then(|s| u64::try_from(s).ok())
            .and_then(|s| s.checked_mul(1000))
            .unwrap_or(local_time);
        if server_now.abs_diff(local_time) > 86_400_000 {
            return Err(LoginFailure::Malformed);
        }
        let offset = server_now as i128 - local_time as i128;
        let adjusted_now = || -> std::result::Result<u64, LoginFailure> {
            u64::try_from(now_ms()? as i128 + offset).map_err(|_| LoginFailure::Malformed)
        };
        let random = nonce();
        let (request, cipher) = (if trusted {
            qr::create_trusted_request
        } else {
            qr::create_request
        })(&QrRequestInput {
            imei: &imei,
            computer_name: "Zalo Linux",
            language: "vi",
            first_launch_ms: local_time,
            nonce_hex: &random,
        })
        .map_err(|_| LoginFailure::Internal)?;
        let (code, value) = envelope(fetch(&mut http, &request).await?)?;
        if code != 0 {
            return Err(LoginFailure::Rejected);
        }
        let encrypted = value.as_str().ok_or(LoginFailure::Malformed)?;
        let inner = cipher
            .decrypt(encrypted)
            .map_err(|_| LoginFailure::Malformed)?;
        let (code, value) = envelope(HttpReply {
            status: 200,
            body: inner,
        })?;
        if code != 0 {
            return Err(LoginFailure::Rejected);
        }
        let mut qr_data = value;
        let mut descriptor = QrDescriptor::parse_mode(&qr_data, false, trusted)?;
        let mut accepted = None;
        for _ in 0..3 {
            let began = adjusted_now()?;
            let worker = if trusted {
                Some(TrustWorker::start(module.clone(), None).await?)
            } else {
                None
            };
            let pixels = if let Some(worker) = &worker {
                qr_pixels(&worker.qr(descriptor.token.clone(), began).await?)?
            } else {
                server_qr_pixels(&qr_data)?
            };
            tx.send(LoginEvent::Qr { generation, pixels })
                .await
                .map_err(|_| LoginFailure::Cancelled)?;
            repaint();
            emit(
                tx,
                LoginEvent::Progress {
                    generation,
                    stage: LoginStage::WaitingForScan,
                },
                repaint,
            );
            let (code, data) = poll(&mut PollHttp(&mut http), &descriptor, false).await?;
            if code == -20 {
                qr_data = data;
                descriptor = QrDescriptor::parse_mode(&qr_data, true, trusted)?;
                continue;
            }
            if code != 0 {
                return Err(LoginFailure::Expired);
            }
            emit(
                tx,
                LoginEvent::Progress {
                    generation,
                    stage: LoginStage::VerifyingTrust,
                },
                repaint,
            );
            let proof = if let Some(worker) = &worker {
                let link = data.get("link_data").ok_or(LoginFailure::Trust)?;
                let proof = worker
                    .proof(
                        bytes64(link, "enc_link_data", 128 * 1024)?,
                        bytes64(link, "enc_link_hmac", 32)?,
                    )
                    .await?;
                let current = adjusted_now()?;
                if proof.linking_timestamp_ms < began.saturating_sub(CLOCK_SKEW_MS)
                    || proof.linking_timestamp_ms > current.saturating_add(CLOCK_SKEW_MS)
                    || descriptor
                        .master_hint
                        .as_ref()
                        .is_some_and(|master| *master != proof.master_id)
                {
                    return Err(LoginFailure::Trust);
                }
                descriptor
                    .confirm
                    .query_pairs_mut()
                    .append_pair(
                        "encLinkingMetaData",
                        &STANDARD.encode(&proof.linking_metadata),
                    )
                    .append_pair("encAccountSign", &STANDARD.encode(&proof.account_signature))
                    .append_pair("encDeviceSign", &STANDARD.encode(&proof.device_signature))
                    .append_pair("encPK", &STANDARD.encode(&worker.public));
                Some(proof)
            } else {
                reject_unexpected_linking(&data)?;
                None
            };
            emit(
                tx,
                LoginEvent::Progress {
                    generation,
                    stage: LoginStage::WaitingForPhone,
                },
                repaint,
            );
            let (code, confirmed) = poll(&mut PollHttp(&mut http), &descriptor, true).await?;
            if code == -20 {
                qr_data = confirmed;
                descriptor = QrDescriptor::parse_mode(&qr_data, true, trusted)?;
                continue;
            }
            if code != 0 {
                return Err(LoginFailure::Rejected);
            }
            let pending = PendingCookies::from_confirmation(&confirmed)
                .map_err(|_| LoginFailure::Malformed)?;
            if descriptor
                .uid_hint
                .as_ref()
                .is_some_and(|uid| *uid != pending.account_id)
            {
                return Err(LoginFailure::AccountMismatch);
            }
            profile
                .check_account(&pending.account_id)
                .map_err(|_| LoginFailure::AccountMismatch)?;
            let assurance = if let (Some(worker), Some(proof)) = (&worker, proof) {
                let devices = confirmed.get("devices").ok_or(LoginFailure::Trust)?;
                let previous = record
                    .as_ref()
                    .and_then(|r| r.replay.get(&proof.master_id))
                    .copied();
                let list = signed_list(
                    devices,
                    &proof,
                    &worker.public,
                    began,
                    adjusted_now()?,
                    previous,
                )?;
                worker
                    .verify(list.encoded.clone(), list.primary.clone(), adjusted_now()?)
                    .await?;
                let keys = worker.keys().await?;
                SessionProof::TrustedDevice(StoredTrust {
                    master_id: list.master,
                    primary_key: list.primary,
                    companion_public: keys.public,
                    companion_private: keys.private,
                    linking_secret: keys.secret,
                    signed_list: list.encoded,
                    list_timestamp: list.timestamp,
                    linking_timestamp: proof.linking_timestamp_ms,
                })
            } else {
                reject_unexpected_linking(&confirmed)?;
                SessionProof::PhoneApproved
            };
            emit(
                tx,
                LoginEvent::Progress {
                    generation,
                    stage: LoginStage::Bootstrapping,
                },
                repaint,
            );
            let info = bootstrap_authenticated(&mut http, &imei, &pending).await?;
            let display_name = confirmed
                .pointer("/profile/displayName")
                .or_else(|| confirmed.pointer("/profile/display_name"))
                .and_then(Value::as_str)
                .filter(|s| s.len() <= 256 && !s.chars().any(char::is_control))
                .unwrap_or("Tài khoản Zalo")
                .to_owned();
            let saved = StoredSession {
                account_id: pending.account_id,
                display_name,
                imei: imei.clone(),
                cookie: pending.session_cookie,
                refresh_token: pending.refresh_token,
                storage_key: pending.storage_key,
                session_key: session::encode_key(&info.session_cipher),
                proof: assurance,
            };
            accepted = Some((saved, info.config));
            break;
        }
        accepted.ok_or(LoginFailure::Expired)?
    };
    publish_session(profile, record, saved, config, context).await
}
async fn publish_session(
    profile: Arc<Profile>,
    record: Option<AuthRecord>,
    saved: StoredSession,
    config: bootstrap::ServerConfig,
    context: &Attempt<'_>,
) -> std::result::Result<(), LoginFailure> {
    let Attempt {
        generation,
        tx,
        epoch,
        last_commit,
        repaint,
    } = *context;
    let account = saved.account_id.clone();
    let runtime = Arc::new(
        VerifiedSession::from_committed(&saved, config, profile.clone())
            .map_err(|_| LoginFailure::Storage)?,
    );
    let mut record = record.unwrap_or_else(|| AuthRecord::empty(&account));
    if record.owner_uid != account {
        return Err(LoginFailure::AccountMismatch);
    }
    if let SessionProof::TrustedDevice(trust) = &saved.proof {
        record
            .replay
            .insert(trust.master_id.clone(), trust.list_timestamp);
    }
    if record.replay.len() > 32 {
        return Err(LoginFailure::Trust);
    }
    // Materialize everything before the generation CAS. No cancellation before
    // that point can replace the old durable record.
    record.active = Some(saved);
    let encoded = record.encode().map_err(|_| LoginFailure::Storage)?;
    profile
        .check_account(&account)
        .map_err(|_| LoginFailure::AccountMismatch)?;
    let prepared = prepare_secret(
        &profile.paths.data_dir.join(session::AUTH_STATE_FILE),
        &encoded,
    )
    .map_err(|_| LoginFailure::Storage)?;
    let permit = tx.reserve().await.map_err(|_| LoginFailure::Cancelled)?;
    epoch
        .compare_exchange(
            generation,
            generation | COMMITTING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|_| LoginFailure::Cancelled)?;
    prepared.commit().map_err(|_| LoginFailure::Storage)?;
    last_commit.store(generation, Ordering::Release);
    let still_current = epoch
        .compare_exchange(
            generation | COMMITTING,
            generation,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok();
    if !still_current {
        logout_record(&profile).map_err(|_| LoginFailure::Storage)?;
        return Err(LoginFailure::Cancelled);
    }
    permit.send(LoginEvent::Ready {
        generation,
        session: runtime,
    });
    repaint();
    Ok(())
}

#[cfg(test)]
mod tests;
