//! Group poll transport and privacy-scoped detail projection.
use super::messaging::{self as wire, Target, ThreadKind, WireError};
use crate::domain::poll::{PollDetails, PollOption, MAX_POLL_ID, MAX_POLL_OPTIONS};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) const SERVICE: &str = "group_poll";
pub(crate) const PATH: &str = "/api/poll/detail";
pub(crate) const VOTE_PATH: &str = "/api/poll/vote";

pub(crate) fn parameters(poll_id: u64, imei: &str) -> Result<Value, WireError> {
    if poll_id == 0 || poll_id > MAX_POLL_ID || imei.is_empty() || imei.len() > 256 {
        return Err(WireError::Invalid);
    }
    Ok(json!({"poll_id": poll_id, "imei": imei}))
}

pub(crate) fn vote_parameters(
    poll_id: u64,
    option_id: u64,
    imei: &str,
) -> Result<Value, WireError> {
    let mut params = parameters(poll_id, imei)?;
    if !(1..=MAX_POLL_ID).contains(&option_id) {
        return Err(WireError::Invalid);
    }
    params["option_ids"] = json!([option_id]);
    Ok(params)
}

pub(crate) fn expiry(payload: &Value) -> Option<i64> {
    payload["data"]["expired_time"]
        .as_i64()
        .filter(|expiry| (0..=MAX_POLL_ID as i64).contains(expiry))
}

pub(crate) fn current(expiry: Option<i64>, now_ms: i64) -> bool {
    expiry.is_some_and(|expiry| expiry == 0 || expiry > now_ms)
}

fn exact_id(value: &Value) -> Result<u64, WireError> {
    wire::id_string(value)?
        .parse::<u64>()
        .ok()
        .filter(|id| *id <= MAX_POLL_ID)
        .ok_or(WireError::Invalid)
}

fn optional_flag(value: &Value, key: &str) -> Result<Option<bool>, WireError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        _ => Err(WireError::Invalid),
    }
}

