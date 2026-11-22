//! Synthetic TLS file-service and multiplexed control witness; never connects outward.
use super::*;

pub(super) const ALBUM_ACCEPT: u8 = 20;
pub(super) const ALBUM_REJECT_SECOND: u8 = 21;
pub(super) const ALBUM_LOSE_SECOND_ACK: u8 = 22;
pub(super) const ALBUM_RECALL_FIRST: u8 = 23;

fn album_mode(mode: u8) -> bool {
    (ALBUM_ACCEPT..=ALBUM_RECALL_FIRST).contains(&mode)
}

fn album_photo_id(parameters: &Value) -> String {
    format!("album-{}", parameters["clientId"].as_u64().unwrap())
}
#[derive(Default)]
pub(super) struct FileState {
    pub paths: Mutex<Vec<String>>,
    pub uploads: Mutex<Vec<(Value, Vec<u8>)>>,
    pub finals: Mutex<Vec<Value>>,
    pub control_sent: AtomicBool,
    pub final_started: AtomicBool,
    pub stalled: AtomicBool,
    pub eligibility_requests: AtomicUsize,
    pub original_plan_requests: AtomicUsize,
    pub echo_sent: AtomicBool,
    control_pending: AtomicBool,
    echo_pending: Mutex<Option<Value>>,
}
impl Fixture {
    pub(super) fn file_session(&self) -> Arc<VerifiedSession> {
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"conversation":["https://wpa.zaloapp.com"],"file":["https://wpa.zaloapp.com"]},"zpw_ws":["wss://wpa.zaloapp.com"],"settings":{"sharefile":{"chunk_size_file":64000}}})).unwrap();
        Arc::new(synthetic_session(self.profile.clone(), config))
    }
    pub(super) fn photo_session(&self, new_endpoint: bool) -> Arc<VerifiedSession> {
        self.photo_session_with_limits(new_endpoint, 2560, 2560, 5 * 1024 * 1024)
    }
    pub(super) fn album_session(
        &self,
        new_endpoint: bool,
        max_count: u32,
        extra_metadata: bool,
    ) -> Arc<VerifiedSession> {
        let config = ServerConfig::parse(&json!({
            "zpw_service_map_v3":{
                "chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],
                "conversation":["https://wpa.zaloapp.com"],"file":["https://wpa.zaloapp.com"],
                "up_file":["https://wpa.zaloapp.com"]
            },
            "zpw_ws":["wss://wpa.zaloapp.com"],
            "settings":{
                "sharefile":{"chunk_size_file":64000,"max_file":max_count},
                "features":{"should_attach_extraMsgProp":extra_metadata},
                "file_photo_limit":{"enable_new_endpoint_upload_photo":if new_endpoint{1}else{0}}
            }
        }))
        .unwrap();
        Arc::new(synthetic_session(self.profile.clone(), config))
    }
    pub(super) fn photo_session_with_limits(
        &self,
        new_endpoint: bool,
        width: u32,
        height: u32,
        size: u64,
    ) -> Arc<VerifiedSession> {
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"conversation":["https://wpa.zaloapp.com"],"file":["https://wpa.zaloapp.com"],"up_file":["https://wpa.zaloapp.com"]},"zpw_ws":["wss://wpa.zaloapp.com"],"settings":{"sharefile":{"chunk_size_file":64000},"file_photo_limit":{"enable_new_endpoint_upload_photo":if new_endpoint{1}else{0},"resolutionLimit":{"hd":{"width":width,"height":height,"size":size}}}}})).unwrap();
        Arc::new(synthetic_session(self.profile.clone(), config))
    }
}
type Socket = tungstenite::WebSocket<StreamOwned<ServerConnection, TcpStream>>;
fn send(ws: &mut Socket, cmd: u16, sub: u8, payload: Value) -> bool {
    ws.send(tungstenite::Message::Binary(
        wire::encode_frame(
            cmd,
            sub,
            &json!({"error_code":0,"encrypt":0,"data":payload}),
        )
        .unwrap(),
    ))
    .is_ok()
}
pub(super) fn socket(mut ws: Socket, state: Arc<State>, mode: u8, photo: bool) {
    let transfer = if photo { &state.photo } else { &state.file };
    let auth = json!({"error_code":0,"data":{"qCmds":[{"cmd":510,"subCmd":0}]}});
    let mut compressed =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    compressed
        .write_all(&serde_json::to_vec(&auth).unwrap())
        .unwrap();
    let auth=wire::encode_frame(1,0,&json!({"error_code":0,"key":STANDARD.encode([7;32]),"encrypt":1,"data":STANDARD.encode(compressed.finish().unwrap())})).unwrap();
    if ws.send(tungstenite::Message::Binary(auth)).is_err() {
        return;
    }
    ws.get_mut()
        .sock
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut album_recall_sent = false;
    while std::time::Instant::now() < deadline {
        if transfer.control_pending.swap(false, Ordering::SeqCst) {
            let mut controls = json!({"error_code":0,"data":{"controls":[{"controlId":"1","content":{"act_type":"other_control","data":{}}},{"controlId":"2","content":{"act_type":"file_done","fileId":"wrong-file","data":{"url":"https://files.example.test/wrong"}}},{"controlId":"3","content":{"act_type":"file_done","fileId":"987","data":"{\"url\":\"https://files.example.test/uploaded\"}"}}]}});
            if photo {
                let uploads = transfer.uploads.lock().unwrap();
                let (parameters, _) = uploads.last().unwrap();
                let size = &parameters["totalSize"];
                let id = if album_mode(mode) {
                    album_photo_id(parameters)
                } else {
                    "987".to_string()
                };
                let tag = if album_mode(mode) {
                    id.as_str()
                } else {
                    "uploaded"
                };
                let original = format!("https://files.example.test/{tag}");
                controls = json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"file_done","fileId":id,"data":{"url":"https://files.example.test/not-photo"}}},{"content":{"act_type":"photo_success","act":"photo","fileId":id,"data":{"org":original,"thumb":"https://files.example.test/thumb","hd":"https://files.example.test/hd","fileSize":size}}}]}});
                if mode == 12 {
                    controls["data"]["controls"][1]["content"]["data"]
                        .as_object_mut()
                        .unwrap()
                        .remove("org");
                }
            }
            // Offline controls deliberately remain unconsumed; a matching offline URL must not win.
            let offline = json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"file_done","fileId":"987","data":{"url":"https://files.example.test/offline-wrong"}}}]}});
            if !send(&mut ws, 603, 0, offline) || !send(&mut ws, 601, 0, controls) {
                break;
            }
            transfer.control_sent.store(true, Ordering::SeqCst);
        }
        let pending = transfer.echo_pending.lock().unwrap().take();
        if let Some(parameters) = pending {
            let content = if photo {
                json!({"title":"","description":parameters["desc"],"href":parameters["rawUrl"],"thumb":parameters["thumbUrl"],"params":{"width":parameters["width"],"height":parameters["height"],"hd":parameters["hdUrl"],"rawUrl":parameters["rawUrl"]}})
            } else {
                let file = crate::protocol::upload::media_content(
                    parameters["fileName"].as_str().unwrap(),
                    parameters["extension"].as_str().unwrap(),
                    parameters["totalSize"].as_u64().unwrap(),
                    parameters["checksum"].as_str().unwrap(),
                    parameters["fileUrl"].as_str().unwrap(),
                )
                .unwrap();
                serde_json::from_str::<Value>(file.as_json()).unwrap()
            };
            let message = json!({"uidFrom":"11","idTo":parameters.get("toid").unwrap_or(&parameters["grid"]),"msgId":"901","cliMsgId":parameters["clientId"],"ts":"1760000000001","msgType":if photo{"chat.photo"}else{"share.file"},"content":content});
            let group = parameters.get("grid").is_some();
            let mut payload = json!({"error_code":0,"data":{}});
            payload["data"][if group { "groupMsgs" } else { "msgs" }] = json!([message]);
            if !send(
                &mut ws,
                if group { 521 } else { 501 },
                if group { 1 } else { 0 },
                payload,
            ) {
                break;
            }
            transfer.echo_sent.store(true, Ordering::SeqCst);
        }
        let message = match ws.read() {
            Ok(message) => message,
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => break,
        };
        match message {
            tungstenite::Message::Binary(bytes) => {
                let cmd = u16::from_le_bytes([bytes[1], bytes[2]]);
                let sub = bytes[3];
                state.observed_commands.lock().unwrap().push(cmd);
                let data: Value = serde_json::from_slice(&bytes[4..]).unwrap();
                match cmd {
                    510 | 511 => {
                        let mut page =
                            json!({"error_code":0,"data":{"more":false,"lastActionId":"100"}});
                        page["data"][if cmd == 510 { "msgs" } else { "groupMsgs" }] = json!([]);
                        if !send(&mut ws, cmd, sub, page) {
                            break;
                        }
                    }
                    563 => {
                        let count = transfer.eligibility_requests.fetch_add(1, Ordering::SeqCst);
                        if mode == 8 && count == 0 {
                            transfer.stalled.store(true, Ordering::SeqCst);
                            continue;
                        }
                        if mode == ALBUM_RECALL_FIRST && !album_recall_sent {
                            let client_id = transfer
                                .finals
                                .lock()
                                .unwrap()
                                .first()
                                .map(|parameters| parameters["clientId"].clone());
                            if let Some(client_id) = client_id {
                                let notice = json!({"uidFrom":"0","idTo":"22","msgId":"999",
                                    "ts":"1760000000002","msgType":"chat.undo",
                                    "content":{"globalMsgId":"901","cliMsgId":client_id,
                                        "srcId":"11","destId":"22"}});
                                let payload = json!({"error_code":0,"data":{"msgs":[notice]}});
                                if !send(&mut ws, 501, 0, payload) {
                                    break;
                                }
                                album_recall_sent = true;
                            }
                        }
                        let mut page = json!({"err":0,"hasMore":false,"uids":[],"threads":[],"waitUids":[],"pendingUids":[]});
                        if state.eligibility_mode.load(Ordering::SeqCst) == 21 {
                            page["uids"] = json!(["22"]);
                        }
                        if !send(
                            &mut ws,
                            563,
                            0,
                            json!({"error_code":0,"data":{"reqId":data["reqId"],"data":page}}),
                        ) {
                            break;
                        }
                    }
                    2 => {
                        state.pings.fetch_add(1, Ordering::SeqCst);
                        if !send(
                            &mut ws,
                            2,
                            sub,
                            json!({"error_code":0,"eventId":data["eventId"]}),
                        ) {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            tungstenite::Message::Ping(bytes) => {
                if ws.send(tungstenite::Message::Pong(bytes)).is_err() {
                    break;
                }
            }
            tungstenite::Message::Close(_) => break,
            _ => {}
        }
    }
    state.closed.fetch_add(1, Ordering::SeqCst);
}
fn response(stream: &mut StreamOwned<ServerConnection, TcpStream>, inner: Value) {
    let cipher =
        SessionCipher::from_base64("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
    let bytes=serde_json::to_vec(&json!({"error_code":0,"data":cipher.encrypt(&serde_json::to_vec(&inner).unwrap()).unwrap()})).unwrap();
    write_json(stream, &bytes);
}
fn write_json(stream: &mut StreamOwned<ServerConnection, TcpStream>, bytes: &[u8]) {
    if write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )
    .is_ok()
    {
        let _ = stream.write_all(bytes);
        let _ = stream.flush();
    }
}
fn wait(flag: &AtomicBool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !flag.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
/// Returns true only for file-service requests; shared directory/privacy fixtures keep their handlers.
pub(super) fn http(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    state: &Arc<State>,
    mode: u8,
    path: &str,
    headers: &str,
    body: &[u8],
    photo: bool,
) -> bool {
    let transfer = if photo { &state.photo } else { &state.file };
    let route = path.split('?').next().unwrap();
    if photo && (10..=12).contains(&mode) {
        match route {
            "/api/getCloudViewerKey" => {
                assert_eq!(path, "/api/getCloudViewerKey?zpw_ver=681&zpw_type=24");
                assert!(headers.starts_with("GET "));
                assert!(headers
                    .to_ascii_lowercase()
                    .contains("cookie: zpw_sek=synthetic"));
                assert!(!headers.to_ascii_lowercase().contains("cloud-viewer-key:"));
                response(
                    stream,
                    json!({"error_code":0,"data":{"viewer_key":"fixture-viewer",
                    "enk":STANDARD.encode([7u8;32])}}),
                );
                return true;
            }
            "/cloudmedia/info/pc/v1/usage-mycloud" => {
                use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
                assert_eq!(
                    path,
                    "/cloudmedia/info/pc/v1/usage-mycloud?zpw_ver=681&zpw_type=24"
                );
                assert!(headers.starts_with("GET "));
                let headers = headers.to_ascii_lowercase();
                assert!(headers.contains("cookie: zpw_sek=synthetic"));
                assert!(headers.contains("cloud-viewer-key: fixture-viewer"));
                let previous = transfer
                    .original_plan_requests
                    .fetch_add(1, Ordering::SeqCst);
                let plan = if mode == 11 && previous > 0 { 69 } else { 0 };
                let iv = [9u8; 16];
                let payload = json!({"plan":plan}).to_string();
                let cipher = cbc::Encryptor::<aes::Aes256>::new_from_slices(&[7u8; 32], &iv)
                    .unwrap()
                    .encrypt_padded_vec_mut::<Pkcs7>(payload.as_bytes());
                let bytes = serde_json::to_vec(&json!({"error_code":0,"data":STANDARD.encode([iv.as_slice(),cipher.as_slice()].concat())})).unwrap();
                write_json(stream, &bytes);
                return true;
            }
            _ => {}
        }
    }
    if if photo {
        !(route.ends_with("/photo_original/upload")
            || route.ends_with("/upload/photo")
            || route.ends_with("/zcloud-upload/photo")
            || route.ends_with("/photo_original/send"))
    } else {
        !route.ends_with("/asyncfile/upload") && !route.ends_with("/asyncfile/msg")
    } {
        return false;
    }
    transfer.paths.lock().unwrap().push(route.into());
    let cipher =
        SessionCipher::from_base64("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
    let upload = route.ends_with("/upload")
        || route.ends_with("/upload/photo")
        || route.ends_with("/zcloud-upload/photo");
    if photo && (10..=12).contains(&mode) {
        assert!(!headers.to_ascii_lowercase().contains("cloud-viewer-key:"));
        if upload {
            assert!(route.ends_with("/zcloud-upload/photo"));
        }
    }
    let encoded = if upload {
        path.split_once('?').unwrap().1.as_bytes()
    } else {
        body
    };
    let encoded = url::form_urlencoded::parse(encoded)
        .find(|(key, _)| key == "params")
        .unwrap()
        .1
        .into_owned();
    let parameters: Value = serde_json::from_slice(&cipher.decrypt(&encoded).unwrap()).unwrap();
    if upload {
        let content_type = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.trim())
            })
            .unwrap();
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let start = body.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let suffix = format!("\r\n--{boundary}--\r\n");
        assert!(body.ends_with(suffix.as_bytes()));
        assert!(String::from_utf8_lossy(&body[..start]).contains("name=\"chunkContent\""));
        transfer.uploads.lock().unwrap().push((
            parameters.clone(),
            body[start..body.len() - suffix.len()].to_vec(),
        ));
        if mode == 7 {
            transfer.stalled.store(true, Ordering::SeqCst);
            let mut byte = [0];
            let _ = stream.read(&mut byte);
            return true;
        }
        if mode == 2 {
            response(
                stream,
                json!({"error_code":1,"data":if photo{json!({"photoId":"987","normalUrl":"https://files.example.test/uploaded","thumbUrl":"https://files.example.test/thumb","hdUrl":"https://files.example.test/hd"})}else{json!({"fileId":"987","url":"https://files.example.test/uploaded"})}}),
            );
            return true;
        }
        let finished = parameters["chunkId"] == parameters["totalChunk"];
        if finished && mode != 1 {
            if photo && album_mode(mode) {
                transfer.control_sent.store(false, Ordering::SeqCst);
            }
            transfer.control_pending.store(true, Ordering::SeqCst);
            wait(&transfer.control_sent);
        }
        let photo_id = if album_mode(mode) {
            album_photo_id(&parameters)
        } else {
            "987".to_string()
        };
        response(
            stream,
            json!({"error_code":0,"data":if finished{if photo{json!({"finished":1,"photoId":photo_id})}else{json!({"finished":1,"fileId":"987"})}}else{json!({})}}),
        );
        if finished && mode == 1 {
            transfer.control_pending.store(true, Ordering::SeqCst);
        }
    } else {
        state.http_sends.fetch_add(1, Ordering::SeqCst);
        let final_number = {
            let mut finals = transfer.finals.lock().unwrap();
            finals.push(parameters.clone());
            finals.len()
        };
        transfer.final_started.store(true, Ordering::SeqCst);
        if photo && album_mode(mode) {
            if final_number == 2 && mode == ALBUM_LOSE_SECOND_ACK {
                return true;
            }
            let inner = if final_number == 2 && mode == ALBUM_REJECT_SECOND {
                json!({"error_code":1234,"data":{}})
            } else {
                json!({"error_code":0,"data":{"msgId":(900+final_number).to_string(),"ts":"1760000000001"}})
            };
            response(stream, inner);
            return true;
        }
        if mode == 9 {
            transfer.stalled.store(true, Ordering::SeqCst);
            let mut byte = [0];
            let _ = stream.read(&mut byte);
            return true;
        }
        if matches!(mode, 4..=6) {
            *transfer.echo_pending.lock().unwrap() = Some(parameters);
            wait(&transfer.echo_sent);
            std::thread::park_timeout(Duration::from_millis(80));
        }
        if mode == 3 || mode == 4 {
            return true;
        }
        response(
            stream,
            json!({"error_code":0,"data":{"msgId":if mode==6{"902"}else{"901"},"ts":"1760000000001"}}),
        );
    }
    true
}
