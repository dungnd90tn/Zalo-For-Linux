//! Readable text for message types that carry no media the client can show, read the way the original's
//! preview builder does (`pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js`,
//! around line 238850: `STR_VOICE_MSG`, `STR_LOC_MSG`, `STR_CARD_MSG`, `STR_UNDO_MSG`, …). Text only: no link,
//! image or action in the content is followed.
use crate::domain::MAX_TEXT_BYTES;
use serde_json::Value;

/// `ECARD_TYPE.BIRTHDAY` (PC 50847).
const ECARD_BIRTHDAY: &str = "1";

fn parsed(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::String(raw) => serde_json::from_str(raw).ok(),
        Value::Object(_) => value.cloned(),
        _ => None,
    }
}

fn text<'a>(content: &'a Value, key: &str) -> &'a str {
    content
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
}

fn bounded(value: &str) -> String {
    let mut result = String::new();
    for character in value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
    {
        if result.len() + character.len_utf8() > MAX_TEXT_BYTES {
            break;
        }
        result.push(character);
    }
    result
}

fn labeled(label: &str, detail: &str) -> String {
    if detail.is_empty() {
        label.to_owned()
    } else {
        bounded(&format!("{label} {detail}"))
    }
}

/// The text shown for `message_type`, or `None` when the type is not one of these (the caller keeps its
/// placeholder). `content` is the message's `content` field.
pub(crate) fn presentation(message_type: &str, content: &Value) -> Option<String> {
    Some(match message_type {
        "chat.ecard" => {
            let title = text(content, "title");
            let description = text(content, "description");
            let params = parsed(content.get("params")).unwrap_or(Value::Null);
            let mut result = String::new();
            for part in [title, description] {
                if !part.is_empty() {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(part);
                }
            }
            if result.is_empty() {
                result = labeled(
                    "[Thiệp]",
                    params
                        .get("previewTxt")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                );
            }
            bounded(&result)
        }
        "chat.gif" => "[GIF]".into(),
        "chat.voice" => "[Tin nhắn thoại]".into(),
        "chat.video.msg" => labeled("[Video]", text(content, "title")),
        "chat.location.new" => {
            let mut detail = text(content, "title");
            for key in ["desc", "description"] {
                if detail.is_empty() {
                    detail = text(content, key);
                }
            }
            labeled("[Vị trí]", detail)
        }
        "chat.doodle" => "[Hình vẽ tay]".into(),
        "chat.todo" => {
            let detail = match content {
                Value::String(message) => message.trim(),
                _ => {
                    let message = text(content, "message");
                    if message.is_empty() {
                        text(content, "title")
                    } else {
                        message
                    }
                }
            };
            labeled("[Công việc]", detail)
        }
        "chat.undo" => "Tin nhắn đã thu hồi".into(),
        "chat.delete.everyone" => "Tin nhắn đã bị xóa".into(),
        "chat.list.action" => {
            let header = content.as_array().and_then(|items| {
                items.iter().find_map(|item| {
                    matches!(
                        item.get("type").and_then(Value::as_str),
                        Some("l.a.header.only" | "l.a.header.full")
                    )
                    .then(|| text(item, "title"))
                    .filter(|title| !title.is_empty())
                })
            });
            bounded(header?)
        }
        _ => return None,
    })
}

/// The original never lets these reach the conversation list (`skipUpdateConvListByPreviewItem`, PC 256027,
/// and Official Account broadcasts grouped out of the list, `shouldShowOAInConvList`, PC 255945): they are
/// kept in the conversation's history but neither create nor reorder a conversation entry.
pub(crate) fn list_silent(message_type: &str, content: &Value) -> bool {
    match message_type {
        "chat.ecard" => {
            content.get("type").and_then(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_u64().map(|n| n.to_string()))
            }) == Some(ECARD_BIRTHDAY.to_owned())
        }
        // ZInstant templates are server-side Official Account content (broadcasts); a person cannot send one.
        "chat.webcontent" => parsed(content.get("params")).is_some_and(|params| {
            params
                .pointer("/item/zinstantdata_id")
                .or_else(|| params.pointer("/pcItem/zinstantdata_id"))
                .is_some()
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn readable_text_follows_the_original_preview_strings() {
        let cases = [
            (
                "chat.ecard",
                json!({"title":"Hôm nay là sinh nhật của An","description":"Gửi lời chúc nhé","type":"1"}),
                "Hôm nay là sinh nhật của An\nGửi lời chúc nhé",
            ),
            (
                "chat.ecard",
                json!({"params":"{\"previewTxt\":\"Bạn bè mới\"}"}),
                "[Thiệp] Bạn bè mới",
            ),
            ("chat.ecard", json!({}), "[Thiệp]"),
            (
                "chat.gif",
                json!({"href":"https://example.test/a.gif"}),
                "[GIF]",
            ),
            ("chat.voice", json!({}), "[Tin nhắn thoại]"),
            ("chat.video.msg", json!({"title":"clip"}), "[Video] clip"),
            ("chat.video.msg", json!({}), "[Video]"),
            (
                "chat.location.new",
                json!({"desc":"Hồ Gươm"}),
                "[Vị trí] Hồ Gươm",
            ),
            ("chat.doodle", json!({}), "[Hình vẽ tay]"),
            (
                "chat.todo",
                json!({"message":"Nộp báo cáo"}),
                "[Công việc] Nộp báo cáo",
            ),
            ("chat.todo", json!("Gọi lại"), "[Công việc] Gọi lại"),
            (
                "chat.undo",
                json!({"globalMsgId":"1"}),
                "Tin nhắn đã thu hồi",
            ),
            ("chat.delete.everyone", json!({}), "Tin nhắn đã bị xóa"),
            (
                "chat.list.action",
                json!([{"type":"l.a.body"},{"type":"l.a.header.full","title":"Đơn hàng"}]),
                "Đơn hàng",
            ),
        ];
        for (kind, content, expected) in cases {
            assert_eq!(
                presentation(kind, &content).as_deref(),
                Some(expected),
                "{kind}"
            );
        }
        assert_eq!(presentation("chat.list.action", &json!([])), None);
        assert_eq!(presentation("chat.unknown", &json!({})), None);
        let long = "x".repeat(MAX_TEXT_BYTES * 2);
        assert!(
            presentation("chat.ecard", &json!({"title": long}))
                .unwrap()
                .len()
                <= MAX_TEXT_BYTES
        );
        assert!(!presentation("chat.ecard", &json!({"title":"a\u{0007}b"}))
            .unwrap()
            .contains('\u{0007}'));
    }

    #[test]
    fn birthday_cards_and_official_account_broadcasts_stay_out_of_the_list() {
        assert!(list_silent("chat.ecard", &json!({"type":"1"})));
        assert!(list_silent("chat.ecard", &json!({"type":1})));
        assert!(!list_silent("chat.ecard", &json!({"type":"2"})));
        assert!(!list_silent("chat.ecard", &json!({})));
        assert!(list_silent(
            "chat.webcontent",
            &json!({"params":"{\"item\":{\"zinstantdata_id\":\"templateId=1\"}}"})
        ));
        assert!(!list_silent(
            "chat.webcontent",
            &json!({"title":"Link","params":"{}"})
        ));
        assert!(!list_silent("webchat", &json!("hello")));
    }
}
