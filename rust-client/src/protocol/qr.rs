//! Independently implemented wire primitives from this repository's Electron
//! bundle evidence. No network transport or trusted-device authentication is
//! enabled here. See PROTOCOL_QR.md for evidence and explicit gaps.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use md5::{Digest, Md5};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;
use url::Url;

pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_POLL_SECONDS: u64 = 60;
const PUBLIC_PARAMETER_KEY: &[u8; 32] = b"3FC4F0D2AB50057BCE0D90D9187A22B1";
const ZERO_IV: [u8; 16] = [0; 16];

/// All redirects must be rejected by the eventual HTTP adapter. In particular,
/// cookie-bearing login requests must never follow arbitrary server URLs.
#[derive(Clone)]
pub struct RequestSpec {
    pub url: Url,
    pub timeout_ms: u64,
    pub max_response_bytes: usize,
    pub include_cookies: bool,
    pub follow_redirects: bool,
}

impl RequestSpec {
    pub fn method(&self) -> &'static str {
        "GET"
    }
}

// Intentionally no Debug/Serialize implementation: response keys and URLs carry
// authentication material and must never appear in diagnostic logs.
pub struct ResponseCipher([u8; 32]);

impl ResponseCipher {
    pub fn decrypt(&self, encoded: &str) -> Result<Vec<u8>> {
        if encoded.len() > MAX_RESPONSE_BYTES {
            bail!("encrypted response exceeds size limit");
        }
        let encoded = percent_decode(encoded)?;
        let ciphertext = STANDARD
            .decode(encoded)
            .context("invalid response base64")?;
        cbc::Decryptor::<aes::Aes256>::new((&self.0).into(), (&ZERO_IV).into())
            .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext)
            .map_err(|_| anyhow::anyhow!("invalid encrypted response padding"))
    }
}

/// Frozen bundle API facts, not a claim about today's server configuration.
/// Caller supplies a fresh device ID and nonce; neither is read from Electron.
pub struct QrRequestInput<'a> {
    pub imei: &'a str,
    pub computer_name: &'a str,
    pub language: &'a str,
    pub first_launch_ms: u64,
    pub nonce_hex: &'a str,
}

/// Build the encrypted server-rendered, phone-approved QR request.
pub fn create_request(input: &QrRequestInput<'_>) -> Result<(RequestSpec, ResponseCipher)> {
    create_request_mode(input, false)
}
pub fn create_trusted_request(input: &QrRequestInput<'_>) -> Result<(RequestSpec, ResponseCipher)> {
    create_request_mode(input, true)
}
fn create_request_mode(
    input: &QrRequestInput<'_>,
    trusted: bool,
) -> Result<(RequestSpec, ResponseCipher)> {
    if input.imei.is_empty()
        || input.imei.len() > 128
        || input.computer_name.is_empty()
        || input.computer_name.len() > 128
    {
        bail!("device ID and computer name must contain 1–128 bytes");
    }
    if !matches!(input.language, "vi" | "en") || input.first_launch_ms == 0 {
        bail!("unsupported language or missing request time");
    }
    // Keep JSON field ordering identical to the verified on-wire plaintext
    // ordering, independently of serde_json's map implementation.
    #[derive(serde::Serialize)]
    struct Payload<'a> {
        language: &'a str,
        client_time: u64,
        imei: &'a str,
        computer_name: &'a str,
        logged_uids: &'a str,
        self_g: u8,
        qr_form: u8,
    }
    let payload = serde_json::to_vec(&Payload {
        language: input.language,
        client_time: input.first_launch_ms.saturating_add(500) / 1000,
        imei: input.imei,
        computer_name: input.computer_name,
        logged_uids: "",
        self_g: u8::from(trusted),
        qr_form: u8::from(trusted),
    })?;
    let (mut params, key) =
        encrypt_parameters(input.imei, input.first_launch_ms, input.nonce_hex, &payload)?;
    params.insert("type".into(), "30".into());
    params.insert("client_version".into(), "681".into());
    let signature = sign_parameters("reqqr", &params);
    params.insert("signkey".into(), signature);
    let mut url = validate_login_url("https://wpa.zaloapp.com/api/login/reqqr")?;
    url.query_pairs_mut().extend_pairs(params.iter());
    Ok((
        RequestSpec {
            url,
            timeout_ms: 10_000,
            max_response_bytes: MAX_RESPONSE_BYTES,
            include_cookies: true,
            follow_redirects: false,
        },
        key,
    ))
}

