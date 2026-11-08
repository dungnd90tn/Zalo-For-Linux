//! Opt-in, read-only probe of the account's E2EE state (stage 1 of `E2EE_LIVE_PLAN.md`).
//!
//! Enabled only when `ZALO_E2EE_PROBE` is an absolute file path. Once per process, right after the
//! socket is authenticated and before offline queues are requested, it sends the two query commands
//! the original uses to reconcile keys: 564 (own uploaded key id) and 541 (own bundles). It changes
//! nothing on the account. The reply is written to that file as a *shape*: field names, JSON types,
//! lengths, and the value of a few small status fields. Keys, signatures, ids and any other value
//! never reach the file, so it can be pasted into a bug report.
use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

const WAIT: Duration = Duration::from_secs(12);
const MAX_LINES: usize = 400;
const MAX_DEPTH: usize = 8;
const MAX_ITEMS: usize = 4;
/// Small status fields whose value is the point of the probe and carries no identity or key material.
const VALUE_FIELDS: &[&str] = &[
    "error_code",
    "err",
    "deviceId",
    "platformType",
    "isReInit",
    "hasMore",
];

/// The output path, if the probe was requested; `None` for a relative or empty value.
pub(super) fn target() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_PROBE")?);
    path.is_absolute().then_some(path)
}
/// True for the first caller only, so a reconnect never repeats the probe.
pub(super) fn claim() -> bool {
    static DONE: AtomicBool = AtomicBool::new(false);
    !DONE.swap(true, Ordering::SeqCst)
}

/// For builds without the engine: say so in the requested report instead of silently doing nothing.
#[cfg(not(feature = "signal-crypto"))]
pub(super) fn replace_requested() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_REPLACE_PC_KEY")?);
    path.is_absolute().then_some(path)
}
#[cfg(not(feature = "signal-crypto"))]
pub(super) fn claim_replace() -> bool {
    static DONE: AtomicBool = AtomicBool::new(false);
    !DONE.swap(true, Ordering::SeqCst)
}
#[cfg(not(feature = "signal-crypto"))]
pub(super) fn note_unavailable(path: &std::path::Path) {
    if let Ok(mut file) = create(path) {
        let _ = writeln!(
            file,
            "RESULT: not run. This build has no E2EE engine; rebuild with --features live,signal-crypto."
        );
    }
}

