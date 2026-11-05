//! Readable text for the original `webchat` / `msginfo.actionlist` format.
//! The complete content is retained separately; this projection never executes
//! actions, interprets HTML, or loads the links/images supplied by a message.
use crate::domain::MAX_TEXT_BYTES;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const FALLBACK: &str = "Thông báo hệ thống";
const MAX_PRESENTATION_HIGHLIGHTS: usize = 1024;

pub(crate) fn presentation(account: &str, content: &Value) -> String {
    let params = content
        .get("params")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    if let Some(params) = params {
        if let Some(template) = params.pointer("/msg/vi").and_then(Value::as_str) {
            if !template.is_empty() {
                let highlights = params.get("highLightsV2").and_then(Value::as_array);
                return substitute(account, template, highlights.map(Vec::as_slice));
            }
        }
    }
    let title = content.get("title").and_then(Value::as_str);
    let mut result = String::new();
    append(
        &mut result,
        title.filter(|title| !title.is_empty()).unwrap_or(FALLBACK),
    );
    result
}

fn substitute(account: &str, template: &str, highlights: Option<&[Value]>) -> String {
    let mut result = String::new();
    let template = bounded(template);
    let mut consumed = 0;
    let mut markers: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    // Index the bounded template once. An array of missing highlights must not
    // repeatedly scan a long template (the original find-per-item is quadratic).
    for (position, _) in template.match_indices('%') {
        let tail = &template[position + 1..];
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || tail.starts_with('0') || !tail[digits..].starts_with("$s") {
            continue;
        }
        if let Ok(index) = tail[..digits].parse::<usize>() {
            markers
                .entry(index)
                .or_default()
                .push((position, position + digits + 3));
        }
    }
    // The original walks highlights in index order and consumes one matching
    // token at a time. No global replacement or template/code evaluation.
    for (index, value) in highlights
        .unwrap_or_default()
        .iter()
        .take(MAX_PRESENTATION_HIGHLIGHTS)
        .enumerate()
    {
        if result.len() >= MAX_TEXT_BYTES || consumed == template.len() {
            break;
        }
        let Some((position, end)) = markers
            .get(&(index + 1))
            .and_then(|positions| positions.iter().find(|(start, _)| *start >= consumed))
            .copied()
        else {
            continue;
        };
        let Some(replacement) = highlight(account, value) else {
            continue;
        };
        if !append(&mut result, &template[consumed..position]) || !append(&mut result, &replacement)
        {
            return result;
        }
        consumed = end;
    }
    append(&mut result, &template[consumed..]);
    result
}

fn highlight(account: &str, value: &Value) -> Option<String> {
    let kind = value.get("type")?;
    // The bundle's comparison accepts numeric/string type tags. Other values
    // stay untouched in the full source data and are not coerced to text.
    let kind = kind.as_u64().or_else(|| match kind.as_str()? {
        "0" => Some(0),
        "1" => Some(1),
        _ => None,
    })?;
    let label = || {
        value
            .get("dpn")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    match kind {
        0 => {
            let is_self = value.get("uid").is_some_and(|uid| {
                uid.as_str() == Some(account)
                    || uid.as_u64().is_some_and(|uid| uid.to_string() == account)
            });
            if is_self {
                Some("Bạn".into())
            } else {
                label().map(bounded)
            }
        }
        // Persisted text must survive replay after timezone or date changes.
        // Prefer the supplied display label; otherwise render a fixed UTC time.
        1 => label().map(bounded).or_else(|| {
            value
                .get("ts")
                .and_then(Value::as_i64)
                .filter(|timestamp| *timestamp > 0)
                .and_then(utc_timestamp)
        }),
        _ => None,
    }
}

fn utc_timestamp(milliseconds: i64) -> Option<String> {
    let seconds: libc::time_t = milliseconds / 1000;
    let mut output = std::mem::MaybeUninit::<libc::tm>::uninit();
    // Owned output and a fixed timezone keep replay independent of process TZ.
    if unsafe { libc::gmtime_r(&seconds, output.as_mut_ptr()) }.is_null() {
        return None;
    }
    let parts = unsafe { output.assume_init() };
    Some(format!(
        "{}-{:02}-{:02} {:02}:{:02} UTC",
        i64::from(parts.tm_year) + 1900,
        parts.tm_mon + 1,
        parts.tm_mday,
        parts.tm_hour,
        parts.tm_min
    ))
}

fn bounded(text: &str) -> String {
    let mut result = String::new();
    append(&mut result, text);
    result
}

// Keep the persisted/rendered text inside the existing message text budget.
// Truncation affects only this projection; the complete content is durable.
fn append(output: &mut String, text: &str) -> bool {
    let available = MAX_TEXT_BYTES - output.len();
    if text.len() <= available {
        output.push_str(text);
        return true;
    }
    if available < '…'.len_utf8() {
        return false;
    }
    let mut take = available - '…'.len_utf8();
    while !text.is_char_boundary(take) {
        take -= 1;
    }
    output.push_str(&text[..take]);
    output.push('…');
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_preview_fixtures_match_the_original_template_projection() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/info.json")).unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            assert_eq!(
                presentation("11", &case["content"]),
                case["text"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn projection_is_bounded_unicode_and_never_executes_peer_data() {
        let content = json!({"action":"msginfo.actionlist","title":"ế".repeat(MAX_TEXT_BYTES),
            "params":"not-json","actions":[{"actionData":"javascript:private_action"}]});
        let text = presentation("11", &content);
        assert!(text.len() <= MAX_TEXT_BYTES);
        assert!(text.ends_with('…'));
        assert!(!text.contains("private_action"));
        let content = json!({"action":"msginfo.actionlist","params":json!({
            "msg":{"vi":"%1$s <b>đã thay đổi nhóm</b>"},
            "highLightsV2":[{"type":0,"uid":"22","dpn":"<script>literal</script>"}]
        }).to_string()});
        assert_eq!(
            presentation("11", &content),
            "<script>literal</script> <b>đã thay đổi nhóm</b>"
        );
        assert_eq!(
            presentation("11", &json!({"action":"msginfo.actionlist"})),
            FALLBACK
        );
    }

    #[test]
    fn timestamp_projection_is_fixed_utc_or_provided_label_for_immutable_replay() {
        let content = |highlight| {
            json!({"action":"msginfo.actionlist","params":json!({
            "msg":{"vi":"Thời gian: %1$s"},"highLightsV2":[highlight]
        }).to_string()})
        };
        assert_eq!(
            presentation("11", &content(json!({"type":1,"ts":1700000000000_i64}))),
            "Thời gian: 2023-11-14 22:13 UTC"
        );
        assert_eq!(
            presentation(
                "11",
                &content(json!({"type":1,"ts":1700000000000_i64,"dpn":"Ngày mai"}))
            ),
            "Thời gian: Ngày mai"
        );
    }

    #[test]
    fn adversarial_missing_markers_have_bounded_template_and_highlight_work() {
        let highlights =
            vec![json!({"type":0,"uid":"22","dpn":"Tên"}); MAX_PRESENTATION_HIGHLIGHTS + 10_000];
        assert_eq!(substitute("11", "%1025$s", Some(&highlights)), "%1025$s");
        let text = substitute("11", &"ế".repeat(MAX_TEXT_BYTES), Some(&highlights));
        assert!(text.len() <= MAX_TEXT_BYTES);
        assert!(text.ends_with('…'));
        assert_eq!(
            substitute("11", "%01$s rồi %1$s", Some(&highlights)),
            "%01$s rồi Tên"
        );
    }
}
