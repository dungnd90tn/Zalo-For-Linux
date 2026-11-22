//! Exact text spans selected for a member or the verified group broadcast.
use super::reply::valid_source_id;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub const MAX_MENTIONS: usize = 32;
pub const ALL_ID: &str = "-1";
pub const ALL_LABEL: &str = "@All";
#[cfg(feature = "messaging")]
pub(crate) const MAX_SEND_UNITS: usize = 3_000;
const MAX_LABEL_UNITS: u32 = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mention {
    pub uid: String,
    pub pos: u32,
    pub len: u32,
}

impl Mention {
    pub fn is_all(&self) -> bool {
        self.uid == ALL_ID
    }
}

/// The PC editor uses JS string indices. Convert valid UTF-16 code-unit
/// boundaries into UTF-8 ranges in one pass so rendering never splits a scalar.
pub fn byte_ranges(text: &str, mentions: &[Mention]) -> Result<Vec<Range<usize>>> {
    ensure!(mentions.len() <= MAX_MENTIONS, "Too many mentions");
    let mut characters = text.char_indices().peekable();
    let mut units = 0;
    let mut byte_at = |position: u32| -> Option<usize> {
        while units < position {
            units += characters.next()?.1.len_utf16() as u32;
        }
        (units == position).then(|| characters.peek().map_or(text.len(), |(byte, _)| *byte))
    };
    let mut spans = Vec::with_capacity(mentions.len());
    let mut has_all = false;
    for mention in mentions {
        ensure!(
            (mention.is_all() || valid_source_id(&mention.uid))
                && (2..=MAX_LABEL_UNITS).contains(&mention.len),
            "Invalid mention member or length"
        );
        let start = byte_at(mention.pos).ok_or_else(|| anyhow::anyhow!("Invalid mention start"))?;
        let end = mention
            .pos
            .checked_add(mention.len)
            .and_then(&mut byte_at)
            .ok_or_else(|| anyhow::anyhow!("Invalid mention end"))?;
        let label = &text[start..end];
        if mention.is_all() {
            ensure!(
                !has_all && label == ALL_LABEL,
                "Invalid group mention label"
            );
            has_all = true;
        } else {
            ensure!(
                label.starts_with('@')
                    && label.trim() == label
                    && !label.chars().any(char::is_control),
                "Invalid mention label"
            );
        }
        spans.push(start..end);
    }
    Ok(spans)
}

pub fn validate(text: &str, mentions: &[Mention]) -> Result<()> {
    byte_ranges(text, mentions).map(|_| ())
}

#[cfg(feature = "messaging")]
pub(crate) fn validate_outgoing(text: &str, mentions: &[Mention]) -> Result<()> {
    ensure!(
        !mentions.is_empty()
            && text.len() <= super::MAX_TEXT_BYTES
            && text.encode_utf16().count() <= MAX_SEND_UNITS
            && text == super::canonical_plain_text(text),
        "Invalid outgoing mention text"
    );
    validate(text, mentions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_identify_exact_scalar_boundaries_and_reject_overlap() {
        let text = "😀 @Ánh @Bình";
        let first = Mention {
            uid: "22".into(),
            pos: 3,
            len: 4,
        };
        let second = Mention {
            uid: "33".into(),
            pos: 8,
            len: 5,
        };
        let spans = byte_ranges(text, &[first.clone(), second.clone()]).unwrap();
        assert_eq!(
            (&text[spans[0].clone()], &text[spans[1].clone()]),
            ("@Ánh", "@Bình")
        );
        let mut split_surrogate = first.clone();
        split_surrogate.pos = 1;
        assert!(validate(text, &[split_surrogate]).is_err());
        assert!(validate(text, &[second, first]).is_err());
    }

    #[test]
    fn group_broadcast_requires_one_exact_case_sensitive_typed_span() {
        let text = "😀 @All @All";
        let all = Mention {
            uid: ALL_ID.into(),
            pos: 3,
            len: 4,
        };
        let named_all = Mention {
            uid: "22".into(),
            pos: 8,
            len: 4,
        };
        assert!(validate(text, &[]).is_ok());
        assert!(validate(text, &[all.clone(), named_all.clone()]).is_ok());
        assert!(validate("😀 @all", std::slice::from_ref(&all)).is_err());
        assert!(validate(
            text,
            &[
                all,
                Mention {
                    uid: ALL_ID.into(),
                    ..named_all
                }
            ]
        )
        .is_err());
    }
}
