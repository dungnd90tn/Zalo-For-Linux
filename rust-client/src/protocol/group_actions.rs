//! Single-friend invitation and non-owner leave on the ordinary GROUP service.
use super::messaging::{id_string, numeric_id, service_code, Target, ThreadKind, WireError};
use serde_json::{json, Value};

pub(crate) const INVITE_PATH: &str = "/api/group/invite/v2";
pub(crate) const LEAVE_PATH: &str = "/api/group/leave";

pub(crate) fn invite_parameters(
    group: &Target,
    member: &str,
    imei: &str,
) -> Result<Value, WireError> {
    verify_group(group)?;
    numeric_id(member)?;
    Ok(json!({
        "grid":group.remote_id,
        "members":[member],
        "memberTypes":[-1],
        "imei":imei,
        "clientLang":"VN"
    }))
}

pub(crate) fn leave_parameters(group: &Target, imei: &str) -> Result<Value, WireError> {
    verify_group(group)?;
    Ok(json!({
        "grids":[group.remote_id],
        "imei":imei,
        "silent":0,
        "language":"VN"
    }))
}

fn verify_group(group: &Target) -> Result<(), WireError> {
    if group.kind != ThreadKind::Group {
        return Err(WireError::Unsupported);
    }
    numeric_id(&group.remote_id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InviteAck {
    Accepted,
    ApprovalPending,
    InvitationSent,
    AlreadyInvited,
    AlreadyMember,
    Rejected,
}

fn exact_member(entries: &Value, member: &str) -> Result<bool, WireError> {
    match entries.as_array().map(Vec::as_slice) {
        Some([]) => Ok(false),
        Some([id]) if id_string(id)?.as_str() == member => Ok(true),
        _ => Err(WireError::Invalid),
    }
}

pub(crate) fn invite_ack(payload: &Value, member: &str) -> Result<InviteAck, WireError> {
    numeric_id(member)?;
    service_code(payload)?;
    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .ok_or(WireError::Invalid)?;
    let listed_error = data
        .get("errorMembers")
        .map(|ids| exact_member(ids, member))
        .transpose()?;
    let mut result = None;
    if let Some(codes) = data.get("error_data").filter(|value| !value.is_null()) {
        let codes = codes
            .as_object()
            .filter(|codes| codes.len() <= 64)
            .ok_or(WireError::Invalid)?;
        for (code, ids) in codes {
            if !exact_member(ids, member)? {
                continue;
            }
            let outcome = match code.as_str() {
                "0" => InviteAck::Accepted,
                "240" | "248" | "252" => InviteAck::ApprovalPending,
                "263" => InviteAck::InvitationSent,
                "262" => InviteAck::AlreadyInvited,
                "178" => InviteAck::AlreadyMember,
                "122" | "166" | "172" | "173" | "175" | "177" | "183" | "188" | "244" | "245"
                | "246" | "247" | "257" | "264" | "271" | "280" | "281" | "283" | "306" | "317"
                | "-100" => InviteAck::Rejected,
                _ => return Err(WireError::Invalid),
            };
            if result.replace(outcome).is_some() {
                return Err(WireError::Invalid);
            }
        }
    }
    match (result, listed_error) {
        (Some(InviteAck::Accepted), Some(true)) | (None, Some(true) | None) => {
            Err(WireError::Invalid)
        }
        (Some(outcome), _) => Ok(outcome),
        // The original derives successful members by subtracting errorMembers
        // from the exact submitted list. This acknowledges processing only.
        (None, Some(false)) => Ok(InviteAck::Accepted),
    }
}

pub(crate) fn leave_ack(payload: &Value) -> Result<(), WireError> {
    // The PC client deletes cached membership after decoded service success.
    service_code(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_group_action_builders_match_single_friend_and_normal_leave() {
        let source: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/group_actions.json"
        ))
        .unwrap();
        let group = Target::new(ThreadKind::Group, source["group"].as_str().unwrap()).unwrap();
        let imei = source["device"].as_str().unwrap();
        assert_eq!(source["invite"]["path"], INVITE_PATH);
        assert_eq!(source["leave"]["path"], LEAVE_PATH);
        assert_eq!(
            invite_parameters(&group, source["friend"].as_str().unwrap(), imei).unwrap(),
            source["invite"]["payload"]
        );
        assert_eq!(
            leave_parameters(&group, imei).unwrap(),
            source["leave"]["payload"]
        );
    }

    #[test]
    fn invite_result_distinguishes_bound_success_approval_and_invitation() {
        for (code, expected) in [
            ("240", InviteAck::ApprovalPending),
            ("248", InviteAck::ApprovalPending),
            ("252", InviteAck::ApprovalPending),
            ("262", InviteAck::AlreadyInvited),
            ("263", InviteAck::InvitationSent),
            ("178", InviteAck::AlreadyMember),
            ("166", InviteAck::Rejected),
        ] {
            let result = json!({"error_code":0,"data":{
                "errorMembers":["22"], "error_data":{code:[22]}
            }});
            assert_eq!(invite_ack(&result, "22"), Ok(expected));
            assert!(invite_ack(&result, "23").is_err());
        }
        for data in [
            json!({"errorMembers":[]}),
            json!({"error_data":{"0":["22"]}}),
        ] {
            assert_eq!(
                invite_ack(&json!({"error_code":0,"data":data}), "22"),
                Ok(InviteAck::Accepted)
            );
        }
        for data in [
            json!({}),
            json!({"errorMembers":["22"]}),
            json!({"errorMembers":["22"],"error_data":{"0":["22"]}}),
            json!({"errorMembers":[],"error_data":{"0":["22"],"240":["22"]}}),
            json!({"errorMembers":[],"error_data":{"0":["22","44"]}}),
            json!({"errorMembers":[],"error_data":{"991":["22"]}}),
        ] {
            assert!(invite_ack(&json!({"error_code":0,"data":data}), "22").is_err());
        }
        assert!(leave_ack(&json!({"error_code":0,"data":null})).is_ok());
        assert!(leave_ack(&json!({"error_code":164})).is_err());
        assert!(leave_ack(&json!({"data":null})).is_err());
    }
}
