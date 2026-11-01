//! Cancellation-aware retry timing independent of any service's wire protocol.
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug)]
pub struct Backoff {
    failures: u32,
    base: Duration,
    cap: Duration,
}
impl Default for Backoff {
    fn default() -> Self {
        Self {
            failures: 0,
            base: Duration::from_secs(1),
            cap: Duration::from_secs(30),
        }
    }
}
impl Backoff {
    pub fn next_delay(&mut self) -> Duration {
        let multiplier = 1u32 << self.failures.min(20);
        self.failures = self.failures.saturating_add(1);
        self.base.saturating_mul(multiplier).min(self.cap)
    }
    /// Brief failed handshakes do not reset retry pressure.
    pub fn record_healthy_connection(&mut self, uptime: Duration) {
        if uptime >= Duration::from_secs(30) {
            self.failures = 0;
        }
    }
}

/// False means cancellation or owner shutdown. No detached retry sleeps survive shutdown.
pub async fn wait_before_retry(cancel: &mut watch::Receiver<bool>, delay: Duration) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        if *cancel.borrow() {
            return false;
        }
        tokio::select! {
            biased;
            changed=cancel.changed() => { if changed.is_err() { return false; } },
            _=&mut sleep => return true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_is_capped_and_resets_only_after_a_healthy_connection() {
        let mut b = Backoff::default();
        assert_eq!(
            (0..7).map(|_| b.next_delay().as_secs()).collect::<Vec<_>>(),
            vec![1, 2, 4, 8, 16, 30, 30]
        );
        b.record_healthy_connection(Duration::from_secs(2));
        assert_eq!(b.next_delay().as_secs(), 30);
        b.record_healthy_connection(Duration::from_secs(30));
        assert_eq!(b.next_delay().as_secs(), 1);
    }
    #[tokio::test]
    async fn cancel_interrupts_long_retry_and_closed_owner_stops_retry() {
        let (tx, mut rx) = watch::channel(false);
        let task =
            tokio::spawn(async move { wait_before_retry(&mut rx, Duration::from_secs(30)).await });
        tx.send(true).unwrap();
        assert!(!tokio::time::timeout(Duration::from_millis(100), task)
            .await
            .unwrap()
            .unwrap());
        let (tx, mut rx) = watch::channel(false);
        drop(tx);
        assert!(!wait_before_retry(&mut rx, Duration::from_secs(30)).await);
    }
}