pub(super) fn encrypt_parameters(
    imei: &str,
    milliseconds: u64,
    nonce: &str,
    plaintext: &[u8],
) -> Result<(BTreeMap<String, String>, ResponseCipher)> {
    if !(6..=12).contains(&nonce.len()) || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("QR nonce must contain 6–12 hexadecimal characters");
    }
    let device = format!("30,{imei},{milliseconds}");
    let zcid = hex_upper(
        &cbc::Encryptor::<aes::Aes256>::new(PUBLIC_PARAMETER_KEY.into(), (&ZERO_IV).into())
            .encrypt_padded_vec_mut::<Pkcs7>(device.as_bytes()),
    );
    let hash = hex_upper(&Md5::digest(nonce.as_bytes()));
    let even_hash: Vec<_> = hash.bytes().step_by(2).collect();
    let even_device: Vec<_> = zcid.bytes().step_by(2).collect();
    let odd_device: Vec<_> = zcid.bytes().skip(1).step_by(2).collect();
    let mut key = [0_u8; 32];
    key[..8].copy_from_slice(&even_hash[..8]);
    key[8..20].copy_from_slice(&even_device[..12]);
    for (destination, value) in key[20..].iter_mut().zip(odd_device.iter().rev().take(12)) {
        *destination = *value;
    }
    let encrypted = cbc::Encryptor::<aes::Aes256>::new((&key).into(), (&ZERO_IV).into())
        .encrypt_padded_vec_mut::<Pkcs7>(plaintext);
    Ok((
        BTreeMap::from([
            ("zcid".into(), zcid),
            ("zcid_ext".into(), nonce.to_owned()),
            ("enc_ver".into(), "v2".into()),
            ("params".into(), STANDARD.encode(encrypted)),
        ]),
        ResponseCipher(key),
    ))
}

pub(super) fn sign_parameters(command: &str, params: &BTreeMap<String, String>) -> String {
    let mut hash = Md5::new();
    hash.update(b"zsecure");
    hash.update(command.as_bytes());
    for value in params.values() {
        hash.update(value.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn hex_upper(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(result, "{byte:02X}").expect("write string");
    }
    result
}

pub(super) fn percent_decode(text: &str) -> Result<Vec<u8>> {
    let input = text.as_bytes();
    let mut result = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let pair = input
                .get(index + 1..index + 3)
                .context("truncated percent escape")?;
            let high = (pair[0] as char)
                .to_digit(16)
                .context("invalid percent escape")?;
            let low = (pair[1] as char)
                .to_digit(16)
                .context("invalid percent escape")?;
            result.push((high * 16 + low) as u8);
            index += 3;
        } else {
            // decodeURIComponent leaves '+' intact; form decoding does not.
            result.push(input[index]);
            index += 1;
        }
    }
    Ok(result)
}

