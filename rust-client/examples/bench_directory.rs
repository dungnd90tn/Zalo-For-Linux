//! Opt-in synthetic directory benchmark; never opens an existing profile.
use anyhow::{ensure, Result};
use serde_json::json;
use std::{collections::HashSet, path::PathBuf, time::Instant};
use zalo_rust::{
    domain::local_message_id,
    profile::Profile,
    store::{
        directory::{
            DirectoryBatch, DirectoryCursor, DirectoryRecord, DIRECTORY_LIMIT, MAX_DIRECTORY_ROWS,
        },
        MessageStore,
    },
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn record(index: usize, count: usize) -> DirectoryRecord {
    DirectoryRecord {
        remote_id: (100_000 + index).to_string(),
        is_group: false,
        display_name: if index + 1 == count {
            format!("Đặng Minh cuối {count:05}")
        } else {
            format!("Liên hệ kiểm thử {index:05}")
        },
        avatar_url: None,
        is_friend: Some(true),
        is_blocked: None,
        member_count: None,
    }
}
fn percentile(samples: &[u128], pct: usize) -> u128 {
    samples[(samples.len() - 1) * pct / 100]
}
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let count: usize = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(20_000);
    ensure!(
        args.next().is_none() && (1..=MAX_DIRECTORY_ROWS).contains(&count),
        "Usage: bench_directory [CONTACTS: 1..30000]"
    );
    let scratch =
        Scratch(std::env::temp_dir().join(format!("zalo-directory-bench-{}", local_message_id()?)));
    let profile = Profile::at_root(&scratch.0, "synthetic")?;
    let mut store = MessageStore::open(&profile.paths.data_dir.join("messages.sqlite"))?;
    let mut write_us = Vec::new();
    let mut replay_us = Vec::new();
    for start in (0..count).step_by(DIRECTORY_LIMIT) {
        let batch = DirectoryBatch {
            account_id: "0".into(),
            entries: (start..(start + DIRECTORY_LIMIT).min(count))
                .map(|i| record(i, count))
                .collect(),
            messages: vec![],
            observed_at_ms: 1_700_000_000_000,
        };
        let now = Instant::now();
        let committed = store.apply_directory_batch(&batch)?;
        write_us.push(now.elapsed().as_micros());
        ensure!(
            committed.metadata_rows == batch.entries.len(),
            "Directory insert count mismatch"
        );
        let now = Instant::now();
        store.apply_directory_batch(&batch)?;
        replay_us.push(now.elapsed().as_micros());
    }
    let mut after = None;
    let mut ids = HashSet::new();
    let mut page_us = Vec::new();
    loop {
        let now = Instant::now();
        let page = store.directory_page("0", false, "", after.as_ref(), 100)?;
        page_us.push(now.elapsed().as_micros());
        if page.is_empty() {
            break;
        }
        for entry in &page {
            let i = entry.remote_id.parse::<usize>()? - 100_000;
            ensure!(
                i < count && *entry == record(i, count) && ids.insert(entry.remote_id.clone()),
                "Directory identity/content/replay mismatch"
            );
        }
        after = page.last().map(DirectoryCursor::from_record);
    }
    ensure!(ids.len() == count, "Pagination omitted contacts");
    let mut search_us = Vec::new();
    let mut missing_us = Vec::new();
    for _ in 0..30 {
        let now = Instant::now();
        let hits = store.directory_page("0", false, "dang minh cuoi", None, 100)?;
        search_us.push(now.elapsed().as_micros());
        ensure!(
            hits == vec![record(count - 1, count)],
            "Whole-cache accent-fold search failed"
        );
        let now = Instant::now();
        ensure!(
            store
                .directory_page("0", false, "absent-directory-fixture", None, 100)?
                .is_empty(),
            "Missing-name search failed"
        );
        missing_us.push(now.elapsed().as_micros());
    }
    let cap_refused = if count == MAX_DIRECTORY_ROWS {
        let extra = record(count, count + 1);
        let batch = DirectoryBatch {
            account_id: "0".into(),
            entries: vec![extra.clone()],
            messages: vec![],
            observed_at_ms: 1_700_000_000_001,
        };
        ensure!(
            store.apply_directory_batch(&batch).is_err(),
            "Directory capacity was exceeded"
        );
        ensure!(
            store.directory_record("0", &extra.thread_id())?.is_none(),
            "Rejected batch leaked a row"
        );
        true
    } else {
        false
    };
    let insert_us: u128 = write_us.iter().sum();
    let replay_total_us: u128 = replay_us.iter().sum();
    for samples in [
        &mut write_us,
        &mut replay_us,
        &mut page_us,
        &mut search_us,
        &mut missing_us,
    ] {
        samples.sort_unstable();
    }
    println!(
        "{}",
        json!({"mode":"synthetic-directory", "contacts_verified":count, "batch_size":DIRECTORY_LIMIT, "insert_total_us":insert_us, "insert_batch_p50_us":percentile(&write_us,50), "insert_batch_p95_us":percentile(&write_us,95), "insert_batch_max_us":write_us.last(), "replay_total_us":replay_total_us, "page_p50_us":percentile(&page_us,50), "page_p95_us":percentile(&page_us,95), "page_queries_including_eof":page_us.len(), "search_samples":search_us.len(), "last_record_accent_search_p50_us":percentile(&search_us,50), "last_record_accent_search_p95_us":percentile(&search_us,95), "missing_search_p95_us":percentile(&missing_us,95), "capacity_refused_atomically":cap_refused, "network_used":false, "temporary_profile_removed_on_exit":true})
    );
    Ok(())
}
