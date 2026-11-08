//! The composer's send to an end-to-end encrypted conversation: the original's `sendE2eeMsg`
//! (`pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js`, line 159908).
//!
//! Ported behavior (see `E2EE_PARITY.md`, D2/D3/D7):
//! 1. `checkSessionThenInit`: a device without a session gets the handshake first (560, 541 with `isReInit`,
//!    543). An init failure is tried again up to `max_retry_reinit` (3) times, immediately; a refusal with
//!    `-3` (client unsupported) or `-5` (own subdevice unsupported) is not retried: the original opens its
//!    downgrade dialog and fails the message (`default_11_allow_downgrade` is true), and so does this.
//! 2. The message is encrypted for the contact's devices and this account's other devices, committed, then
//!    posted to `/api/e2ee/pc/message/sms` on the `chat_e2e` service.
//! 3. Per-device results in the reply are not read (only the ids and the timestamp), as in the original.
use super::e2ee_send::{has_session, post_prepared, prepare_authorized};
use super::signal_transport::Report;
use super::*;
use crate::protocol::signal::SignalError;

/// `max_retry_reinit` (PC 226700 area): immediate re-attempts of a failed initialization.
const MAX_RETRY_REINIT: usize = 3;

fn next_sequence() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(9_400_000);
    NEXT.fetch_add(16, Ordering::SeqCst)
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
        let fail = |message: &str, unknown: bool| LiveEvent::SendFailed {
            account_id: events.account.clone(),
            generation,
            thread: thread.clone(),
            request_id,
            error: message.into(),
            unknown_delivery: unknown,
            server_accepted: false,
        };
        let Some(http) = self.e2ee_http.clone() else {
            return events
                .emit(fail(
                    "Chưa sẵn sàng gửi mã hóa. Bản nháp được giữ lại.",
                    false,
                ))
                .await;
        };
        let Some(peer) = thread.strip_prefix("d:").map(str::to_owned) else {
            return events
                .emit(fail("Chỉ hỗ trợ mã hóa đầu cuối cho hội thoại 1-1.", false))
                .await;
        };
        let owner = self.account.clone();

        // checkSessionThenInit: a missing session means the handshake first.
        let mut attempt = 0;
        while !has_session(db, &owner, &peer).await? {
            match self.handshake_with(db, &peer, next_sequence()).await {
                Ok(_) => break,
                Err(error) => {
                    let definitive = matches!(
                        error.downcast_ref::<SignalError>(),
                        Some(SignalError::Service(-3 | -5))
                    );
                    let _ = diagnostic(&error, LiveStage::Send);
                    attempt += 1;
                    if definitive || attempt > MAX_RETRY_REINIT {
                        let message = format!(
                            "{} {}",
                            init_failure_message(&error),
                            detail("bắt tay", &error)
                        );
                        return events.emit(fail(&message, false)).await;
                    }
                }
            }
        }

        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        let prepared = match prepare_authorized(db, &owner, &peer, text, now_ms).await {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = diagnostic(&error, LiveStage::Send);
                let message = format!(
                    "Không mã hóa được tin nhắn cho người nhận. Bản nháp được giữ lại. {}",
                    detail("mã hóa", &error)
                );
                return events.emit(fail(&message, false)).await;
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
                let message = format!(
                    "Chưa có xác nhận từ Zalo. Không tự gửi lại. {}",
                    detail("gửi", &error)
                );
                events.emit(fail(&message, true)).await
            }
        }
    }
}
