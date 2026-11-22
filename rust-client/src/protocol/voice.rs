//! Ordinary voice multipart submission, as defined by the desktop bundle.
use super::{
    client::ClientType,
    messaging::{self, Target, ThreadKind, WireError},
};
use serde_json::{json, Value};
use url::Url;

/// The FILE-origin request in compact-app-pc at 489489–489503.
pub fn endpoint(root: &Url, kind: ThreadKind, client: ClientType) -> Result<Url, WireError> {
    messaging::validate_origin(root, "https")?;
    let (path, scope) = match kind {
        ThreadKind::Direct => ("/api/message/voicemp3", "2"),
        ThreadKind::Group => ("/api/group/sendmp3", "11"),
    };
    let mut url = root.clone();
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("zpw_ver", messaging::API_VERSION)
        .append_pair("zpw_type", client.api_type())
        .append_pair("type", scope);
    Ok(url)
}

/// Encrypt this object as the URL's `params` when posting `fileContent`.
pub fn message_parameters(target: &Target, client_id: u64) -> Result<Value, WireError> {
    messaging::numeric_id(&target.remote_id)?;
    if !(1..=9_007_199_254_740_991).contains(&client_id) {
        return Err(WireError::Invalid);
    }
    let recipient = match target.kind {
        ThreadKind::Direct => "toid",
        ThreadKind::Group => "grid",
    };
    let mut fields = json!({"clientId":client_id.to_string()});
    fields[recipient] = json!(target.remote_id);
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{MediaDescriptor, MediaKind},
        protocol::bootstrap::ServerConfig,
    };

    #[test]
    fn legacy_send_routes_bind_ciphertext_parameters_to_target() {
        let root = Url::parse("https://files-wpa.zaloapp.com").unwrap();
        for (kind, path, scope, recipient) in [
            (ThreadKind::Direct, "/api/message/voicemp3", "2", "toid"),
            (ThreadKind::Group, "/api/group/sendmp3", "11", "grid"),
        ] {
            let url = endpoint(&root, kind, ClientType::PcLinux24).unwrap();
            assert_eq!(url.path(), path);
            assert!(url
                .query_pairs()
                .any(|(key, value)| key == "type" && value == scope));
            assert!(url
                .query_pairs()
                .any(|(key, value)| key == "zpw_type" && value == "24"));
            let params = message_parameters(&Target::new(kind, "22").unwrap(), 9000).unwrap();
            assert_eq!(params, json!({(recipient):"22","clientId":"9000"}));
        }
        assert!(endpoint(
            &Url::parse("https://example.invalid").unwrap(),
            ThreadKind::Direct,
            ClientType::PcLinux24
        )
        .is_err());
    }

    #[test]
    fn authenticated_voice_recording_disable_applies_to_explicit_configuration() {
        let allowed = |value| ServerConfig::parse(&value).unwrap().voice_recording_allowed;
        assert!(allowed(json!({})));
        assert!(allowed(
            json!({"settings":{"features":{"isOffVoiceRecord":false}}})
        ));
        assert!(!allowed(json!({"setttings":{"isOffVoiceRecord":1}})));
        assert!(!allowed(
            json!({"isOffVoiceRecord":false,"settings":{"isOffVoiceRecord":true}})
        ));
        assert!(!allowed(json!({"isOffVoiceRecord":"0"})));
    }

    #[test]
    fn voice_echo_retains_media_and_never_projects_encrypted_sources() {
        let voice_url = "https://cache.voice.talk.zdn.vn/a.mp3?synthetic=1";
        let content = json!({"href":voice_url,"params":"{\"duration\":\"4050\"}"});
        let mut payload = json!({"error_code":0,"data":{"msgs":[{
            "uidFrom":"0","idTo":"22","msgId":"811","cliMsgId":"9000",
            "ts":"1760000000001","msgType":"chat.voice","content":content
        }]}});
        let voice = messaging::incoming_messages("11", &payload)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(voice.message.sender, "11");
        assert_eq!(voice.message.thread_id, "d:22");
        assert_eq!(voice.client_id.as_deref(), Some("9000"));
        assert_eq!(voice.message.text, MediaKind::Voice.legacy_placeholder());
        let media = voice.media.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(media.as_json()).unwrap(),
            content
        );
        let MediaDescriptor::Voice(voice) = media.descriptor() else {
            panic!("expected voice descriptor");
        };
        assert_eq!(voice.url.as_deref(), Some(voice_url));
        assert_eq!(voice.duration_ms, Some(4050));
        payload["data"]["msgs"][0]["e2eeExt"] = json!({});
        let encrypted = messaging::incoming_messages("11", &payload)
            .unwrap()
            .pop()
            .unwrap();
        assert!(encrypted.media.is_none() && encrypted.client_id.is_none());
        assert_eq!(
            encrypted.message.text,
            MediaKind::Voice.legacy_placeholder()
        );
    }
}
