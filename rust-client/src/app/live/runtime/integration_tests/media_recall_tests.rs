use super::*;
use crate::{
    domain::{MediaKind, MediaMessageContent},
    store::{
        attachments::{AttachmentResource, AttachmentSpec},
        photos::{PhotoResource, PhotoSpec},
    },
};

const SERVER_MESSAGE: &str = "913";

#[derive(Clone, Copy)]
pub(super) struct Operation {
    group: bool,
    source_client: u64,
    drop_receipt: bool,
}

impl Operation {
    pub(super) fn respond(
        self,
        stream: &mut StreamOwned<ServerConnection, TcpStream>,
        state: &State,
        cipher: &SessionCipher,
        path: &str,
        headers: &str,
        body: &[u8],
    ) {
        let (route, query) = path.split_once('?').unwrap();
        if matches!(route, "/api/group/getgi/v2" | "/api/group/getmg-v2") {
            assert!(self.group);
            group_recall_tests::respond(
                stream,
                state,
                cipher,
                group_recall_tests::Mode::Accepted,
                path,
                headers,
                body,
            );
            return;
        }
        assert_eq!(
            route,
            if self.group {
                "/api/group/undomsg"
            } else {
                "/api/message/undo"
            }
        );
        assert_eq!(query, "zpw_ver=681&zpw_type=24");
        assert!(headers.starts_with("POST "));
        assert!(headers
            .to_ascii_lowercase()
            .contains("cookie: zpw_sek=synthetic"));
        let form: Vec<_> = url::form_urlencoded::parse(body).collect();
        assert_eq!(form.len(), 1);
        assert_eq!(form[0].0, "params");
        let params: Value = serde_json::from_slice(&cipher.decrypt(&form[0].1).unwrap()).unwrap();
        let client = params["clientId"].as_u64().unwrap();
        assert!(client > self.source_client);
        let mut expected = json!({
            "msgId":SERVER_MESSAGE,"clientId":client,"cliMsgIdUndo":self.source_client.to_string()
        });
        if self.group {
            expected["grid"] = json!("22");
            expected["visibility"] = json!(0);
            expected["imei"] = json!("native-fixture");
        } else {
            expected["toid"] = json!("22");
        }
        assert_eq!(params, expected);
        state.http_sends.fetch_add(1, Ordering::SeqCst);
        if self.drop_receipt {
            return;
        }
        let inner = serde_json::to_vec(&json!({"error_code":0,"data":null})).unwrap();
        let encrypted = cipher.encrypt(&inner).unwrap();
        let response = serde_json::to_vec(&json!({"error_code":0,"data":encrypted})).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        )
        .unwrap();
        stream.write_all(&response).unwrap();
        stream.flush().unwrap();
    }
}

fn accept_direct_photo(store: &mut MessageStore, timestamp: i64) -> u64 {
    let spec = PhotoSpec {
        caption: "Ảnh thử thu hồi".into(),
        width: 80,
        height: 60,
        size: 1000,
        checksum: "a".repeat(32),
    };
    let client = store.reserve_photo("11", "d:22", &spec, timestamp).unwrap();
    let original = "https://files.zdn.vn/photo-original.jpg";
    let thumb = "https://files.zdn.vn/photo-thumb.jpg";
    let hd = "https://files.zdn.vn/photo-hd.jpg";
    let resource = PhotoResource {
        photo_id: "synthetic-photo".into(),
        parameters: json!({
            "photoId":"synthetic-photo","clientId":client,"desc":spec.caption,
            "width":spec.width,"height":spec.height,"rawUrl":original,"thumbUrl":thumb,
            "hdUrl":hd,"language":"VN","imei":"native-fixture","ttl":0,
            "toid":"22","normalUrl":original
        }),
        content: MediaMessageContent::new(
            MediaKind::Photo,
            &json!({"title":"","description":spec.caption,"href":original,"thumb":thumb,
                "params":{"width":spec.width,"height":spec.height,"hd":hd,"rawUrl":original}}),
        )
        .unwrap(),
    };
    store.photo_resource_ready("11", client, &resource).unwrap();
    store.begin_photo_dispatch("11", client).unwrap();
    store
        .record_photo_ack("11", client, SERVER_MESSAGE, Some(timestamp))
        .unwrap();
    store.accept_photo("11", client).unwrap();
    client
}

