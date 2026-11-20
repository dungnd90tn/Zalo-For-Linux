//! Local annotation ranges are byte-based until a message is submitted.
use super::Composer;
use crate::domain::{
    canonical_plain_text,
    mention::{self, Mention},
    MAX_TEXT_BYTES,
};
use anyhow::{ensure, Result};
use std::ops::Range;

pub(super) struct Span {
    bytes: Range<usize>,
    token: String,
    uid: String,
}

impl Composer {
    pub fn has_mentions(&self) -> bool {
        !self.mentions.is_empty()
    }

    pub fn apply_mention(
        &mut self,
        replace: Range<usize>,
        display_name: &str,
        uid: &str,
    ) -> Result<usize> {
        crate::protocol::messaging::numeric_id(uid)?;
        ensure!(
            self.reply().is_none() && self.attachment().is_none() && !self.has_photo_intent(),
            "Bỏ trích dẫn hoặc tệp đính kèm trước khi nhắc tên."
        );
        ensure!(
            self.text.get(replace.clone()).is_some()
                && !display_name.is_empty()
                && display_name == display_name.trim()
                && display_name.chars().count() <= 100
                && !display_name.chars().any(char::is_control),
            "Không thể chèn tên thành viên vào bản nháp."
        );
        let token = format!("@{display_name}");
        let token_end = replace.start + token.len();
        let append_space = !self.text[replace.end..].starts_with(char::is_whitespace);
        let new_end = token_end + usize::from(append_space);
        ensure!(
            self.text.len() - replace.len() + token.len() + usize::from(append_space)
                <= MAX_TEXT_BYTES,
            "Bản nháp vượt giới hạn dung lượng."
        );
        let remaining = self
            .mentions
            .iter()
            .filter(|span| span.bytes.end <= replace.start || span.bytes.start >= replace.end)
            .count();
        ensure!(
            remaining < mention::MAX_MENTIONS,
            "Đã đến giới hạn số lượt nhắc tên."
        );
        let insertion = if append_space {
            format!("{token} ")
        } else {
            token.clone()
        };
        self.text.replace_range(replace.clone(), &insertion);
        let start = replace.start;
        self.rebase_mentions(replace, new_end);
        let index = self
            .mentions
            .partition_point(|span| span.bytes.start < start);
        self.mentions.insert(
            index,
            Span {
                bytes: start..token_end,
                token,
                uid: uid.to_owned(),
            },
        );
        self.edited();
        Ok(new_end)
    }

    pub fn edit_with_mentions(&mut self, previous_text: &str) {
        if !self.mentions.is_empty() && previous_text != self.text {
            let prefix = equal_bytes(previous_text.chars(), self.text.chars());
            let suffix = equal_bytes(previous_text.chars().rev(), self.text.chars().rev());
            // Repeated equal text can give both ends a plausible claim on the
            // same bytes. Drop annotations in that ambiguous part rather
            // than moving a member binding onto a manually typed token.
            let shorter = previous_text.len().min(self.text.len());
            let before = prefix.min(shorter.saturating_sub(suffix));
            let after = suffix.min(shorter.saturating_sub(prefix));
            self.rebase_mentions(before..previous_text.len() - after, self.text.len() - after);
        }
        self.edited();
    }

    fn rebase_mentions(&mut self, old: Range<usize>, new_end: usize) {
        self.mentions.retain_mut(|span| {
            if span.bytes.end <= old.start {
                true
            } else if span.bytes.start >= old.end {
                span.bytes =
                    (span.bytes.start - old.end + new_end)..(span.bytes.end - old.end + new_end);
                true
            } else {
                false
            }
        });
    }

    pub(super) fn wire_mentions(&self) -> Result<Vec<Mention>> {
        let canonical = canonical_plain_text(&self.text);
        let start = self.text.find(canonical).unwrap_or_default();
        let mut mentions = Vec::with_capacity(self.mentions.len());
        for span in &self.mentions {
            ensure!(
                span.bytes.start >= start
                    && span.bytes.end <= start + canonical.len()
                    && self.text.get(span.bytes.clone()) == Some(span.token.as_str()),
                "Đoạn nhắc tên đã thay đổi."
            );
            mentions.push(Mention {
                uid: span.uid.clone(),
                pos: u32::try_from(self.text[start..span.bytes.start].encode_utf16().count())?,
                len: u32::try_from(span.token.encode_utf16().count())?,
            });
        }
        mention::validate_outgoing(canonical, &mentions)?;
        Ok(mentions)
    }

    pub(super) fn clear_mentions(&mut self) {
        self.mentions.clear();
    }
}

fn equal_bytes(left: impl Iterator<Item = char>, right: impl Iterator<Item = char>) -> usize {
    left.zip(right)
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_draft_positions_follow_canonical_utf16_after_edits() {
        let mut draft = Composer {
            text: "  👋 @a nhé ".into(),
            ..Default::default()
        };
        let at = draft.text.find('@').unwrap();
        draft.apply_mention(at..at + 2, "Ánh 😀", "22").unwrap();
        assert_eq!(draft.text, "  👋 @Ánh 😀 nhé ");
        assert_eq!(
            draft.wire_mentions().unwrap(),
            [Mention {
                uid: "22".into(),
                pos: 3,
                len: 7
            }]
        );
        let previous = draft.text.clone();
        draft.text.insert_str(2, "a ");
        draft.edit_with_mentions(&previous);
        assert_eq!(draft.wire_mentions().unwrap()[0].pos, 5);
        let previous = draft.text.clone();
        let at = draft.text.find('Á').unwrap();
        draft.text.replace_range(at..at + 'Á'.len_utf8(), "A");
        draft.edit_with_mentions(&previous);
        assert!(!draft.has_mentions());
    }

    #[test]
    fn ambiguous_identical_mentions_never_inherit_the_wrong_member() {
        let mut draft = Composer::default();
        let end = draft.apply_mention(0..0, "An", "22").unwrap();
        draft.apply_mention(end..end, "An", "33").unwrap();
        let previous = draft.text.clone();
        draft.text = "@An ".into();
        draft.edit_with_mentions(&previous);
        assert!(!draft.has_mentions());
    }
}
