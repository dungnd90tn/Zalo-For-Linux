//! Decodes a `frames.jsonl` written by `reverse-engineering/tools/electron_cdp_capture.mjs` and prints
//! the signal (E2EE) conversation of the original client as shapes: per request the command, the flags it
//! carried, the service's answer code and the reply's structure. Values are hidden except a short allowlist
//! of status fields; user ids are replaced by `uid#N` labels and keys, ciphertext and ids never appear.
//!
//!   cargo run --features live --example decode_ws_capture -- <frames.jsonl> [--all]
//!
//! `--all` also lists every other command (header only).
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use std::collections::BTreeMap;
use zalo_rust::protocol::messaging::FrameDecoder;

const VALUES: &[&str] = &[
    "error_code",
    "err",
    "deviceId",
    "platformType",
    "isReInit",
    "msgInfoNoti",
    "isAutoUpgrade",
    "type",
    "delSS",
    "hasMore",
    "first",
    "reqIid",
    "di",
];
/// The end-to-end encrypted command family: signal commands plus the encrypted queues and their acks.
fn is_signal(cmd: u16) -> bool {
    (540..=564).contains(&cmd) || matches!(cmd, 513 | 516..=519)
}

fn shape(
    value: &Value,
    path: &str,
    name: &str,
    labels: &mut BTreeMap<String, usize>,
    out: &mut Vec<String>,
) {
    if out.len() > 80 {
        return;
    }
    let here = if path.is_empty() { "$" } else { path };
    match value {
        Value::Null => out.push(format!("{here}: null")),
        Value::Bool(b) => out.push(if VALUES.contains(&name) {
            format!("{here}: {b}")
        } else {
            format!("{here}: bool")
        }),
        Value::Number(n) => out.push(if VALUES.contains(&name) {
            format!("{here}: {n}")
        } else {
            format!("{here}: number")
        }),
        Value::String(s) => {
            if name == "uid" || name == "uidTo" || name == "uidFrom" {
                let next = labels.len() + 1;
                let label = *labels.entry(s.clone()).or_insert(next);
                out.push(format!("{here}: uid#{label}"));
            } else {
                out.push(format!("{here}: string({})", s.len()));
            }
        }
        Value::Array(items) => {
            out.push(format!("{here}: array({})", items.len()));
            for (i, item) in items.iter().take(4).enumerate() {
                shape(item, &format!("{path}[{i}]"), name, labels, out);
            }
        }
        Value::Object(map) => {
            out.push(format!("{here}: object({})", map.len()));
            for (key, item) in map {
                let key_clean: String = key
                    .chars()
                    .take(40)
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                let next = if path.is_empty() {
                    key_clean.clone()
                } else {
                    format!("{path}.{key_clean}")
                };
                shape(item, &next, &key_clean, labels, out);
            }
        }
    }
}

