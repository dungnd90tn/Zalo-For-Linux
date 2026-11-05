//! Opt-in synthetic storage benchmark. Never opens an existing account or database.
use anyhow::{bail, ensure, Result};
use serde_json::json;
use std::time::{Duration, Instant};
use zalo_rust::{
    domain::{local_message_id, Message, MAX_PAGE_SIZE},
    profile::Profile,
    store::MessageStore,
};

const ACCOUNT: &str = "0";
const THREAD: &str = "synthetic-benchmark";
const EPOCH_MS: i64 = 1_700_000_000_000;
const PAGE_SIZE: usize = 127;

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sample(index: usize) -> Message {
    Message {
        account_id: ACCOUNT.into(), thread_id: THREAD.into(), id: format!("{index:010}"),
        sender: "synthetic-sender".into(),
        text: format!("Synthetic message {index:010}: bounded transactions and indexed cursor pagination with twenty timestamp ties."),
        sticker: None,
        media: None,
        sent_at_ms: EPOCH_MS + (index / 20) as i64,
    }
}

fn percentile(samples: &[u128], percentile: usize) -> u128 {
    samples[(samples.len() - 1) * percentile / 100]
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let count = args
        .next()
        .map(|v| v.parse::<usize>())
        .transpose()?
        .unwrap_or(20_000);
    if args.next().is_some() || !(1..=200_000).contains(&count) {
        bail!("Usage: bench_store [MESSAGE_COUNT: 1..200000]");
    }
    let wall = Instant::now();
    let scratch =
        Scratch(std::env::temp_dir().join(format!("zalo-native-bench-{}", local_message_id()?)));
    let profile = Profile::at_root(&scratch.0, "synthetic")?;
    profile.bind_account(ACCOUNT)?;
    let mut store = MessageStore::open(&profile.paths.data_dir.join("messages.sqlite3"))?;
    let mut insert_time = Duration::ZERO;
    let mut replay_time = Duration::ZERO;
    let mut inserted = 0;
    let mut replay_inserted = 0;
    for start in (0..count).step_by(MAX_PAGE_SIZE) {
        let batch: Vec<_> = (start..(start + MAX_PAGE_SIZE).min(count))
            .map(sample)
            .collect();
        let start = Instant::now();
        inserted += store.insert_batch(&batch)?;
        insert_time += start.elapsed();
        let start = Instant::now();
        replay_inserted += store.insert_batch(&batch)?;
        replay_time += start.elapsed();
    }
    ensure!(
        inserted == count && replay_inserted == 0,
        "Synthetic ingestion/deduplication count mismatch"
    );
    let mut cursor = None;
    let mut remaining = count;
    let mut query_us = Vec::new();
    let mut messages_verified = 0;
    loop {
        let start = Instant::now();
        let page = store.page(ACCOUNT, THREAD, cursor.as_ref(), PAGE_SIZE)?;
        query_us.push(start.elapsed().as_micros());
        if page.is_empty() {
            break;
        }
        ensure!(
            page.len() <= remaining,
            "Duplicate page or cursor failed to advance"
        );
        let page_start = remaining - page.len();
        for (offset, message) in page.iter().enumerate() {
            ensure!(
                *message == sample(page_start + offset),
                "Identity/order/content mismatch across timestamp ties"
            );
        }
        remaining = page_start;
        messages_verified += page.len();
        cursor = page.first().map(Message::cursor);
    }
    ensure!(
        remaining == 0 && messages_verified == count,
        "Pagination has missing messages"
    );
    query_us.sort_unstable();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "benchmark":"synthetic-sqlite-keyset", "client_version":env!("CARGO_PKG_VERSION"),
            "profile":"temporary-synthetic-removed-on-exit", "messages":count,
            "batch_limit":MAX_PAGE_SIZE,"page_size":PAGE_SIZE,"timestamp_ties":20,
            "inserted":inserted,"replayed_rows_inserted":replay_inserted,"messages_verified":messages_verified,
            "insert_ms":insert_time.as_secs_f64()*1000.0,"replay_ms":replay_time.as_secs_f64()*1000.0,
            "page_queries_including_eof":query_us.len(),"page_p50_us":percentile(&query_us,50),
            "page_p95_us":percentile(&query_us,95),"page_max_us":query_us.last(),
            "wall_ms":wall.elapsed().as_secs_f64()*1000.0,"network_used":false,
            "note":"Synthetic release measurement only; no live Zalo/Electron performance comparison."
        }))?
    );
    // Explicit drop order releases SQLite, then profile lock, then removes scratch files.
    drop(store);
    drop(profile);
    drop(scratch);
    Ok(())
}