fn key_name(name: &str) -> String {
    name.chars()
        .take(40)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '?'
            }
        })
        .collect()
}
/// Lines like `data.keys[3].keys[0].spKey.key: string(44)`; never a string, id or key value.
pub(super) fn shape(value: &Value) -> Vec<String> {
    fn walk(value: &Value, path: &str, name: &str, depth: usize, out: &mut Vec<String>) {
        if out.len() >= MAX_LINES {
            return;
        }
        let here = if path.is_empty() {
            "$".to_owned()
        } else {
            path.to_owned()
        };
        match value {
            Value::Null => out.push(format!("{here}: null")),
            Value::Bool(flag) if VALUE_FIELDS.contains(&name) => {
                out.push(format!("{here}: {flag}"))
            }
            Value::Bool(_) => out.push(format!("{here}: bool")),
            Value::Number(number) if VALUE_FIELDS.contains(&name) => {
                out.push(format!("{here}: number = {number}"))
            }
            Value::Number(_) => out.push(format!("{here}: number")),
            Value::String(text) => out.push(format!("{here}: string({})", text.len())),
            Value::Array(items) => {
                out.push(format!("{here}: array({})", items.len()));
                if depth < MAX_DEPTH {
                    // The first few elements (e.g. one bundle per device); the length above says how many.
                    for (index, item) in items.iter().take(MAX_ITEMS).enumerate() {
                        walk(item, &format!("{path}[{index}]"), name, depth + 1, out);
                    }
                }
            }
            Value::Object(map) => {
                out.push(format!("{here}: object({})", map.len()));
                if depth < MAX_DEPTH {
                    for (key, item) in map {
                        let key = key_name(key);
                        let next = if path.is_empty() {
                            key.clone()
                        } else {
                            format!("{path}.{key}")
                        };
                        walk(item, &next, &key, depth + 1, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(value, "", "", 0, &mut out);
    out
}

fn create(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

impl Connection {
    /// Best effort and bounded: any failure is written to the file and the session carries on.
    pub(super) async fn e2ee_probe(&mut self, path: &std::path::Path) {
        let Ok(mut file) = create(path) else {
            return;
        };
        let _ = writeln!(
            file,
            "# zalo-native E2EE probe (read-only). Shapes only; no keys, ids or message data."
        );
        let own = self.account.clone();
        let (own_key, bundles) = (self.request(), self.request());
        let requests = [
            (564u16, own_key, json!({"platformType": 2})),
            (
                541,
                bundles,
                json!({"uids":[{"uid": own, "deviceId": -1}],"isReInit":0,"msgInfoNoti":0,"isAutoUpgrade":0}),
            ),
        ];
        for (cmd, id, data) in &requests {
            if self
                .send(*cmd, 0, json!({"reqId": id, "data": data}))
                .await
                .is_err()
            {
                let _ = writeln!(file, "## command {cmd}: send failed");
                return;
            }
        }
        let mut pending: Vec<u16> = requests.iter().map(|(cmd, _, _)| *cmd).collect();
        let deadline = tokio::time::Instant::now() + WAIT;
        while !pending.is_empty() {
            match tokio::time::timeout_at(deadline, self.next()).await {
                Err(_) => {
                    let _ = writeln!(file, "## timeout waiting for {pending:?}");
                    break;
                }
                Ok(Err(error)) => {
                    // A rejected outer envelope is itself the answer; the error names a category.
                    let _ = writeln!(file, "## reply rejected before parsing: {error}");
                    break;
                }
                Ok(Ok(None)) => {}
                Ok(Ok(Some(frame))) => {
                    if pending.contains(&frame.cmd) {
                        pending.retain(|cmd| *cmd != frame.cmd);
                        let _ = writeln!(file, "## command {} sub {}", frame.cmd, frame.sub_cmd);
                        for line in shape(&frame.payload) {
                            let _ = writeln!(file, "{line}");
                        }
                    }
                }
            }
        }
        let _ = file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Obviously fake placeholder text of a given length.
    fn filler(letter: &str, len: usize) -> String {
        letter.repeat(len)
    }

    #[test]
    fn shape_keeps_structure_and_a_few_status_values_but_never_secrets() {
        let reply = json!({
            "error_code": 0,
            "data": {"reqId": "signal_req_9", "data": {
                "err": -1,
                "keyId": 12345,
                "submitTs": 1700000000000u64,
                "keys": [{"uid": "123456789", "keys": [{
                    "deviceId": 2,
                    "spKey": {"id": 7, "key": filler("K", 44), "sig": filler("S", 12)},
                    "opKeys": [{"id": 1, "key": filler("O", 8)}, {"id": 2, "key": filler("P", 8)}]
                }]}]
            }}
        });
        let text = shape(&reply).join("\n");
        for needed in [
            "$: object(2)",
            "error_code: number = 0",
            "data.data.err: number = -1",
            "data.data.keyId: number",
            "data.data.keys: array(1)",
            "data.data.keys[0].keys[0].deviceId: number = 2",
            "data.data.keys[0].keys[0].spKey.key: string(",
            "data.data.keys[0].keys[0].opKeys: array(2)",
        ] {
            assert!(text.contains(needed), "{needed}\n{text}");
        }
        for secret in [
            "12345",
            "1700000000000",
            "123456789",
            "OOOOOOOO",
            "signal_req_9",
            "KKKKKKKK",
        ] {
            assert!(!text.contains(secret), "{secret} leaked:\n{text}");
        }
        assert!(!text.contains("keyId: number ="));
    }

    #[test]
    fn shape_is_bounded_and_sanitizes_peer_controlled_names() {
        let mut deep = json!("x");
        for _ in 0..40 {
            deep = json!({ "a": deep });
        }
        assert!(shape(&deep).len() <= MAX_DEPTH + 2);
        let wide: serde_json::Map<String, Value> =
            (0..1000).map(|i| (format!("k{i}"), json!(i))).collect();
        assert!(shape(&Value::Object(wide)).len() <= MAX_LINES + 1);
        let hostile = json!({"a/\n\u{202e}b": 1});
        let text = shape(&hostile).join("\n");
        assert!(text.contains("a???b: number") && !text.contains('\u{202e}'));
    }

    #[test]
    fn only_an_absolute_path_enables_the_probe_and_it_runs_once() {
        std::env::set_var("ZALO_E2EE_PROBE", "relative.txt");
        assert!(target().is_none());
        std::env::set_var("ZALO_E2EE_PROBE", "/tmp/probe.txt");
        assert_eq!(target(), Some("/tmp/probe.txt".into()));
        std::env::remove_var("ZALO_E2EE_PROBE");
        assert!(target().is_none());
        assert!(claim());
        assert!(!claim());
    }

    #[tokio::test]
    async fn probe_sends_only_the_two_read_commands_and_writes_shapes_to_a_private_file() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            {
                use base64::{engine::general_purpose::STANDARD, Engine};
                let mut z =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                z.write_all(br#"{"error_code":0,"data":{"qCmds":[{"cmd":510,"subCmd":0}]}}"#)
                    .unwrap();
                let auth = wire::encode_frame(
                    1,
                    0,
                    &json!({"error_code":0,"key":STANDARD.encode([7u8; 32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())}),
                )
                .unwrap();
                socket
                    .send(tungstenite::Message::Binary(auth))
                    .await
                    .unwrap();
            }
            let mut seen = Vec::new();
            for _ in 0..2 {
                let Some(Ok(tungstenite::Message::Binary(bytes))) = socket.next().await else {
                    panic!("expected a binary frame");
                };
                let cmd = u16::from(bytes[1]) | (u16::from(bytes[2]) << 8);
                let body: Value = serde_json::from_slice(&bytes[4..]).unwrap();
                seen.push((cmd, body));
            }
            for (cmd, body) in &seen {
                let reply = match cmd {
                    564 => json!({"err": -1}),
                    _ => json!({"keys": [{"uid": "77", "keys": []}]}),
                };
                let frame = wire::encode_frame(
                    *cmd,
                    0,
                    &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":reply}}}),
                )
                .unwrap();
                socket
                    .send(tungstenite::Message::Binary(frame))
                    .await
                    .unwrap();
            }
            seen
        });
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "77");
        // The decoder only accepts replies after the authentication frame, as in a real session.
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        let dir = std::env::temp_dir().join(format!("zalo-probe-{:x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.txt");
        connection.e2ee_probe(&path).await;
        let seen = server.await.unwrap();
        assert_eq!(
            seen.iter().map(|(cmd, _)| *cmd).collect::<Vec<_>>(),
            [564, 541]
        );
        assert_eq!(seen[0].1["data"], json!({"platformType": 2}));
        assert_eq!(
            seen[1].1["data"]["uids"],
            json!([{"uid": "77", "deviceId": -1}])
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("## command 564") && text.contains("data.data.err: number = -1"),
            "{text}"
        );
        assert!(text.contains("## command 541") && text.contains("data.data.keys: array(1)"));
        assert!(!text.contains("77\n") && !text.contains("signal_req"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