/// How an encrypted message record is laid out: which field carries the ciphertext, its framing (wire type byte
/// then the 0x33 version byte) and the extension string, with ids anonymized and no ciphertext printed.
fn describe_message(message: &Value, labels: &mut BTreeMap<String, usize>) -> String {
    let text = |name: &str| message.get(name).and_then(Value::as_str);
    let mut label = |uid: &str| {
        let next = labels.len() + 1;
        format!("uid#{}", *labels.entry(uid.to_owned()).or_insert(next))
    };
    let from = text("uidFrom").map(&mut label).unwrap_or_default();
    let content = text("content").unwrap_or("");
    let framing = match STANDARD.decode(content) {
        Ok(raw) if raw.len() > 10 && raw[1] == 0x33 && matches!(raw[0], 1 | 3) => {
            format!("cipher type {} ({} bytes)", raw[0], raw.len())
        }
        Ok(raw) => format!(
            "base64 but not type||0x33 framing ({} bytes, first bytes {:02x?})",
            raw.len(),
            &raw[..raw.len().min(3)]
        ),
        Err(_) => format!("not base64 (string of {})", content.len()),
    };
    let ext = text("e2ee")
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .map(|value| {
            value
                .as_object()
                .map(|map| {
                    map.iter()
                        .map(|(k, v)| {
                            if v.is_number() || v.is_boolean() {
                                format!("{k}={v}")
                            } else {
                                format!("{k}:{}", if v.is_string() { "string" } else { "value" })
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default()
        })
        .unwrap_or_else(|| "none".into());
    let platform = message
        .pointer("/paramsExt/platformType")
        .map(|v| v.to_string())
        .unwrap_or_default();
    format!(
        "from {from} platform {platform} msgType {} | {framing} | e2ee ext [{ext}]",
        text("msgType").unwrap_or("?")
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .expect("usage: decode_ws_capture <frames.jsonl> [--all]");
    let rest: Vec<String> = args.collect();
    let all = rest.iter().any(|a| a == "--all");
    // Local debugging aid only: raw ack bookkeeping ids (never message content or ciphertext).
    let ids = rest.iter().any(|a| a == "--ids");
    let text = std::fs::read_to_string(&file).expect("cannot read the capture");
    let mut decoder = FrameDecoder::default();
    let mut labels = BTreeMap::new();
    let mut start = None;
    let mut connection = 0usize;
    // request id -> (time, cmd) for the answer line
    let mut sent: BTreeMap<String, (i64, u16)> = BTreeMap::new();
    for line in text.lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        let t = record["t"].as_i64().unwrap_or(0);
        let origin = *start.get_or_insert(t);
        let when = format!("+{:>6.2}s", (t - origin) as f64 / 1000.0);
        match record["dir"].as_str().unwrap_or("") {
            "open" => {
                connection += 1;
                decoder = FrameDecoder::default();
                println!("{when} == connection {connection} opened ==");
                continue;
            }
            "closed" => {
                println!("{when} == connection {connection} closed ==");
                continue;
            }
            _ => {}
        }
        if record["opcode"].as_i64() != Some(2) {
            continue;
        }
        let Ok(bytes) = STANDARD.decode(record["data"].as_str().unwrap_or("")) else {
            continue;
        };
        if bytes.len() < 4 {
            continue;
        }
        let cmd = u16::from_le_bytes([bytes[1], bytes[2]]);
        let sub = bytes[3];
        let outgoing = record["dir"] == "out";
        let interesting = is_signal(cmd);
        if !outgoing {
            // Server frame: decode through the production decoder (it keeps the session key from cmd 1).
            let outer: Value = serde_json::from_slice(&bytes[4..]).unwrap_or(Value::Null);
            match decoder.decode_detailed(&bytes) {
                Ok(frame) if interesting || all => {
                    let id = frame
                        .payload
                        .pointer("/data/reqId")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let latency = sent
                        .get(&id)
                        .map(|(t0, _)| format!(" ({} ms)", t - t0))
                        .unwrap_or_default();
                    let mut out = Vec::new();
                    shape(&frame.payload, "", "", &mut labels, &mut out);
                    println!("{when} <- {cmd}/{sub}{latency}");
                    if ids && matches!(cmd, 551 | 553) {
                        for key in ["msgs", "delivereds"] {
                            for message in frame
                                .payload
                                .pointer(&format!("/data/{key}"))
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                            {
                                let pick: serde_json::Map<String, Value> = [
                                    "msgId",
                                    "cliMsgId",
                                    "actionId",
                                    "realMsgId",
                                    "uidFrom",
                                    "idTo",
                                    "ts",
                                    "cmd",
                                    "st",
                                    "status",
                                    "threadId",
                                    "deviceId",
                                ]
                                .iter()
                                .filter_map(|k| message.get(*k).map(|v| (k.to_string(), v.clone())))
                                .collect();
                                println!("        {key} ids (raw): {}", Value::Object(pick));
                            }
                        }
                        println!(
                            "        lastActionId (raw): {}",
                            frame
                                .payload
                                .pointer("/data/lastActionId")
                                .map(|v| v.to_string())
                                .unwrap_or_default()
                        );
                    }
                    if let Some(messages) = frame
                        .payload
                        .pointer("/data/msgs")
                        .and_then(Value::as_array)
                    {
                        for message in messages {
                            println!(
                                "        message: {}",
                                describe_message(message, &mut labels)
                            );
                        }
                    }
                    if interesting {
                        for l in out.iter().take(60) {
                            println!("        {l}");
                        }
                    }
                }
                Ok(_) => {}
                Err(failure) => {
                    if interesting || all {
                        println!(
                            "{when} <- {cmd}/{sub} REJECTED by the decoder ({:?}); outer error_code={}",
                            failure.reason,
                            outer.get("error_code").map(|v| v.to_string()).unwrap_or_else(|| "absent".into())
                        );
                    }
                }
            }
        } else if interesting || all {
            let body: Value = serde_json::from_slice(&bytes[4..]).unwrap_or(Value::Null);
            let id = body
                .get("reqId")
                .or_else(|| body.pointer("/data/reqId"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            sent.insert(id, (t, cmd));
            println!("{when} -> {cmd}/{sub}");
            if ids && cmd == 545 {
                println!(
                    "        ack info (raw): {} queueName {}",
                    body.pointer("/data/info")
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    body.pointer("/data/queueName")
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                );
            }
            if interesting {
                let mut out = Vec::new();
                shape(&body, "", "", &mut labels, &mut out);
                for l in out.iter().take(40) {
                    println!("        {l}");
                }
            }
        }
    }
}