fn accept_group_file(store: &mut MessageStore, timestamp: i64) -> u64 {
    let spec = AttachmentSpec {
        file_name: "Tài liệu.txt".into(),
        size: 6,
        checksum: "b".repeat(32),
    };
    let client = store
        .reserve_attachment("11", "g:22", &spec, timestamp)
        .unwrap();
    let address = "https://files.zdn.vn/synthetic-file.txt";
    let resource = AttachmentResource {
        file_id: "synthetic-file".into(),
        file_url: address.into(),
        checksum: spec.checksum.clone(),
        parameters: json!({
            "fileId":"synthetic-file","checksum":spec.checksum,"checksumSha":"",
            "extension":"txt","totalSize":spec.size,"fileName":spec.file_name,"clientId":client,
            "fType":1,"fileCount":0,"fdata":"{}","fileUrl":address,"ttl":0,"grid":"22"
        }),
        content: MediaMessageContent::new(
            MediaKind::File,
            &json!({"title":spec.file_name,"description":"","href":address,"params":{
                "fileSize":spec.size,"checksum":spec.checksum,"checksumSha":"",
                "fileExt":"txt","fdata":"{}","fType":1}}),
        )
        .unwrap(),
    };
    store
        .attachment_resource_ready("11", client, &resource)
        .unwrap();
    store.begin_attachment_dispatch("11", client).unwrap();
    store
        .record_attachment_ack("11", client, SERVER_MESSAGE, Some(timestamp))
        .unwrap();
    store.accept_attachment("11", client).unwrap();
    client
}

#[tokio::test]
async fn actual_media_recall_uses_typed_receipt_and_does_not_retry_unknown() {
    for (group, drop_receipt) in [(false, false), (true, false), (false, true)] {
        let timestamp = now_ms();
        let operation = Operation {
            group,
            source_client: timestamp as u64,
            drop_receipt,
        };
        let fixture = Fixture::new(Reply::MediaRecall(operation));
        let session = fixture.session();
        let path = fixture.profile.paths.data_dir.join("messages.sqlite");
        let thread = if group { "g:22" } else { "d:22" };
        let mut store = MessageStore::open(&path).unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        let original_client = if group {
            accept_group_file(&mut store, timestamp)
        } else {
            accept_direct_photo(&mut store, timestamp)
        };
        assert_eq!(original_client, operation.source_client);
        drop(store);

        for request_id in 1..=if drop_receipt { 2 } else { 1 } {
            let (commands, requests) = mpsc::channel(8);
            let (events, mut incoming) = mpsc::channel(32);
            let (stop, cancel) = watch::channel(false);
            let actor = tokio::spawn(run_with_transports(
                fixture.profile.clone(),
                session.clone(),
                requests,
                events,
                Arc::new(|| {}),
                cancel,
                fixture.transports(),
            ));
            tokio::time::timeout(Duration::from_secs(5), async {
                let (mut online, mut privacy) = (false, false);
                while !online || !privacy {
                    match incoming.recv().await.unwrap() {
                        LiveEvent::Connection {
                            state: LiveConnection::Online,
                            ..
                        } => online = true,
                        LiveEvent::PrivacyChanged { revision, .. } => privacy |= revision > 1,
                        _ => {}
                    }
                }
                commands
                    .send(LiveCommand::Recall {
                        request_id,
                        thread: thread.into(),
                        message_id: SERVER_MESSAGE.into(),
                    })
                    .await
                    .unwrap();
                let mut redacted = false;
                loop {
                    match incoming.recv().await.unwrap() {
                        LiveEvent::Recalled {
                            thread: actual_thread,
                            message_id,
                            actor_id,
                            message,
                            ..
                        } => {
                            assert_eq!(
                                (
                                    actual_thread.as_str(),
                                    message_id.as_str(),
                                    actor_id.as_str()
                                ),
                                (thread, SERVER_MESSAGE, "11")
                            );
                            let message = message.unwrap();
                            assert!(message.is_recalled());
                            assert!(message.media.is_none());
                            redacted = true;
                        }
                        LiveEvent::RecallFinished {
                            request_id: actual_id,
                            thread: actual_thread,
                            message_id,
                            outcome,
                            ..
                        } => {
                            assert_eq!(actual_id, request_id);
                            assert_eq!(
                                (actual_thread.as_str(), message_id.as_str()),
                                (thread, SERVER_MESSAGE)
                            );
                            assert_eq!(
                                outcome,
                                if drop_receipt {
                                    RecallOutcome::Unknown
                                } else {
                                    RecallOutcome::Accepted
                                }
                            );
                            assert_eq!(redacted, !drop_receipt);
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap_or_else(|_| panic!("media recall timed out: {group}/{drop_receipt}"));
            stop.send(true).unwrap();
            actor.await.unwrap();
        }
        assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 1);
        let store = MessageStore::open(&path).unwrap();
        let messages = store.page("11", thread, None, 10).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].is_recalled(), !drop_receipt);
        assert_eq!(messages[0].media.is_some(), drop_receipt);
    }
}
