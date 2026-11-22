//! Bounded poll details and the local state of a first vote.
use anyhow::{ensure, Result};
use serde_json::Value;

pub const MAX_POLL_OPTIONS: usize = 30;
pub const MAX_POLL_QUESTION_BYTES: usize = 1024;
pub const MAX_POLL_OPTION_BYTES: usize = 512;
pub const MAX_POLL_ID: u64 = 9_007_199_254_740_991;
// A notice remains displayable as a summary when its source exceeds this lookup limit.
pub(crate) const MAX_POLL_NOTICE_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollDetails {
    pub question: String,
    pub options: Vec<PollOption>,
    pub closed: bool,
    pub anonymous: bool,
    pub can_vote: bool,
    pub joined: Option<bool>,
    pub vote_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollOption {
    pub option_id: u64,
    pub text: String,
    pub votes: Option<u32>,
    pub voted: bool,
}

#[cfg(feature = "messaging")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollVoteState {
    Pending,
    Joined,
}

#[cfg(feature = "messaging")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollVoteSettlement {
    /// The request did not leave the client or an exact rejection proves no vote was cast.
    NotDispatched,
    Joined,
}

impl PollDetails {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_text(&self.question, MAX_POLL_QUESTION_BYTES)
                && (2..=MAX_POLL_OPTIONS).contains(&self.options.len())
                && self.options.iter().all(|option| {
                    (1..=MAX_POLL_ID).contains(&option.option_id)
                        && valid_text(&option.text, MAX_POLL_OPTION_BYTES)
                })
                && (!self.can_vote
                    || (!self.closed
                        && self.joined == Some(false)
                        && !self.vote_pending
                        && self.options.iter().all(|option| !option.voted))),
            "Invalid poll details"
        );
        Ok(())
    }
}

fn valid_text(text: &str, max: usize) -> bool {
    text.len() <= max
        && !text.trim().is_empty()
        && !text
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
}

pub(crate) fn notice_id(params: &Value) -> Option<u64> {
    let value = params.get("pollId")?;
    let id = match value {
        Value::Number(id) => id.as_u64(),
        Value::String(id)
            if !id.starts_with('0') && id.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            id.parse().ok()
        }
        _ => None,
    }?;
    (1..=MAX_POLL_ID).contains(&id).then_some(id)
}
