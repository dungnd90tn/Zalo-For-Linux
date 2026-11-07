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
const SIGNAL: std::ops::RangeInclusive<u16> = 540..=564;

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

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .expect("usage: decode_ws_capture <frames.jsonl> [--all]");
    let all = args.any(|a| a == "--all");
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
        let interesting = SIGNAL.contains(&cmd);
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
