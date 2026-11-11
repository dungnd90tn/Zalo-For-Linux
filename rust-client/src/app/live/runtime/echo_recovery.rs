//! Local-only recovery of accepted chat messages, before startup publishes history.
use super::{Database, StorageOperation};
use crate::store::e2ee::MAX_ACK_REPAIR_BATCH;
use anyhow::{ensure, Result};

pub(super) async fn repair(db: &Database, account: &str) -> Result<()> {
    let mut after = None;
    loop {
        let owner = account.to_owned();
        let batch = db
            .call_with(StorageOperation::ReconcileOutgoing, move |store| {
                store.e2ee_repair_acknowledged(&owner, after, MAX_ACK_REPAIR_BATCH)
            })
            .await?;
        if let Some(last) = batch.last_client_id {
            ensure!(
                after.is_none_or(|previous| last > previous),
                "Local echo recovery did not advance"
            );
            after = Some(last);
        }
        if batch.repaired < MAX_ACK_REPAIR_BATCH {
            return Ok(());
        }
        // The outer runtime cancellation select can stop startup between bounded actor jobs.
        tokio::task::yield_now().await;
    }
}
