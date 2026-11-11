//! The composer's send to an end-to-end encrypted conversation: the original's `sendE2eeMsg`
//! (`pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js`, line 159908).
//!
//! Normal composer safety (see `E2EE_PARITY.md`, D2/D3/D7):
//! 1. Read fresh enabled-device metadata, compatibility, and bundles. Initialize only verified missing
//!    peer sessions; stale bindings, missing own-device sessions, and unconfirmed initialization fail
//!    closed without automatic retry or plaintext fallback.
//! 2. Recheck the account, epoch, trust, and exact recipient coverage before reserving the draft.
//!    Verify each prepared record's routed address and actual session registration before committing.
//! 3. Post the committed exact ciphertext to `/api/e2ee/pc/message/sms` on `chat_e2e` and preserve
//!    ambiguous delivery without re-encrypting it. Per-device response results remain a parity gap.
use super::e2ee_send::{post_prepared, prepare_checked};
use super::signal_transport::Report;
use super::*;
use crate::protocol::signal::SignalError;

fn next_sequence() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(9_400_000);
    NEXT.fetch_add(128, Ordering::SeqCst)
}

/// What the user is told when the message cannot go; the draft is always kept.
fn init_failure_message(error: &anyhow::Error) -> &'static str {
    match error.downcast_ref::<SignalError>() {
        Some(SignalError::Service(-3)) => {
            "Người nhận chưa dùng phiên bản Zalo hỗ trợ mã hóa đầu cuối. Bản nháp được giữ lại."
        }
        Some(SignalError::Service(-5)) => {
            "Một thiết bị khác của tài khoản này chưa hỗ trợ mã hóa đầu cuối. Bản nháp được giữ lại."
        }
        _ => "Chưa thiết lập được phiên mã hóa với người nhận. Hãy thử lại; bản nháp được giữ lại.",
    }
}

/// The error chain, for the message the user sees. These are fixed strings written by this code (never keys,
/// ids or message text), so they are safe to show; they say which step failed.
fn detail(step: &str, error: &anyhow::Error) -> String {
    let chain: String = format!("{error:#}").chars().take(180).collect();
    format!("[{step}: {chain}]")
}

/// Keep server acceptance separate from local persistence and ambiguous delivery.
fn post_failure(error: &anyhow::Error) -> (String, bool, bool) {
    if error
        .downcast_ref::<super::e2ee_send::AcceptedSendError>()
        .is_some()
    {
        ("Zalo đã nhận tin nhắn nhưng chưa lưu được xác nhận trên thiết bị. Không gửi lại; bản nháp được giữ lại.".into(), false, true)
    } else {
        (
            format!(
                "Chưa có xác nhận từ Zalo. Không tự gửi lại. {}",
                detail("gửi", error)
            ),
            true,
            false,
        )
    }
}

impl Connection {
    /// Handshake if needed, then send. Emits `Sent` or `SendFailed`; never leaves the composer waiting.
    pub(super) async fn send_e2ee(
        &mut self,
        db: &Database,
        events: &Events,
        generation: u64,
        thread: String,
        request_id: u64,
        text: &str,
    ) -> Result<()> {
        let fail = |message: &str, unknown: bool, accepted: bool| LiveEvent::SendFailed {
            account_id: events.account.clone(),
            generation,
            thread: thread.clone(),
            request_id,
            error: message.into(),
            unknown_delivery: unknown,
            server_accepted: accepted,
        };
        let Some(http) = self.e2ee_http.clone() else {
            return events
                .emit(fail(
                    "Hội thoại này đã bật mã hóa đầu cuối. Client chưa sẵn sàng gửi ở chế độ này; bản nháp được giữ lại.",
                    false,
                    false,
                ))
                .await;
        };
        let Some(peer) = thread.strip_prefix("d:").map(str::to_owned) else {
            return events
                .emit(fail(
                    "Chỉ hỗ trợ mã hóa đầu cuối cho hội thoại 1-1.",
                    false,
                    false,
                ))
                .await;
        };
        let owner = self.account.clone();

        // Every send verifies the full fresh device set. Deterministic metadata,
        // identity and readiness errors must not retry account-changing handshakes.
        let readiness = match self
            .send_readiness(db, events, &peer, next_sequence())
            .await
        {
            Ok(readiness) => std::sync::Arc::new(readiness),
            Err(error) => {
                let error =
                    if error.downcast_ref::<SignalError>() == Some(&SignalError::Authentication) {
                        error.context(WireError::Authentication)
                    } else {
                        error
                    };
                let critical = error.downcast_ref::<StorageContext>().is_some()
                    || wire_error(&error) == Some(&WireError::Authentication);
                let _ = diagnostic(&error, LiveStage::Send);
                let message = format!(
                    "{} {}",
                    init_failure_message(&error),
                    detail("thiết bị", &error)
                );
                events.emit(fail(&message, false, false)).await?;
                if critical {
                    return Err(error);
                }
                return Ok(());
            }
        };

        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        let prepared = match prepare_checked(db, &owner, &peer, text, now_ms, readiness).await {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = diagnostic(&error, LiveStage::Send);
                let message = format!(
                    "Không mã hóa được tin nhắn cho người nhận. Bản nháp được giữ lại. {}",
                    detail("mã hóa", &error)
                );
                return events.emit(fail(&message, false, false)).await;
            }
        };
        let mut report = Report::discard();
        match post_prepared(db, &http, &owner, &prepared, &mut report).await {
            Ok((message, conversation)) => {
                events
                    .emit(LiveEvent::Sent {
                        account_id: owner,
                        generation,
                        thread,
                        request_id,
                        message,
                        conversation,
                    })
                    .await
            }
            Err(error) => {
                let _ = diagnostic(&error, LiveStage::Send);
                let (message, unknown, accepted) = post_failure(&error);
                events.emit(fail(&message, unknown, accepted)).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_request_blocks_do_not_overlap() {
        // Twenty metadata pages plus compatibility, three peer lookups, own
        // inventory and three missing-device renewals consume at most 28 IDs.
        let first = next_sequence();
        let next = next_sequence();
        assert!(next >= first + 128 && first + 28 < next);
    }

    #[test]
    fn accepted_local_failure_is_not_reported_as_unknown_delivery() {
        let error: anyhow::Error = super::super::e2ee_send::AcceptedSendError.into();
        let (message, unknown, accepted) = post_failure(&error);
        assert!(accepted && !unknown);
        assert!(message.contains("Zalo đã nhận") && message.contains("bản nháp"));
        let (_, unknown, accepted) = post_failure(&anyhow::anyhow!("no readable reply"));
        assert!(unknown && !accepted);
    }
}