pub(super) fn validate_login_url(text: &str) -> Result<Url> {
    if text.len() > 8192 {
        bail!("login URL exceeds length limit");
    }
    let url = Url::parse(text).context("invalid login URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.host_str(), Some("wpa.zaloapp.com" | "wpa.chat.zalo.me"))
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
    {
        bail!("login URL must use an allowlisted HTTPS authentication host without credentials, alternate ports or fragments");
    }
    Ok(url)
}

/// Exact server-issued polling origins, separately from cookie-bearing auth origins.
#[cfg(feature = "live-login")]
pub(super) fn validate_poll_url(text: &str) -> Result<Url> {
    if text.len() > 8192 {
        bail!("poll URL exceeds length limit");
    }
    let url = Url::parse(text).map_err(|_| anyhow::anyhow!("invalid poll URL"))?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !matches!(
            url.host_str(),
            Some("wpa.zaloapp.com" | "wpa.chat.zalo.me" | "qr-id.zaloapp.com")
        )
    {
        bail!("poll URL violates authentication polling policy");
    }
    Ok(url)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrState {
    Idle,
    Requested,
    Scanned,
    AwaitingConfirmation,
    TrustRequired,
    Authenticated,
    Expired,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollStage {
    Scan,
    Confirm,
}

#[derive(Deserialize)]
struct Envelope {
    error_code: i64,
    #[serde(default)]
    data: Value,
}

#[derive(Deserialize)]
struct CreatedQr {
    base64_qr: String,
    chk_wait_scan: String,
    chk_wait_cfirm: String,
    qr_retry: u64,
    qr_timeout: u64,
}

// Secrets stay private. Do not derive Debug/Serialize for the QR flow.
pub struct QrFlow {
    state: QrState,
    generation: u64,
    scan_url: Option<Url>,
    confirm_url: Option<Url>,
    image_base64: Option<String>,
    timeout_ms: u64,
    remaining_attempts: u64,
    attempts_per_stage: u64,
    in_flight: Option<PollStage>,
    language: &'static str,
}

impl Default for QrFlow {
    fn default() -> Self {
        Self {
            state: QrState::Idle,
            generation: 0,
            scan_url: None,
            confirm_url: None,
            image_base64: None,
            timeout_ms: 0,
            remaining_attempts: 0,
            attempts_per_stage: 0,
            in_flight: None,
            language: "vi",
        }
    }
}

impl QrFlow {
    pub fn set_language(&mut self, language: &str) -> Result<()> {
        self.language = match language {
            "vi" => "vi",
            "en" => "en",
            _ => bail!("unsupported QR polling language"),
        };
        Ok(())
    }

    pub fn begin(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.state = QrState::Requested;
        self.clear_tokens();
        self.generation
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.state = QrState::Cancelled;
        self.clear_tokens();
    }

    pub fn image_base64(&self) -> Option<&str> {
        self.image_base64.as_deref()
    }
    pub fn state(&self) -> QrState {
        self.state
    }

    /// Accept only the decrypted create-QR response belonging to this attempt.
    pub fn receive_created(&mut self, generation: u64, body: &[u8]) -> Result<()> {
        self.check_generation(generation)?;
        if self.state != QrState::Requested || self.scan_url.is_some() {
            bail!("unexpected QR response state");
        }
        let envelope = parse_envelope(body)?;
        if envelope.error_code != 0 {
            bail!("QR request was rejected (code {})", envelope.error_code);
        }
        let qr: CreatedQr =
            serde_json::from_value(envelope.data).context("incomplete QR response")?;
        if qr.base64_qr.is_empty()
            || qr.base64_qr.len() > MAX_RESPONSE_BYTES
            || !(1..=MAX_POLL_SECONDS).contains(&qr.qr_timeout)
            || !(1..=120).contains(&qr.qr_retry)
        {
            bail!("invalid QR image or polling bounds");
        }
        let image = STANDARD
            .decode(&qr.base64_qr)
            .context("QR image is not valid base64")?;
        if !image.starts_with(b"\x89PNG\r\n\x1a\n") {
            bail!("QR image must be a PNG");
        }
        let scan = validate_login_url(&qr.chk_wait_scan)?;
        let confirm = validate_login_url(&qr.chk_wait_cfirm)?;
        self.scan_url = Some(scan);
        self.confirm_url = Some(confirm);
        self.image_base64 = Some(qr.base64_qr);
        self.timeout_ms = qr.qr_timeout * 1000;
        self.remaining_attempts = qr.qr_retry;
        self.attempts_per_stage = qr.qr_retry;
        Ok(())
    }

    pub fn next_poll(&mut self, stage: PollStage) -> Result<RequestSpec> {
        if self.in_flight.is_some() {
            bail!("a QR poll is already in flight");
        }
        let mut url = match (self.state, stage) {
            (QrState::Requested, PollStage::Scan) => self.scan_url.clone(),
            (QrState::Scanned, PollStage::Confirm)
            | (QrState::AwaitingConfirmation, PollStage::Confirm) => self.confirm_url.clone(),
            _ => bail!("poll stage does not match QR state"),
        }
        .context("QR polling URL is unavailable")?;
        if stage == PollStage::Scan {
            let retained: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(name, _)| name != "lang")
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect();
            url.set_query(None);
            url.query_pairs_mut()
                .extend_pairs(retained)
                .append_pair("lang", self.language);
        }
        if self.remaining_attempts == 0 {
            self.state = QrState::Expired;
            self.clear_tokens();
            bail!("QR polling budget exhausted");
        }
        self.remaining_attempts -= 1;
        self.in_flight = Some(stage);
        if stage == PollStage::Confirm {
            self.state = QrState::AwaitingConfirmation;
        }
        Ok(RequestSpec {
            url,
            timeout_ms: self.timeout_ms,
            max_response_bytes: MAX_RESPONSE_BYTES,
            include_cookies: true,
            follow_redirects: false,
        })
    }

    /// Release one failed HTTP attempt so a bounded retry can be scheduled.
    /// Network failures leave authentication and the QR stage unchanged.
    pub fn transport_failed(&mut self, generation: u64, stage: PollStage) -> Result<()> {
        self.check_generation(generation)?;
        if self.in_flight != Some(stage) {
            bail!("no matching QR poll is in flight");
        }
        self.in_flight = None;
        Ok(())
    }

    /// Caller supplies HTTP status separately so a transport failure cannot be
    /// mistaken for a successful scan or invalidate an existing account session.
    pub fn receive_poll(
        &mut self,
        generation: u64,
        stage: PollStage,
        http_status: u16,
        body: &[u8],
    ) -> Result<()> {
        self.check_generation(generation)?;
        if self.in_flight != Some(stage) {
            bail!("no matching QR poll is in flight");
        }
        self.in_flight = None;
        if !matches!(
            (self.state, stage),
            (QrState::Requested, PollStage::Scan)
                | (QrState::AwaitingConfirmation, PollStage::Confirm)
        ) {
            bail!("poll response does not match QR state");
        }
        if http_status != 200 {
            bail!("QR polling HTTP failure ({http_status})");
        }
        let response = parse_envelope(body)?;
        match response.error_code {
            0 if stage == PollStage::Scan => {
                if !response.data.is_object() {
                    bail!("scan response data is missing");
                }
                self.state = QrState::Scanned;
                self.remaining_attempts = self.attempts_per_stage;
            }
            0 => {
                let account = response
                    .data
                    .get("uid")
                    .and_then(Value::as_str)
                    .context("confirmation lacks account ID")?;
                if account.is_empty() || !account.bytes().all(|b| b.is_ascii_digit()) {
                    bail!("invalid confirmed account ID");
                }
                // Confirmation is not enough: linked-device signatures and
                // getLoginInfo must still be verified before accepting sessions.
                self.state = QrState::TrustRequired;
                self.clear_tokens();
            }
            -20 => {
                self.state = QrState::TrustRequired;
                self.clear_tokens();
            }
            -21 | 408 => {
                self.state = QrState::Expired;
                self.clear_tokens();
            }
            code => {
                bail!("QR polling rejected (code {code}); authentication state was not changed")
            }
        }
        Ok(())
    }

    fn check_generation(&self, generation: u64) -> Result<()> {
        if generation != self.generation {
            bail!("stale or cancelled QR response");
        }
        Ok(())
    }
    fn clear_tokens(&mut self) {
        self.scan_url = None;
        self.confirm_url = None;
        self.image_base64 = None;
        self.timeout_ms = 0;
        self.remaining_attempts = 0;
        self.attempts_per_stage = 0;
        self.in_flight = None;
    }
}

fn parse_envelope(body: &[u8]) -> Result<Envelope> {
    if body.len() > MAX_RESPONSE_BYTES {
        bail!("QR response exceeds size limit");
    }
    serde_json::from_slice(body).context("invalid QR response envelope")
}

pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Adapters must own an isolated cookie jar, enforce streaming byte limits,
/// reject redirects and revalidate URLs. No concrete network adapter is enabled.
pub trait QrTransport {
    fn get(&mut self, request: &RequestSpec) -> impl Future<Output = Result<HttpReply>> + Send;
}

/// Async protocol orchestration independent of the GUI and HTTP library.
pub struct QrDriver<T> {
    transport: T,
    flow: QrFlow,
}

impl<T: QrTransport> QrDriver<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            flow: QrFlow::default(),
        }
    }
    pub fn state(&self) -> QrState {
        self.flow.state()
    }
    pub fn image_base64(&self) -> Option<&str> {
        self.flow.image_base64()
    }
    pub fn cancel(&mut self) {
        self.flow.cancel();
    }

    pub async fn start(
        &mut self,
        input: &QrRequestInput<'_>,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<()> {
        let (request, cipher) = create_request(input)?;
        self.flow.set_language(input.language)?;
        let generation = self.flow.begin();
        let reply = match self.fetch(&request, cancel).await {
            Ok(reply) => reply,
            Err(error) => {
                self.flow.cancel();
                return Err(error);
            }
        };
        let result = (|| {
            if reply.status != 200 {
                bail!("QR creation HTTP failure ({})", reply.status);
            }
            let envelope = parse_envelope(&reply.body)?;
            if envelope.error_code != 0 {
                bail!("QR creation rejected (code {})", envelope.error_code);
            }
            let encrypted = envelope
                .data
                .as_str()
                .context("encrypted QR data is missing")?;
            let body = cipher.decrypt(encrypted)?;
            self.flow.receive_created(generation, &body)
        })();
        if result.is_err() {
            self.flow.cancel();
        }
        result
    }

    pub async fn poll(
        &mut self,
        stage: PollStage,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<()> {
        let generation = self.flow.generation;
        let request = self.flow.next_poll(stage)?;
        match self.fetch(&request, cancel).await {
            Ok(reply) => self
                .flow
                .receive_poll(generation, stage, reply.status, &reply.body),
            Err(error) => {
                if *cancel.borrow() || cancel.has_changed().is_err() {
                    self.flow.cancel();
                } else {
                    self.flow.transport_failed(generation, stage)?;
                }
                Err(error)
            }
        }
    }

    async fn fetch(
        &mut self,
        request: &RequestSpec,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<HttpReply> {
        validate_login_url(request.url.as_str())?;
        if request.follow_redirects
            || !request.include_cookies
            || request.max_response_bytes > MAX_RESPONSE_BYTES
            || request.timeout_ms == 0
            || request.timeout_ms > MAX_POLL_SECONDS * 1000
        {
            bail!("invalid bounded login request policy");
        }
        if *cancel.borrow() {
            bail!("QR request cancelled");
        }
        let reply = tokio::select! {
            biased;
            _ = cancelled(cancel) => bail!("QR request cancelled"),
            result = tokio::time::timeout(Duration::from_millis(request.timeout_ms), self.transport.get(request)) => result.context("QR request timed out")??,
        };
        if reply.body.len() > request.max_response_bytes {
            bail!("QR HTTP response exceeds size limit");
        }
        if (300..400).contains(&reply.status) {
            bail!("QR HTTP redirects are forbidden");
        }
        Ok(reply)
    }
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() || cancel.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod driver_tests {
    use super::*;
    use serde_json::json;

    struct MockHttp {
        replies: std::collections::VecDeque<HttpReply>,
        requests: usize,
    }
    impl QrTransport for MockHttp {
        fn get(&mut self, request: &RequestSpec) -> impl Future<Output = Result<HttpReply>> + Send {
            assert!(!request.follow_redirects);
            self.requests += 1;
            std::future::ready(
                self.replies
                    .pop_front()
                    .context("fixture response exhausted"),
            )
        }
    }
    fn request_input() -> QrRequestInput<'static> {
        QrRequestInput {
            imei: "fixture-imei",
            computer_name: "Fixture Linux",
            language: "vi",
            first_launch_ms: 1700000000123,
            nonce_hex: "abcdef123456",
        }
    }

    #[tokio::test]
    async fn runs_encrypted_http_fixture_without_installing_a_session() {
        let (_, key) = create_request(&request_input()).unwrap();
        let plaintext = serde_json::to_vec(&json!({"error_code":0,"data":{
            "base64_qr":STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture"),
            "chk_wait_scan":"https://wpa.zaloapp.com/scan?fixture",
            "chk_wait_cfirm":"https://wpa.zaloapp.com/confirm?fixture",
            "qr_retry":3,"qr_timeout":15
        }}))
        .unwrap();
        let ciphertext = cbc::Encryptor::<aes::Aes256>::new((&key.0).into(), (&ZERO_IV).into())
            .encrypt_padded_vec_mut::<Pkcs7>(&plaintext);
        let replies = [
            HttpReply {
                status: 200,
                body: serde_json::to_vec(
                    &json!({"error_code":0,"data":STANDARD.encode(ciphertext)}),
                )
                .unwrap(),
            },
            HttpReply {
                status: 200,
                body: br#"{"error_code":0,"data":{"display_name":"Fixture"}}"#.to_vec(),
            },
            HttpReply {
                status: 200,
                body: br#"{"error_code":0,"data":{"uid":"123","dkey":"fixture"}}"#.to_vec(),
            },
        ]
        .into();
        let mut driver = QrDriver::new(MockHttp {
            replies,
            requests: 0,
        });
        let (_sender, mut cancel) = watch::channel(false);
        driver.start(&request_input(), &mut cancel).await.unwrap();
        assert!(driver.image_base64().is_some());
        driver.poll(PollStage::Scan, &mut cancel).await.unwrap();
        assert_eq!(driver.state(), QrState::Scanned);
        driver.poll(PollStage::Confirm, &mut cancel).await.unwrap();
        assert_eq!(driver.state(), QrState::TrustRequired);
        assert_eq!(driver.transport.requests, 3);
    }

    #[tokio::test]
    async fn rejects_redirects_oversized_bodies_and_pre_cancelled_requests() {
        for reply in [
            HttpReply {
                status: 302,
                body: Vec::new(),
            },
            HttpReply {
                status: 200,
                body: vec![b' '; MAX_RESPONSE_BYTES + 1],
            },
        ] {
            let mut driver = QrDriver::new(MockHttp {
                replies: [reply].into(),
                requests: 0,
            });
            let (_sender, mut cancel) = watch::channel(false);
            assert!(driver.start(&request_input(), &mut cancel).await.is_err());
            assert_eq!(driver.state(), QrState::Cancelled);
        }
        let mut driver = QrDriver::new(MockHttp {
            replies: Default::default(),
            requests: 0,
        });
        let (_sender, mut cancel) = watch::channel(true);
        assert!(driver.start(&request_input(), &mut cancel).await.is_err());
        assert_eq!(driver.transport.requests, 0);
    }

    struct HangingHttp;
    impl QrTransport for HangingHttp {
        fn get(
            &mut self,
            _request: &RequestSpec,
        ) -> impl Future<Output = Result<HttpReply>> + Send {
            std::future::pending()
        }
    }

    #[tokio::test]
    async fn timeout_and_cancellation_drop_pending_request() {
        let (mut request, _) = create_request(&request_input()).unwrap();
        request.timeout_ms = 5;
        let mut driver = QrDriver::new(HangingHttp);
        let (_sender, mut cancel) = watch::channel(false);
        let error = driver.fetch(&request, &mut cancel).await.err().unwrap();
        assert!(error.to_string().contains("timed out"));
        let (sender, mut cancel) = watch::channel(false);
        let task = tokio::spawn(async move {
            tokio::task::yield_now().await;
            sender.send(true).unwrap();
        });
        request.timeout_ms = 10_000;
        let error = driver.fetch(&request, &mut cancel).await.err().unwrap();
        assert!(error.to_string().contains("cancelled"));
        task.await.unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn created_body(scan_url: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"error_code":0,"data":{
            "base64_qr":STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture"),
            "chk_wait_scan":scan_url,"chk_wait_cfirm":"https://wpa.zaloapp.com/confirm?token=fixture",
            "qr_retry":3,"qr_timeout":15
        }})).unwrap()
    }

    #[test]
    fn node_crypto_known_answer_matches_parameter_cipher_and_signer() {
        // Independently generated with Node's node:crypto AES-CBC implementation;
        // constants and recipe are recorded in PROTOCOL_QR.md.
        let (mut params, cipher) = encrypt_parameters(
            "fixture-imei",
            1700000000123,
            "abcdef123456",
            r#"{"fixture":"Vietnamese: Tiếng Việt","value":42}"#.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            params["zcid"],
            "3961F9BC6141899DD997B3154F50C9420951B0F726A8F180BDB0983496EEEECF"
        );
        assert_eq!(&cipher.0, b"9E2FEF1536FB6489D9B1FEE6480D0186");
        assert_eq!(params["params"],"SH8piQrqQjBaga1zbXvW5B5S6rsX6yZjZd8AqvC2VUOkfXrmLOt3MMFTVRQSyTAFTFgo10ID2kV4oIggF1xwAw==");
        params.insert("type".into(), "30".into());
        params.insert("client_version".into(), "681".into());
        assert_eq!(
            sign_parameters("reqqr", &params),
            "a5124a84eb34d1757e00d7a6049c2aa1"
        );
        assert_eq!(
            cipher.decrypt(&params["params"]).unwrap(),
            r#"{"fixture":"Vietnamese: Tiếng Việt","value":42}"#.as_bytes()
        );
    }

    #[test]
    fn scan_locale_is_validated_and_does_not_duplicate_or_destroy_tokens() {
        let mut flow = QrFlow::default();
        flow.set_language("en").unwrap();
        assert!(flow.set_language("en&redirect=evil").is_err());
        let generation = flow.begin();
        flow.receive_created(
            generation,
            &created_body("https://wpa.zaloapp.com/scan?token=a%2Bb&lang=vi"),
        )
        .unwrap();
        let request = flow.next_poll(PollStage::Scan).unwrap();
        let pairs: Vec<_> = request.url.query_pairs().collect();
        assert_eq!(pairs.iter().filter(|(name, _)| name == "lang").count(), 1);
        assert!(pairs
            .iter()
            .any(|(name, value)| name == "lang" && value == "en"));
        assert!(pairs
            .iter()
            .any(|(name, value)| name == "token" && value == "a+b"));
    }

    #[test]
    fn request_is_bounded_cookie_aware_and_encrypted() {
        let (request, _) = create_request(&QrRequestInput {
            imei: "fixture-imei",
            computer_name: "Fixture Linux",
            language: "vi",
            first_launch_ms: 1700000000123,
            nonce_hex: "abcdef123456",
        })
        .unwrap();
        assert_eq!(request.method(), "GET");
        assert_eq!(request.url.path(), "/api/login/reqqr");
        assert!(request.include_cookies && !request.follow_redirects);
        assert_eq!(request.timeout_ms, 10_000);
        assert!(!request.url.as_str().contains("Fixture Linux"));
    }

    #[test]
    fn fake_http_fixture_scan_is_not_authenticated_and_confirmation_requires_trust() {
        let mut flow = QrFlow::default();
        let generation = flow.begin();
        flow.receive_created(
            generation,
            &created_body("https://wpa.zaloapp.com/scan?token=fixture"),
        )
        .unwrap();
        let scan = flow.next_poll(PollStage::Scan).unwrap();
        assert_eq!(scan.timeout_ms, 15_000);
        flow.receive_poll(
            generation,
            PollStage::Scan,
            200,
            br#"{"error_code":0,"data":{"display_name":"Fixture"}}"#,
        )
        .unwrap();
        assert_eq!(flow.state, QrState::Scanned);
        flow.next_poll(PollStage::Confirm).unwrap();
        assert_eq!(flow.state, QrState::AwaitingConfirmation);
        flow.receive_poll(
            generation,
            PollStage::Confirm,
            200,
            br#"{"error_code":0,"data":{"uid":"123","dkey":"fixture"}}"#,
        )
        .unwrap();
        assert_eq!(flow.state, QrState::TrustRequired);
        assert!(flow.image_base64().is_none());
        assert!(flow.next_poll(PollStage::Confirm).is_err());
    }

    #[test]
    fn untrusted_poll_urls_and_redirect_destinations_are_rejected() {
        for url in [
            "http://wpa.zaloapp.com/poll",
            "https://evil.example/poll",
            "https://wpa.zaloapp.com.evil.example/poll",
            "https://user@wpa.zaloapp.com/poll",
            "https://127.0.0.1/poll",
            "https://192.168.0.1/poll",
            "https://[::1]/poll",
            "https://wpa.zaloapp.com:444/poll",
            "https://wpa.zaloapp.com/poll#fragment",
        ] {
            let mut flow = QrFlow::default();
            let generation = flow.begin();
            assert!(
                flow.receive_created(generation, &created_body(url))
                    .is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn stale_cancellation_and_network_errors_do_not_authenticate() {
        let mut flow = QrFlow::default();
        let generation = flow.begin();
        flow.receive_created(
            generation,
            &created_body("https://wpa.chat.zalo.me/scan?token=fixture"),
        )
        .unwrap();
        flow.next_poll(PollStage::Scan).unwrap();
        assert!(flow
            .receive_poll(generation, PollStage::Scan, 503, b"unavailable")
            .is_err());
        assert_eq!(flow.state, QrState::Requested);
        flow.cancel();
        assert!(flow
            .receive_poll(
                generation,
                PollStage::Scan,
                200,
                br#"{"error_code":0,"data":{}}"#
            )
            .is_err());
        assert_eq!(flow.state, QrState::Cancelled);
    }

    #[test]
    fn expired_and_unsupported_second_layer_fail_closed() {
        for (code, state) in [
            (-20, QrState::TrustRequired),
            (-21, QrState::Expired),
            (408, QrState::Expired),
        ] {
            let mut flow = QrFlow::default();
            let generation = flow.begin();
            flow.receive_created(generation, &created_body("https://wpa.zaloapp.com/poll"))
                .unwrap();
            flow.next_poll(PollStage::Scan).unwrap();
            flow.receive_poll(
                generation,
                PollStage::Scan,
                200,
                &serde_json::to_vec(&json!({"error_code":code})).unwrap(),
            )
            .unwrap();
            assert_eq!(flow.state, state);
        }
    }

    #[test]
    fn retries_are_bounded_and_reset_after_scan_and_only_one_request_can_run() {
        let mut flow = QrFlow::default();
        let generation = flow.begin();
        flow.receive_created(generation, &created_body("https://wpa.zaloapp.com/poll"))
            .unwrap();
        for _ in 0..2 {
            flow.next_poll(PollStage::Scan).unwrap();
            assert!(flow.next_poll(PollStage::Scan).is_err());
            flow.transport_failed(generation, PollStage::Scan).unwrap();
        }
        flow.next_poll(PollStage::Scan).unwrap();
        flow.receive_poll(
            generation,
            PollStage::Scan,
            200,
            br#"{"error_code":0,"data":{}}"#,
        )
        .unwrap();
        for _ in 0..3 {
            flow.next_poll(PollStage::Confirm).unwrap();
            flow.transport_failed(generation, PollStage::Confirm)
                .unwrap();
        }
        assert!(flow.next_poll(PollStage::Confirm).is_err());
        assert_eq!(flow.state(), QrState::Expired);
    }
}
