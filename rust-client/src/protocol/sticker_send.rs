//! Ordinary sticker submit from the shipped client's `sendSticker`.
use super::{
    client::ClientType,
    messaging::{self as wire, SendAck, Target, ThreadKind, WireError},
};
use crate::domain::{StickerDescriptor, MAX_STICKER_ID};
use serde_json::{json, Value};
use url::Url;

pub(super) const DIRECT_PATH: &str = "/api/message/sticker";
pub(super) const GROUP_PATH: &str = "/api/group/sticker";

pub(crate) fn url(root: &Url, kind: ThreadKind, client_type: ClientType) -> Result<Url, WireError> {
    wire::validate_origin(root, "https")?;
    let mut url = root.clone();
    url.set_path(match kind {
        ThreadKind::Direct => DIRECT_PATH,
        ThreadKind::Group => GROUP_PATH,
    });
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", client_type.api_type());
    Ok(url)
}

pub(crate) fn parameters(
    target: &Target,
    sticker: StickerDescriptor,
    client_id: u64,
    imei: &str,
) -> Result<Value, WireError> {
    wire::numeric_id(&target.remote_id)?;
    if !sticker.can_send()
        || !(1..=MAX_STICKER_ID).contains(&client_id)
        || imei.is_empty()
        || imei.len() > 256
    {
        return Err(WireError::Invalid);
    }
    // buildParams adds the default zsource when no analytics tracking ID
    // exists, then fills ttl from the already verified normal conversation.
    let mut payload = json!({
        "stickerId": sticker.sticker_id,
        "cateId": sticker.category_id,
        "type": sticker.sticker_type,
        "clientId": client_id,
        "imei": imei,
        "zsource": -1,
        "ttl": 0,
    });
    payload[match target.kind {
        ThreadKind::Direct => "toid",
        ThreadKind::Group => "grid",
    }] = json!(target.remote_id);
    Ok(payload)
}

pub(crate) fn ack(payload: &Value, target: &Target, client_id: u64) -> Result<SendAck, WireError> {
    let ack = wire::send_ack(payload)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    if let Some(id) = data.get("clientId") {
        if wire::id_string(id)? != client_id.to_string() {
            return Err(WireError::Invalid);
        }
    }
    let (address, other_address) = match target.kind {
        ThreadKind::Direct => ("toid", "grid"),
        ThreadKind::Group => ("grid", "toid"),
    };
    if data.get(other_address).is_some()
        || data
            .get(address)
            .is_some_and(|id| wire::id_string(id).as_deref() != Ok(target.remote_id.as_str()))
        || ack
            .timestamp_ms
            .is_some_and(|ts| ts > MAX_STICKER_ID as i64)
    {
        return Err(WireError::Invalid);
    }
    Ok(ack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticker_request_and_ack_are_scoped_to_the_original_direct_and_group_routes() {
        for (kind, path, address) in [
            (ThreadKind::Direct, DIRECT_PATH, "toid"),
            (ThreadKind::Group, GROUP_PATH, "grid"),
        ] {
            let target = Target::new(kind, "22").unwrap();
            let sticker = StickerDescriptor::builtin(30);
            let mut expected = json!({
                "stickerId":30,"cateId":0,"type":7,"clientId":101,
                "imei":"synthetic","zsource":-1,"ttl":0
            });
            expected[address] = json!("22");
            assert_eq!(parameters(&target, sticker, 101, "synthetic"), Ok(expected));
            assert_eq!(
                url(
                    &Url::parse("https://chat.zalo.me/").unwrap(),
                    kind,
                    ClientType::PcLinux24
                )
                .unwrap()
                .path(),
                path
            );
            assert!(parameters(&target, StickerDescriptor::builtin(31), 101, "synthetic").is_err());
            let mut response =
                json!({"error_code":0,"data":{"msgId":"901","ts":"12345","clientId":101}});
            response["data"][address] = json!("22");
            assert_eq!(ack(&response, &target, 101).unwrap().server_id, "901");
            assert!(ack(&response, &target, 102).is_err());
            response["data"][address] = json!("23");
            assert!(ack(&response, &target, 101).is_err());
            assert!(ack(&json!({"error_code":0,"data":{}}), &target, 101).is_err());
        }
    }
}
