//! Opt-in, original HTTP transport for the verified authentication request specs.
//! No UI/CLI path activates it. Cookies are transient, private to one attempt and bounded.
use super::qr::{HttpReply, QrTransport, RequestSpec, MAX_RESPONSE_BYTES};
use reqwest::{
    cookie::{CookieStore, Jar},
    header::HeaderValue,
    Client,
};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use url::Url;

const MAX_URL_BYTES: usize = 8192;
const MAX_TIMEOUT_MS: u64 = 60_000;
const MAX_RESPONSE_HEADER_BYTES: usize = 32 * 1024;
const MAX_RESPONSE_HEADERS: usize = 128;
const MAX_COOKIE_BYTES: usize = 64 * 1024;
const MAX_COOKIE_HEADERS: usize = 128;
const MAX_COOKIE_HEADER_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpError {
    InvalidRequest,
    ClientInitialization,
    Transport,
    Timeout,
    Redirect,
    Status(u16),
    ResponseTooLarge,
    HeaderBudget,
    Allocation,
    CookieBudget,
}
impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest => {
                f.write_str("Authentication request violates the URL or resource policy")
            }
            Self::ClientInitialization => {
                f.write_str("Authentication HTTP client could not initialize")
            }
            Self::Transport => {
                f.write_str("Authentication transport failed; credentials were preserved")
            }
            Self::Timeout => {
                f.write_str("Authentication request timed out; credentials were preserved")
            }
            Self::Redirect => f.write_str("Authentication redirects are forbidden"),
            Self::Status(status) => write!(
                f,
                "Authentication HTTP status {status}; credentials were preserved"
            ),
            Self::ResponseTooLarge => f.write_str("Authentication response exceeds the byte limit"),
            Self::HeaderBudget => {
                f.write_str("Authentication response headers exceed the resource limit")
            }
            Self::Allocation => f.write_str("Authentication response buffer allocation failed"),
            Self::CookieBudget => {
                f.write_str("Authentication cookie budget exhausted; start a new attempt")
            }
        }
    }
}
// Never retain reqwest::Error as a source: its URL may contain credential query parameters.
impl std::error::Error for HttpError {}
fn transport_error(error: reqwest::Error) -> HttpError {
    if error.is_timeout() {
        HttpError::Timeout
    } else {
        HttpError::Transport
    }
}

fn allowed_url(url: &Url) -> bool {
    url.as_str().len() <= MAX_URL_BYTES
        && url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && matches!(url.host_str(), Some("wpa.zaloapp.com" | "wpa.chat.zalo.me"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}
fn validate(request: &RequestSpec) -> Result<(), HttpError> {
    if !allowed_url(&request.url)
        || request.follow_redirects
        || !request.include_cookies
        || request.timeout_ms == 0
        || request.timeout_ms > MAX_TIMEOUT_MS
        || request.max_response_bytes == 0
        || request.max_response_bytes > MAX_RESPONSE_BYTES
    {
        return Err(HttpError::InvalidRequest);
    }
    Ok(())
}

#[derive(Default)]
struct CookieUsage {
    bytes: usize,
    headers: usize,
    exhausted: bool,
}
#[derive(Default)]
struct AttemptCookies {
    jar: Jar,
    usage: Mutex<CookieUsage>,
}
impl AttemptCookies {
    fn exhausted(&self) -> bool {
        self.usage.lock().map(|u| u.exhausted).unwrap_or(true)
    }
}
impl CookieStore for AttemptCookies {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        if !allowed_url(url) {
            return;
        }
        let Ok(mut usage) = self.usage.lock() else {
            return;
        };
        if usage.exhausted {
            return;
        }
        // Monotonic attempt-wide budgets also count replacements, preventing endless
        // Set-Cookie churn from retaining unbounded parser/store allocations.
        let mut accepted = Vec::new();
        for header in headers {
            let len = header.as_bytes().len();
            if len > MAX_COOKIE_HEADER_BYTES
                || usage.headers >= MAX_COOKIE_HEADERS
                || len > MAX_COOKIE_BYTES - usage.bytes
            {
                usage.exhausted = true;
                return;
            }
            usage.bytes += len;
            usage.headers += 1;
            accepted.push(header);
        }
        self.jar.set_cookies(&mut accepted.into_iter(), url);
    }
    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        if !allowed_url(url) || self.exhausted() {
            return None;
        }
        self.jar.cookies(url)
    }
}

