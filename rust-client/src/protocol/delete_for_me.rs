//! The bundled PC client's ordinary onlyMe request and receipt.
use super::messaging::{id_string, numeric_id, service_code, Target, ThreadKind, WireError};
use serde_json::{json, Value};

pub(crate) const DIRECT_PATH: &str = "/api/message/delete";
pub(crate) const GROUP_PATH: &str = "/api/group/deletemsg";

pub(crate) fn parameters(
    account: &str,
    target: &Target,
    action_id: u64,
    message_id: &str,
    sender: &str,
    source_client_id: &str,
    imei: &str,
) -> Result<Value, WireError> {
    for value in [
        account,
        &target.remote_id,
        message_id,
        sender,
        source_client_id,
    ] {
        numeric_id(value)?;
    }
    if !(1..=9_007_199_254_740_991).contains(&action_id) {
        return Err(WireError::Invalid);
    }
    let destination = match target.kind {
        ThreadKind::Direct if target.remote_id != account && !imei.is_empty() => {
            if sender == account {
                target.remote_id.as_str()
            } else if sender == target.remote_id {
                account
            } else {
                return Err(WireError::Invalid);
            }
        }
        ThreadKind::Group => &target.remote_id,
        ThreadKind::Direct => return Err(WireError::Invalid),
    };
    // compact-app-pc 269915–269927, 472907–472928, 491041–491075 and
    // 633906–633945. The action ID is numeric and separate from the source ID.
    let mut parameters = json!({
        "cliMsgId": action_id,
        "msgs": [{
            "cliMsgId": source_client_id,
            "globalMsgId": message_id,
            "ownerId": sender,
            "destId": destination
        }],
        "onlyMe": 1
    });
    match target.kind {
        ThreadKind::Direct => {
            parameters["toid"] = json!(target.remote_id);
            parameters["imei"] = json!(imei);
        }
        ThreadKind::Group => {
            parameters["grid"] = json!(target.remote_id);
        }
    }
    Ok(parameters)
}

// Accept the request-bound receipt without requiring IDs absent from the original reply.
pub(crate) fn ack(payload: &Value, target: &Target, action_id: u64) -> Result<(), WireError> {
    service_code(payload)?;
    for candidate in [Some(payload), payload.get("data")].into_iter().flatten() {
        for key in ["cliMsgId", "clientId"] {
            if let Some(value) = candidate.get(key) {
                if id_string(value)? != action_id.to_string() {
                    return Err(WireError::Invalid);
                }
            }
        }
        for (key, kind) in [("toid", ThreadKind::Direct), ("grid", ThreadKind::Group)] {
            if let Some(value) = candidate.get(key) {
                if kind != target.kind || id_string(value)? != target.remote_id {
                    return Err(WireError::Invalid);
                }
            }
        }
        if candidate
            .get("onlyMe")
            .is_some_and(|value| value.as_u64() != Some(1))
        {
            return Err(WireError::Invalid);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_for_me_requests_match_source_destination_and_only_me() {
        let direct = Target::from_thread("d:22").unwrap();
        let group = Target::from_thread("g:33").unwrap();
        let build = |target, sender| parameters("11", target, 500, "70", sender, "18", "fixture");
        assert_eq!(
            build(&direct, "11").unwrap(),
            json!({"toid":"22","imei":"fixture","cliMsgId":500,"onlyMe":1,"msgs":[
                {"cliMsgId":"18","globalMsgId":"70","ownerId":"11","destId":"22"}
            ]})
        );
        assert_eq!(
            build(&direct, "22").unwrap(),
            json!({"toid":"22","imei":"fixture","cliMsgId":500,"onlyMe":1,"msgs":[
                {"cliMsgId":"18","globalMsgId":"70","ownerId":"22","destId":"11"}
            ]})
        );
        assert_eq!(
            build(&group, "44").unwrap(),
            json!({"grid":"33","cliMsgId":500,"onlyMe":1,"msgs":[
                {"cliMsgId":"18","globalMsgId":"70","ownerId":"44","destId":"33"}
            ]})
        );
        assert!(build(&direct, "44").is_err());
        assert!(build(&Target::from_thread("d:11").unwrap(), "11").is_err());
    }

    #[test]
    fn delete_for_me_receipt_cannot_confirm_another_scope_or_everyone() {
        let target = Target::from_thread("g:33").unwrap();
        assert!(ack(&json!({"error_code":0}), &target, 500).is_ok());
        assert!(ack(
            &json!({"error_code":0,"data":{"clientId":"500","grid":"33","onlyMe":1}}),
            &target,
            500
        )
        .is_ok());
        for payload in [
            json!({"error_code":0,"data":{"clientId":"501"}}),
            json!({"error_code":0,"data":{"toid":"33"}}),
            json!({"error_code":0,"data":{"grid":"22"}}),
            json!({"error_code":0,"data":{"onlyMe":0}}),
            json!({"error_code":211}),
        ] {
            assert!(ack(&payload, &target, 500).is_err());
        }
    }
}