pub(crate) fn detail(
    payload: &Value,
    target: &Target,
    poll_id: u64,
    now_ms: i64,
) -> Result<PollDetails, WireError> {
    if target.kind != ThreadKind::Group || poll_id == 0 || poll_id > MAX_POLL_ID {
        return Err(WireError::Invalid);
    }
    wire::service_code(payload)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    if exact_id(&data["poll_id"])? != poll_id {
        return Err(WireError::Invalid);
    }
    if let Some(group) = data.get("group_id").filter(|group| !group.is_null()) {
        if wire::id_string(group)? != target.remote_id {
            return Err(WireError::Invalid);
        }
    }

    let hidden = optional_flag(data, "is_hide_vote_preview")?;
    let joined = optional_flag(data, "joined")?;
    let show_votes = hidden == Some(false) || (hidden == Some(true) && joined == Some(true));
    let rows = data["options"].as_array().ok_or(WireError::Invalid)?;
    if !(2..=MAX_POLL_OPTIONS).contains(&rows.len()) {
        return Err(WireError::Invalid);
    }
    let mut ids = HashSet::with_capacity(rows.len());
    let mut options = Vec::with_capacity(rows.len());
    let mut valid_vote_flags = true;
    for row in rows {
        let option_id = exact_id(&row["option_id"])?;
        if !ids.insert(option_id) {
            return Err(WireError::Invalid);
        }
        let votes = match (show_votes, row.get("votes")) {
            (false, _) | (_, None | Some(Value::Null)) => None,
            (true, Some(count)) => Some(
                count
                    .as_u64()
                    .and_then(|count| u32::try_from(count).ok())
                    .ok_or(WireError::Invalid)?,
            ),
        };
        let voted = optional_flag(row, "voted");
        valid_vote_flags &= voted.is_ok();
        options.push(PollOption {
            option_id,
            text: row["content"].as_str().ok_or(WireError::Invalid)?.into(),
            votes,
            voted: voted == Ok(Some(true)),
        });
    }
    let closed = optional_flag(data, "closed")?.ok_or(WireError::Invalid)?;
    let can_vote = !closed
        && current(expiry(payload), now_ms)
        && joined == Some(false)
        && optional_flag(data, "allow_multi_choices") == Ok(Some(false))
        && valid_vote_flags
        && !options.iter().any(|option| option.voted);
    let detail = PollDetails {
        question: data["question"].as_str().ok_or(WireError::Invalid)?.into(),
        closed,
        anonymous: optional_flag(data, "is_anonymous")?.ok_or(WireError::Invalid)?,
        joined,
        can_vote,
        vote_pending: false,
        options,
    };
    detail.validate().map_err(|_| WireError::Invalid)?;
    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Value {
        json!({"error_code":0,"data":{"poll_id":91,"group_id":"22","question":"Đi ngày nào?",
            "closed":false,"is_anonymous":true,"is_hide_vote_preview":false,"joined":false,
            "options":[{"option_id":1,"content":"Thứ hai","votes":0,"voters":["private"]},
                {"option_id":"2","content":"Thứ sáu","votes":8}]}})
    }

    #[test]
    fn exact_scope_and_no_inferred_results_for_hidden_poll() {
        let group = Target::from_thread("g:22").unwrap();
        let mut response = payload();
        let shown = detail(&response, &group, 91, 100).unwrap();
        assert!(shown.anonymous);
        assert_eq!(shown.options[0].votes, Some(0));
        assert_eq!(shown.options[1].votes, Some(8));
        assert_eq!(detail(&response, &group, 92, 100), Err(WireError::Invalid));
        response["data"]["group_id"] = json!(23);
        assert_eq!(detail(&response, &group, 91, 100), Err(WireError::Invalid));
        response["data"]["group_id"] = Value::Null;
        for hide in [json!(true), Value::Null] {
            response["data"]["is_hide_vote_preview"] = hide;
            let hidden = detail(&response, &group, 91, 100).unwrap();
            assert!(hidden.options.iter().all(|option| option.votes.is_none()));
        }
        response["data"]["is_hide_vote_preview"] = json!(true);
        response["data"]["joined"] = json!(true);
        let joined = detail(&response, &group, 91, 100).unwrap();
        assert_eq!(joined.options, shown.options);
        assert_eq!(joined.joined, Some(true));
    }

    #[test]
    fn detail_rejects_duplicate_options_and_non_exact_fields() {
        let group = Target::from_thread("g:22").unwrap();
        let mut response = payload();
        response["data"]["options"][1]["option_id"] = json!("1");
        assert_eq!(detail(&response, &group, 91, 100), Err(WireError::Invalid));
        response["data"]["options"][1]["option_id"] = json!(2);
        response["data"]["options"][1]["votes"] = json!(-1);
        assert_eq!(detail(&response, &group, 91, 100), Err(WireError::Invalid));
        response["data"]["options"][1]["votes"] = Value::Null;
        assert!(detail(&response, &group, 91, 100).unwrap().options[1]
            .votes
            .is_none());
        response["data"]["closed"] = json!("false");
        assert_eq!(detail(&response, &group, 91, 100), Err(WireError::Invalid));
        assert_eq!(parameters(0, "imei"), Err(WireError::Invalid));
        assert_eq!(parameters(MAX_POLL_ID + 1, "imei"), Err(WireError::Invalid));
    }

    #[test]
    fn first_vote_requires_explicit_open_single_choice_and_unjoined_fields() {
        let group = Target::from_thread("g:22").unwrap();
        let mut response = payload();
        response["data"]["allow_multi_choices"] = json!(false);
        assert!(!detail(&response, &group, 91, 100).unwrap().can_vote);
        for expiry in [0, 101] {
            response["data"]["expired_time"] = json!(expiry);
            assert!(detail(&response, &group, 91, 100).unwrap().can_vote);
        }
        response["data"]["expired_time"] = json!(100);
        assert!(!detail(&response, &group, 91, 100).unwrap().can_vote);
        response["data"]["expired_time"] = json!(0);
        response["data"]["options"][0]["voted"] = json!(true);
        let detail = detail(&response, &group, 91, 100).unwrap();
        assert!(!detail.can_vote);
        assert!(detail.options[0].voted);
        assert_eq!(
            vote_parameters(91, 2, "imei"),
            Ok(json!({"poll_id":91,"option_ids":[2],"imei":"imei"}))
        );
        assert_eq!(vote_parameters(91, 0, "imei"), Err(WireError::Invalid));
    }
}