/// One instance is one isolated login attempt. Not Clone; no public cookie mutation,
/// persistence, client access or conversion from a pending/confirmed session exists.
pub struct HttpAdapter {
    backend: ReqwestBackend,
}
impl HttpAdapter {
    pub fn new() -> Result<Self, HttpError> {
        let cookies = Arc::new(AttemptCookies::default());
        let client = client_builder(cookies.clone())
            .build()
            .map_err(|_| HttpError::ClientInitialization)?;
        Ok(Self {
            backend: ReqwestBackend { client, cookies },
        })
    }
    /// Supports QR and bootstrap RequestSpecs. An exclusive borrow serializes requests;
    /// dropping the future cancels polling without spawning detached work.
    pub async fn fetch(&mut self, request: &RequestSpec) -> Result<HttpReply, HttpError> {
        bounded_fetch(&mut self.backend, request).await
    }
}
/// Shared strict policy. Test-only code can add loopback DNS and its own CA;
/// no production constructor accepts resolver, certificate or insecure overrides.
fn client_builder(cookies: Arc<AttemptCookies>) -> reqwest::ClientBuilder {
    Client::builder()
        .cookie_provider(cookies)
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .https_only(true)
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_millis(MAX_TIMEOUT_MS))
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(Duration::from_secs(30))
        .user_agent(concat!(
            "zalo-linux-native-preview/",
            env!("CARGO_PKG_VERSION")
        ))
}

impl QrTransport for HttpAdapter {
    async fn get(&mut self, request: &RequestSpec) -> anyhow::Result<HttpReply> {
        self.fetch(request).await.map_err(anyhow::Error::new)
    }
}

trait Body: Send {
    type Chunk: AsRef<[u8]> + Send;
    fn next(&mut self) -> impl Future<Output = Result<Option<Self::Chunk>, HttpError>> + Send;
}
struct Head<B> {
    status: u16,
    content_length: Option<u64>,
    header_bytes: usize,
    header_count: usize,
    body: B,
}
trait Backend: Send {
    type Body: Body;
    fn open(
        &mut self,
        request: &RequestSpec,
    ) -> impl Future<Output = Result<Head<Self::Body>, HttpError>> + Send;
}
struct ReqwestBackend {
    client: Client,
    cookies: Arc<AttemptCookies>,
}
struct ReqwestBody(reqwest::Response);
impl Body for ReqwestBody {
    type Chunk = bytes::Bytes;
    async fn next(&mut self) -> Result<Option<Self::Chunk>, HttpError> {
        self.0.chunk().await.map_err(transport_error)
    }
}
impl Backend for ReqwestBackend {
    type Body = ReqwestBody;
    async fn open(&mut self, request: &RequestSpec) -> Result<Head<Self::Body>, HttpError> {
        if self.cookies.exhausted() {
            return Err(HttpError::CookieBudget);
        }
        let response = self
            .client
            .get(request.url.clone())
            .timeout(Duration::from_millis(request.timeout_ms))
            .send()
            .await
            .map_err(transport_error)?;
        if self.cookies.exhausted() {
            return Err(HttpError::CookieBudget);
        }
        let mut header_bytes = 0usize;
        let mut header_count = 0usize;
        for (name, value) in response.headers() {
            header_count += 1;
            header_bytes = header_bytes
                .checked_add(name.as_str().len())
                .and_then(|n| n.checked_add(value.as_bytes().len()))
                .ok_or(HttpError::HeaderBudget)?;
            if header_count > MAX_RESPONSE_HEADERS || header_bytes > MAX_RESPONSE_HEADER_BYTES {
                return Err(HttpError::HeaderBudget);
            }
        }
        Ok(Head {
            status: response.status().as_u16(),
            content_length: response.content_length(),
            header_bytes,
            header_count,
            body: ReqwestBody(response),
        })
    }
}
async fn bounded_fetch<B: Backend>(
    backend: &mut B,
    request: &RequestSpec,
) -> Result<HttpReply, HttpError> {
    validate(request)?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(request.timeout_ms);
    tokio::time::timeout_at(deadline, async {
        let mut head = backend.open(request).await?;
        if head.header_count > MAX_RESPONSE_HEADERS || head.header_bytes > MAX_RESPONSE_HEADER_BYTES
        {
            return Err(HttpError::HeaderBudget);
        }
        if (300..400).contains(&head.status) {
            return Err(HttpError::Redirect);
        }
        if head.status != 200 {
            return Err(HttpError::Status(head.status));
        }
        let limit = request.max_response_bytes;
        if head.content_length.is_some_and(|len| len > limit as u64) {
            return Err(HttpError::ResponseTooLarge);
        }
        let mut body = Vec::new();
        let mut chunks = 0usize;
        while let Some(chunk) = head.body.next().await? {
            chunks += 1;
            if chunks.is_multiple_of(64) {
                // Ready/empty chunks must not monopolize the executor and hide the
                // outer deadline or a caller dropping this request future.
                tokio::task::yield_now().await;
                if tokio::time::Instant::now() >= deadline {
                    return Err(HttpError::Timeout);
                }
            }
            let chunk = chunk.as_ref();
            if chunk.len() > limit - body.len() {
                return Err(HttpError::ResponseTooLarge);
            }
            let needed = body.len() + chunk.len();
            if needed > body.capacity() {
                // Geometric growth bounds tiny-chunk copying while never requesting
                // application buffer capacity beyond the response payload budget.
                let target = body
                    .capacity()
                    .saturating_mul(2)
                    .max(4096)
                    .max(needed)
                    .min(limit);
                body.try_reserve_exact(target - body.len())
                    .map_err(|_| HttpError::Allocation)?;
            }
            body.extend_from_slice(chunk);
        }
        Ok(HttpReply {
            status: head.status,
            body,
        })
    })
    .await
    .map_err(|_| HttpError::Timeout)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct FakeBody {
        chunks: VecDeque<Vec<u8>>,
        pending: bool,
        repeat_empty: bool,
        polls: Arc<AtomicUsize>,
        dropped: Arc<AtomicBool>,
    }
    impl Drop for FakeBody {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
    impl Body for FakeBody {
        type Chunk = Vec<u8>;
        async fn next(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            if self.pending {
                std::future::pending::<()>().await;
            }
            if self.repeat_empty {
                return Ok(Some(Vec::new()));
            }
            Ok(self.chunks.pop_front())
        }
    }
    struct FakeBackend {
        head: Option<Head<FakeBody>>,
        opens: usize,
    }
    impl Backend for FakeBackend {
        type Body = FakeBody;
        async fn open(&mut self, _: &RequestSpec) -> Result<Head<FakeBody>, HttpError> {
            self.opens += 1;
            self.head.take().ok_or(HttpError::Transport)
        }
    }
    fn fixture(chunks: Vec<Vec<u8>>) -> (FakeBackend, Arc<AtomicUsize>, Arc<AtomicBool>) {
        let polls = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        (
            FakeBackend {
                opens: 0,
                head: Some(Head {
                    status: 200,
                    content_length: None,
                    header_bytes: 64,
                    header_count: 2,
                    body: FakeBody {
                        chunks: chunks.into(),
                        pending: false,
                        repeat_empty: false,
                        polls: polls.clone(),
                        dropped: dropped.clone(),
                    },
                }),
            },
            polls,
            dropped,
        )
    }
    fn request() -> RequestSpec {
        RequestSpec {
            url: Url::parse("https://wpa.zaloapp.com/api/login/reqqr?token=synthetic-secret")
                .unwrap(),
            timeout_ms: 100,
            max_response_bytes: 16,
            include_cookies: true,
            follow_redirects: false,
        }
    }

    #[tokio::test]
    async fn chunked_unknown_length_is_bounded_and_body_is_dropped_on_overflow() {
        let (mut backend, polls, dropped) = fixture(vec![
            b"abc".to_vec(),
            b"def".to_vec(),
            b"never-read".to_vec(),
        ]);
        let mut spec = request();
        spec.max_response_bytes = 5;
        assert!(matches!(
            bounded_fetch(&mut backend, &spec).await,
            Err(HttpError::ResponseTooLarge)
        ));
        assert_eq!(polls.load(Ordering::SeqCst), 2);
        assert!(dropped.load(Ordering::SeqCst));
        let (mut backend, _, _) = fixture(vec![b"ab".to_vec(), b"cd".to_vec()]);
        spec.max_response_bytes = 4;
        assert_eq!(
            bounded_fetch(&mut backend, &spec).await.unwrap().body,
            b"abcd"
        );
    }
    #[tokio::test]
    async fn declared_length_status_and_header_limits_reject_before_body_read() {
        for (status, length, headers, bytes, expected) in [
            (200, Some(17), 2, 64, HttpError::ResponseTooLarge),
            (302, None, 2, 64, HttpError::Redirect),
            (401, None, 2, 64, HttpError::Status(401)),
            (503, None, 2, 64, HttpError::Status(503)),
            (
                200,
                None,
                MAX_RESPONSE_HEADERS + 1,
                64,
                HttpError::HeaderBudget,
            ),
            (
                200,
                None,
                2,
                MAX_RESPONSE_HEADER_BYTES + 1,
                HttpError::HeaderBudget,
            ),
        ] {
            let (mut backend, polls, dropped) =
                fixture(vec![b"sensitive-body-never-read".to_vec()]);
            let head = backend.head.as_mut().unwrap();
            head.status = status;
            head.content_length = length;
            head.header_count = headers;
            head.header_bytes = bytes;
            assert!(
                matches!(bounded_fetch(&mut backend,&request()).await,Err(actual) if actual==expected)
            );
            assert_eq!(polls.load(Ordering::SeqCst), 0);
            assert!(dropped.load(Ordering::SeqCst));
        }
    }
    #[tokio::test]
    async fn many_tiny_chunks_preserve_exact_body_within_the_same_payload_limit() {
        let bytes = vec![b'x'; 16_384];
        let (mut backend, polls, _) = fixture(bytes.iter().map(|b| vec![*b]).collect());
        let mut spec = request();
        spec.max_response_bytes = bytes.len();
        spec.timeout_ms = 1000;
        assert_eq!(
            bounded_fetch(&mut backend, &spec).await.unwrap().body,
            bytes
        );
        assert_eq!(polls.load(Ordering::SeqCst), 16_385);
    }
    #[tokio::test(start_paused = true)]
    async fn total_deadline_cancels_a_stalled_body() {
        let (mut backend, _, dropped) = fixture(vec![]);
        backend.head.as_mut().unwrap().body.pending = true;
        assert!(matches!(
            bounded_fetch(&mut backend, &request()).await,
            Err(HttpError::Timeout)
        ));
        assert!(dropped.load(Ordering::SeqCst));
    }
    #[tokio::test(start_paused = true)]
    async fn always_ready_empty_chunks_cannot_starve_deadline_or_other_tasks() {
        let (mut backend, polls, dropped) = fixture(vec![]);
        backend.head.as_mut().unwrap().body.repeat_empty = true;
        let spec = request();
        let clock = async {
            tokio::task::yield_now().await;
            tokio::time::advance(Duration::from_millis(spec.timeout_ms)).await;
        };
        let (result, ()) = tokio::join!(bounded_fetch(&mut backend, &spec), clock);
        assert!(matches!(result, Err(HttpError::Timeout)));
        assert!(polls.load(Ordering::SeqCst) > 0);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn dropping_the_request_future_drops_pending_body_without_detached_work() {
        let (mut backend, polls, dropped) = fixture(vec![]);
        backend.head.as_mut().unwrap().body.pending = true;
        let spec = request();
        let mut future = Box::pin(bounded_fetch(&mut backend, &spec));
        tokio::select! {biased;_=&mut future=>panic!("pending fixture completed"),_=tokio::task::yield_now()=>{}}
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        drop(future);
        assert!(dropped.load(Ordering::SeqCst));
    }
    #[tokio::test]
    async fn invalid_urls_and_request_policies_never_reach_the_backend() {
        for url in [
            "http://wpa.zaloapp.com",
            "https://wpa.zaloapp.com.evil.example",
            "https://example.com",
            "https://wpa.zaloapp.com:444",
            "https://user:password@wpa.zaloapp.com",
            "https://wpa.zaloapp.com/#secret",
        ] {
            let (mut backend, _, _) = fixture(vec![]);
            let mut spec = request();
            spec.url = Url::parse(url).unwrap();
            assert!(matches!(
                bounded_fetch(&mut backend, &spec).await,
                Err(HttpError::InvalidRequest)
            ));
            assert_eq!(backend.opens, 0);
        }
        for case in 0..5 {
            let (mut backend, _, _) = fixture(vec![]);
            let mut spec = request();
            match case {
                0 => spec.follow_redirects = true,
                1 => spec.include_cookies = false,
                2 => spec.timeout_ms = 0,
                3 => spec.max_response_bytes = MAX_RESPONSE_BYTES + 1,
                _ => spec.timeout_ms = MAX_TIMEOUT_MS + 1,
            }
            assert!(matches!(
                bounded_fetch(&mut backend, &spec).await,
                Err(HttpError::InvalidRequest)
            ));
            assert_eq!(backend.opens, 0);
        }
    }
    #[tokio::test]
    async fn concrete_adapters_have_separate_transient_cookie_jars_and_refuse_unknown_hosts() {
        let one = HttpAdapter::new().unwrap();
        let mut two = HttpAdapter::new().unwrap();
        let url = request().url;
        let cookie = HeaderValue::from_static("attempt=synthetic; Secure; HttpOnly; Path=/");
        one.backend
            .cookies
            .set_cookies(&mut std::iter::once(&cookie), &url);
        assert!(one.backend.cookies.cookies(&url).is_some());
        assert!(two.backend.cookies.cookies(&url).is_none());
        assert!(one
            .backend
            .cookies
            .cookies(&Url::parse("https://example.com").unwrap())
            .is_none());
        // This concrete request is rejected before DNS/connect; no test contacts Zalo.
        let mut spec = request();
        spec.url = Url::parse("https://example.com").unwrap();
        assert!(matches!(
            two.fetch(&spec).await,
            Err(HttpError::InvalidRequest)
        ));
    }
    #[test]
    fn cookie_lifetime_count_and_byte_budgets_fail_closed() {
        let jar = AttemptCookies::default();
        let url = request().url;
        for _ in 0..MAX_COOKIE_HEADERS {
            let cookie = HeaderValue::from_static("a=b; Secure");
            jar.set_cookies(&mut std::iter::once(&cookie), &url);
        }
        assert!(!jar.exhausted());
        let cookie = HeaderValue::from_static("a=c; Secure");
        jar.set_cookies(&mut std::iter::once(&cookie), &url);
        assert!(jar.exhausted());
        assert!(jar.cookies(&url).is_none());
        let cumulative = AttemptCookies::default();
        let full = HeaderValue::from_str(&format!("a={}", "x".repeat(MAX_COOKIE_HEADER_BYTES - 2)))
            .unwrap();
        for _ in 0..(MAX_COOKIE_BYTES / MAX_COOKIE_HEADER_BYTES) {
            cumulative.set_cookies(&mut std::iter::once(&full), &url);
        }
        assert!(!cumulative.exhausted());
        cumulative.set_cookies(&mut std::iter::once(&cookie), &url);
        assert!(cumulative.exhausted());
        let jar = AttemptCookies::default();
        let large =
            HeaderValue::from_str(&format!("a={}", "x".repeat(MAX_COOKIE_HEADER_BYTES))).unwrap();
        jar.set_cookies(&mut std::iter::once(&large), &url);
        assert!(jar.exhausted());
    }
    #[test]
    fn transport_errors_discard_secret_urls_and_entire_error_source_chains() {
        use std::error::Error;
        let client = Client::new();
        let error = client
            .get("https://wpa.zaloapp.com/?zpw_sek=synthetic-secret")
            .header("X-Fixture", "bad\r\nheader")
            .build()
            .unwrap_err();
        let error = transport_error(error);
        let wrapped = anyhow::Error::new(error);
        assert!(!format!("{wrapped:#}").contains("synthetic-secret"));
        assert!(!format!("{wrapped:?}").contains("zpw_sek"));
        assert!(error.source().is_none());
    }
}

#[cfg(test)]
mod tls_tests {
    use super::*;
    use rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConfig, ServerConnection, StreamOwned,
    };
    use std::{
        fs,
        io::{Read, Write},
        net::{SocketAddr, TcpListener, TcpStream},
        path::PathBuf,
        process::Command,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            OnceLock,
        },
        thread::{self, JoinHandle},
    };

    struct Material {
        root: Vec<u8>,
        leaf: Vec<u8>,
        key: Vec<u8>,
    }
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn material() -> &'static Material {
        static MATERIAL: OnceLock<Material> = OnceLock::new();
        MATERIAL.get_or_init(|| {
            let scratch = Scratch(std::env::temp_dir().join(format!(
                "zalo-loopback-tls-{}",
                crate::domain::local_message_id().unwrap()
            )));
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&scratch.0)
                    .unwrap();
            }
            #[cfg(not(unix))]
            fs::create_dir(&scratch.0).unwrap();
            let run = |args: &[&str]| {
                let output = Command::new("openssl")
                    .args(args)
                    .current_dir(&scratch.0)
                    .output()
                    .expect("OpenSSL is required for local TLS integration tests");
                assert!(
                    output.status.success(),
                    "local synthetic certificate command failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            };
            run(&[
                "req",
                "-x509",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
                "-nodes",
                "-keyout",
                "root.key",
                "-out",
                "root.pem",
                "-days",
                "1",
                "-subj",
                "/CN=Native localhost fixture CA",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
            ]);
            run(&[
                "req",
                "-new",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
                "-nodes",
                "-keyout",
                "leaf.key",
                "-out",
                "leaf.csr",
                "-subj",
                "/CN=wpa.zaloapp.com",
            ]);
            fs::write(
                scratch.0.join("leaf.ext"),
                concat!(
                    "subjectAltName=DNS:wpa.zaloapp.com\n",
                    "basicConstraints=critical,CA:FALSE\n",
                    "keyUsage=critical,digitalSignature\n",
                    "extendedKeyUsage=serverAuth\n"
                ),
            )
            .unwrap();
            run(&[
                "x509",
                "-req",
                "-in",
                "leaf.csr",
                "-CA",
                "root.pem",
                "-CAkey",
                "root.key",
                "-set_serial",
                "1",
                "-out",
                "leaf.pem",
                "-days",
                "1",
                "-extfile",
                "leaf.ext",
            ]);
            run(&[
                "x509", "-in", "root.pem", "-outform", "DER", "-out", "root.der",
            ]);
            run(&[
                "x509", "-in", "leaf.pem", "-outform", "DER", "-out", "leaf.der",
            ]);
            run(&[
                "pkcs8", "-topk8", "-nocrypt", "-in", "leaf.key", "-outform", "DER", "-out",
                "leaf.pk8",
            ]);
            Material {
                root: fs::read(scratch.0.join("root.der")).unwrap(),
                leaf: fs::read(scratch.0.join("leaf.der")).unwrap(),
                key: fs::read(scratch.0.join("leaf.pk8")).unwrap(),
            }
        })
    }

    #[derive(Clone)]
    struct Captured {
        path: String,
        host: String,
        cookie: Option<String>,
    }
    #[derive(Default)]
    struct State {
        requests: Mutex<Vec<Captured>>,
        sni: Mutex<Vec<String>>,
        closed_stalls: AtomicUsize,
    }
    struct Fixture {
        addr: SocketAddr,
        state: Arc<State>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }
    impl Fixture {
        fn start() -> Self {
            let material = material();
            let config = ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(material.leaf.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(material.key.clone())),
            )
            .unwrap();
            let listener = TcpListener::bind("127.0.0.1:0")
                .expect("local TLS tests require loopback socket permission");
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let state = Arc::new(State::default());
            let worker_stop = stop.clone();
            let worker_state = state.clone();
            let config = Arc::new(config);
            let thread = thread::spawn(move || {
                let mut children = Vec::new();
                while !worker_stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((socket, _)) => {
                            assert!(
                                children.len() < 32,
                                "local fixture connection budget exceeded"
                            );
                            let config = config.clone();
                            let state = worker_state.clone();
                            children.push(thread::spawn(move || serve(socket, config, state)));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::park_timeout(Duration::from_millis(2))
                        }
                        Err(e) => panic!("loopback accept failed: {e}"),
                    }
                }
                for child in children {
                    child.join().unwrap();
                }
            });
            Self {
                addr,
                state,
                stop,
                thread: Some(thread),
            }
        }
        fn adapter(&self, trusted: bool) -> HttpAdapter {
            let cookies = Arc::new(AttemptCookies::default());
            // Both production-allowed hosts are pinned to this listener. There is
            // no DNS or proxy fallback that can contact an external Zalo server.
            let mut builder = client_builder(cookies.clone())
                .resolve("wpa.zaloapp.com", self.addr)
                .resolve("wpa.chat.zalo.me", self.addr);
            if trusted {
                builder = builder.tls_built_in_root_certs(false).add_root_certificate(
                    reqwest::Certificate::from_der(&material().root).unwrap(),
                );
            }
            let client = builder.build().unwrap();
            HttpAdapter {
                backend: ReqwestBackend { client, cookies },
            }
        }
        fn request(&self, path: &str, limit: usize) -> RequestSpec {
            RequestSpec {
                url: Url::parse(&format!("https://wpa.zaloapp.com{path}")).unwrap(),
                timeout_ms: 3000,
                max_response_bytes: limit,
                include_cookies: true,
                follow_redirects: false,
            }
        }
        fn seen(&self, path: &str) -> bool {
            self.state
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.path == path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    fn serve(mut socket: TcpStream, config: Arc<ServerConfig>, state: Arc<State>) {
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut connection = ServerConnection::new(config).unwrap();
        while connection.is_handshaking() {
            let result = connection.complete_io(&mut socket);
            if result.is_err() {
                if let Some(name) = connection.server_name() {
                    state.sni.lock().unwrap().push(name.into());
                }
                return;
            }
        }
        if let Some(name) = connection.server_name() {
            state.sni.lock().unwrap().push(name.into());
        }
        let mut tls = StreamOwned::new(connection, socket);
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0u8];
            if tls.read(&mut byte).ok() != Some(1) {
                return;
            }
            request.push(byte[0]);
            if request.len() > 16 * 1024 {
                return;
            }
        }
        let text = String::from_utf8(request).unwrap();
        let mut lines = text.split("\r\n");
        let path = lines
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        let mut host = String::new();
        let mut cookie = None;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("host") {
                    host = value.trim().into();
                }
                if name.eq_ignore_ascii_case("cookie") {
                    cookie = Some(value.trim().to_owned());
                }
            }
        }
        state.requests.lock().unwrap().push(Captured {
            path: path.clone(),
            host,
            cookie: cookie.clone(),
        });
        let route = path.split('?').next().unwrap();
        if route == "/stall-body" || route == "/stall-headers" {
            if route == "/stall-body" {
                let _ = tls.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n",
                );
                let _ = tls.flush();
            }
            let mut byte = [0u8];
            match tls.read(&mut byte) {
                Ok(0) => {
                    state.closed_stalls.fetch_add(1, Ordering::SeqCst);
                }
                Err(e)
                    if !matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    state.closed_stalls.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
            return;
        }
        let response=match route {
            "/chunked"=>b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n".to_vec(),
            "/ok"=>b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nab\r\n2\r\ncd\r\n0\r\n\r\n".to_vec(),
            "/declared"=>format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",MAX_RESPONSE_BYTES+1).into_bytes(),
            "/headers"=>format!("HTTP/1.1 200 OK\r\nX-Overflow: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n","x".repeat(MAX_RESPONSE_HEADER_BYTES)).into_bytes(),
            "/redirect"=>b"HTTP/1.1 302 Found\r\nLocation: https://wpa.zaloapp.com/followed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            "/status"=>b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            "/set-cookie"=>b"HTTP/1.1 200 OK\r\nSet-Cookie: attempt=wire-synthetic; Secure; HttpOnly; Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
            "/echo-cookie"=>{let body=cookie.unwrap_or_default();format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).into_bytes()},
            _=>b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        };
        let _ = tls.write_all(&response);
        let _ = tls.flush();
        let _ = tls.sock.shutdown(std::net::Shutdown::Write);
    }
    async fn wait_until(condition: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("local TLS fixture condition timed out");
    }

    #[tokio::test]
    async fn real_tls_validates_sni_and_streams_unknown_length_body() {
        let fixture = Fixture::start();
        let mut adapter = fixture.adapter(true);
        let result = adapter.fetch(&fixture.request("/ok", 4)).await.unwrap();
        assert_eq!(result.body, b"abcd");
        assert_eq!(
            fixture.state.sni.lock().unwrap().as_slice(),
            ["wpa.zaloapp.com"]
        );
        assert_eq!(
            fixture.state.requests.lock().unwrap()[0].host,
            "wpa.zaloapp.com"
        );
    }
    #[tokio::test]
    async fn real_tls_refuses_chunked_declared_and_header_overflow() {
        let fixture = Fixture::start();
        let mut adapter = fixture.adapter(true);
        for (path, expected) in [
            ("/chunked", HttpError::ResponseTooLarge),
            ("/declared", HttpError::ResponseTooLarge),
            ("/headers", HttpError::HeaderBudget),
        ] {
            assert!(
                matches!(adapter.fetch(&fixture.request(path,5)).await,Err(error) if error==expected),
                "{path}"
            );
        }
    }
    #[tokio::test]
    async fn real_tls_refuses_status_and_does_not_follow_redirect() {
        let fixture = Fixture::start();
        let mut adapter = fixture.adapter(true);
        assert!(matches!(
            adapter.fetch(&fixture.request("/redirect", 64)).await,
            Err(HttpError::Redirect)
        ));
        assert!(matches!(
            adapter.fetch(&fixture.request("/status", 64)).await,
            Err(HttpError::Status(503))
        ));
        assert!(!fixture.seen("/followed"));
        assert_eq!(fixture.state.requests.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn real_tls_sends_cookies_only_inside_the_same_attempt() {
        let fixture = Fixture::start();
        let mut one = fixture.adapter(true);
        let mut two = fixture.adapter(true);
        one.fetch(&fixture.request("/set-cookie", 64))
            .await
            .unwrap();
        assert_eq!(
            one.fetch(&fixture.request("/echo-cookie", 64))
                .await
                .unwrap()
                .body,
            b"attempt=wire-synthetic"
        );
        assert!(two
            .fetch(&fixture.request("/echo-cookie", 64))
            .await
            .unwrap()
            .body
            .is_empty());
        let requests = fixture.state.requests.lock().unwrap();
        assert!(requests[0].cookie.is_none());
        assert_eq!(
            requests[1].cookie.as_deref(),
            Some("attempt=wire-synthetic")
        );
        assert!(requests[2].cookie.is_none());
    }
    #[tokio::test]
    async fn real_tls_deadline_closes_stalled_headers_and_body() {
        for path in ["/stall-headers", "/stall-body"] {
            let fixture = Fixture::start();
            let mut adapter = fixture.adapter(true);
            let mut spec = fixture.request(path, 64);
            spec.timeout_ms = 500;
            assert!(matches!(
                adapter.fetch(&spec).await,
                Err(HttpError::Timeout)
            ));
            wait_until(|| fixture.state.closed_stalls.load(Ordering::SeqCst) == 1).await;
        }
    }
    #[tokio::test]
    async fn real_tls_abort_drops_request_and_closes_the_socket() {
        let fixture = Fixture::start();
        let mut adapter = fixture.adapter(true);
        let spec = fixture.request("/stall-body", 64);
        let task = tokio::spawn(async move { adapter.fetch(&spec).await });
        wait_until(|| fixture.seen("/stall-body")).await;
        task.abort();
        assert!(matches!(task.await,Err(error) if error.is_cancelled()));
        wait_until(|| fixture.state.closed_stalls.load(Ordering::SeqCst) == 1).await;
    }
    #[tokio::test]
    async fn real_tls_rejects_untrusted_certificate_and_wrong_sni_without_secret_error_chains() {
        use std::error::Error;
        let fixture = Fixture::start();
        for trusted in [false, true] {
            let mut adapter = fixture.adapter(trusted);
            let mut spec = fixture.request("/ok?token=synthetic-wire-secret", 64);
            if trusted {
                spec.url.set_host(Some("wpa.chat.zalo.me")).unwrap();
            }
            let error = match adapter.fetch(&spec).await {
                Ok(_) => panic!("invalid TLS identity accepted"),
                Err(error) => error,
            };
            assert_eq!(error, HttpError::Transport);
            assert!(error.source().is_none());
            let error = anyhow::Error::new(error);
            assert!(!format!("{error:#}").contains("synthetic-wire-secret"));
            assert!(!format!("{error:?}").contains("wpa."));
        }
        assert!(fixture.state.requests.lock().unwrap().is_empty());
        wait_until(|| {
            fixture
                .state
                .sni
                .lock()
                .unwrap()
                .iter()
                .any(|name| name == "wpa.chat.zalo.me")
        })
        .await;
    }
}
