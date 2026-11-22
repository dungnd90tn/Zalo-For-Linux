//! Read-only catch-up for authenticated desktop session and personal message queues.
use super::*;
use crate::protocol::signal;

pub(super) const COMMAND: u16 = 513;
pub(super) const SESSION_COMMAND: u16 = 516;
const SUBCOMMAND: u8 = 0;
const QUEUE: &str = "513_0";
const MAX_PAGES: u8 = 64;
const MAX_ROWS: usize = 200;
const SESSION_WAIT: Duration = Duration::from_secs(5);

pub(super) struct SessionCatchup {
    scope: signal::Scope,
    pages: signal::SessionPages,
    messages: Option<Catchup>,
    deadline: tokio::time::Instant,
    request_id: String,
    last_id: u64,
    requests: u8,
}

pub(super) struct Catchup {
    epoch: u64,
    requested: u64,
    request_id: String,
    pages: u8,
    durable_prefix: bool,
}

impl Catchup {
    fn cursor_key(epoch: u64) -> String {
        format!("signal-pc-{QUEUE}-{epoch}")
    }

    fn checkpoint(&mut self, page: &Page<'_>, all_durable: bool) -> Option<u64> {
        self.durable_prefix &= all_durable && !page.evicted;
        if !self.durable_prefix || page.rows.is_empty() {
            return None;
        }
        // An uncorrelated or missing item position must not move a checkpoint
        // beyond data that is demonstrably durable. The connection may still
        // request later pages, but a future connection will retry this prefix.
        let upper = page.next?;
        let mut last = self.requested;
        for row in page.rows {
            let Some(action) = row.get("actionId").and_then(action_position) else {
                self.durable_prefix = false;
                return None;
            };
            if action > upper {
                self.durable_prefix = false;
                return None;
            }
            last = last.max(action);
        }
        let checkpoint = last.min(page.checkpoint.unwrap_or(upper));
        (checkpoint > self.requested).then_some(checkpoint)
    }
}

struct Page<'a> {
    rows: &'a [Value],
    next: Option<u64>,
    checkpoint: Option<u64>,
    more: bool,
    evicted: bool,
}

fn action_position(value: &Value) -> Option<u64> {
    wire::id_string(value)
        .ok()
        .and_then(|id| safe_cursor(&id).ok())
}

fn position(value: Option<&Value>) -> std::result::Result<Option<u64>, wire::FrameReason> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) if value.as_u64() == Some(0) || value.as_str() == Some("0") => Ok(None),
        Some(value) => action_position(value)
            .map(Some)
            .ok_or(wire::FrameReason::QueueCursor),
    }
}

impl<'a> Page<'a> {
    fn parse(data: &'a Value, requested: u64) -> std::result::Result<Self, wire::FrameReason> {
        use wire::FrameReason;
        if !data.is_object() {
            return Err(FrameReason::BatchEnvelope);
        }
        let rows = match data.get("msgs") {
            None | Some(Value::Null) => &[][..],
            Some(Value::Array(rows)) if rows.len() <= MAX_ROWS => rows,
            Some(Value::Array(_)) => return Err(FrameReason::BatchLimit),
            _ => return Err(FrameReason::BatchEnvelope),
        };
        let more = match data.get("more") {
            None | Some(Value::Bool(false)) => false,
            Some(Value::Bool(true)) => true,
            Some(value) if value.as_u64() == Some(0) => false,
            Some(value) if value.as_u64() == Some(1) => true,
            _ => return Err(FrameReason::QueueMore),
        };
        let queue_status = data.get("queueStatus").filter(|status| !status.is_null());
        if queue_status.is_some_and(|status| !status.is_object()) {
            return Err(FrameReason::QueueStatus);
        }
        let status = queue_status.and_then(|status| status.get(QUEUE));
        if status.is_some_and(|status| !status.is_object()) {
            return Err(FrameReason::QueueStatus);
        }
        let checkpoint = position(status.and_then(|status| status.get("lastId")))?;
        let next = position(data.get("lastActionId"))?.or(checkpoint);
        if more && next.is_none_or(|next| next <= requested) {
            return Err(FrameReason::QueueProgress);
        }
        let evicted = match status.and_then(|status| status.get("evict")) {
            None | Some(Value::Null | Value::Bool(false)) => false,
            Some(Value::Bool(true)) => true,
            Some(value) if value.as_u64() == Some(0) || value.as_str() == Some("0") => false,
            Some(value) if value.as_u64() == Some(1) || value.as_str() == Some("1") => true,
            _ => return Err(FrameReason::QueueStatus),
        };
        Ok(Self {
            rows,
            next,
            checkpoint,
            more,
            evicted,
        })
    }
}

fn trace_queue(command: u16, kind: crate::diagnostics::Kind, reason: crate::diagnostics::Reason) {
    crate::diagnostics::record(
        crate::diagnostics::Event::new(kind, crate::diagnostics::Stage::Synchronization, reason)
            .frame(command, SUBCOMMAND, None),
    );
}

fn matches_request(reply: &Value, request_id: &str) -> bool {
    [reply.get("reqId"), reply.pointer("/data/reqId")]
        .into_iter()
        .flatten()
        .all(|value| value.is_null() || value.as_str() == Some(request_id))
}

impl Connection {
    pub(super) async fn start_e2ee_catchup(
        &mut self,
        db: &Database,
        events: &Events,
        advertised: &[Value],
    ) -> Result<()> {
        if !self.decoder.authenticated() {
            return Ok(());
        }
        let has_queue = |command: u16| {
            advertised.iter().any(|queue| {
                queue.get("cmd").and_then(Value::as_u64) == Some(command.into())
                    && queue.get("subCmd").and_then(Value::as_u64) == Some(SUBCOMMAND.into())
            })
        };
        let sessions = has_queue(SESSION_COMMAND);
        let messages = has_queue(COMMAND);
        if !sessions && !messages {
            trace_queue(
                COMMAND,
                crate::diagnostics::Kind::Debug,
                crate::diagnostics::Reason::Unsupported,
            );
            return Ok(());
        }
        let account = events.account.clone();
        let saved = db
            .call_with(StorageOperation::ReceiveCursor, move |store| {
                let Some(epoch) = store.e2ee_current_local(&account)? else {
                    return Ok(None);
                };
                let cursor = if messages {
                    let cursor = store.receive_cursor(&account, &Catchup::cursor_key(epoch))?;
                    safe_cursor(&cursor)?
                } else {
                    1
                };
                Ok(Some((epoch, cursor)))
            })
            .await?;
        let Some((epoch, cursor)) = saved else {
            trace_queue(
                if sessions { SESSION_COMMAND } else { COMMAND },
                crate::diagnostics::Kind::Transition,
                crate::diagnostics::Reason::Unsupported,
            );
            return Ok(());
        };
        let messages = messages.then_some(Catchup {
            epoch,
            requested: cursor,
            request_id: String::new(),
            pages: 0,
            durable_prefix: true,
        });
        if sessions {
            let scope = signal::Scope::new(&self.account, epoch)?;
            let pages = signal::SessionPages::new(&scope, SESSION_COMMAND)?;
            self.request_e2ee_session_page(SessionCatchup {
                scope,
                pages,
                messages,
                deadline: tokio::time::Instant::now() + SESSION_WAIT,
                request_id: String::new(),
                last_id: 1,
                requests: 0,
            })
            .await
        } else if let Some(messages) = messages {
            self.request_e2ee_page(messages).await
        } else {
            Ok(())
        }
    }

    async fn request_e2ee_cursor(
        &mut self,
        command: u16,
        first: bool,
        last_id: u64,
    ) -> Result<String> {
        let request_id = self.request();
        self.send(
            command,
            SUBCOMMAND,
            json!({"first":first,"reqId":request_id,"lastId":last_id,"preIds":[]}),
        )
        .await?;
        trace_queue(
            command,
            crate::diagnostics::Kind::Transition,
            crate::diagnostics::Reason::Requested,
        );
        Ok(request_id)
    }

    async fn request_e2ee_page(&mut self, mut catchup: Catchup) -> Result<()> {
        catchup.request_id = self
            .request_e2ee_cursor(COMMAND, catchup.pages == 0, catchup.requested)
            .await?;
        catchup.pages += 1;
        self.e2ee_catchup = Some(catchup);
        Ok(())
    }

    async fn request_e2ee_session_page(&mut self, mut catchup: SessionCatchup) -> Result<()> {
        catchup.request_id = self
            .request_e2ee_cursor(SESSION_COMMAND, catchup.requests == 0, catchup.last_id)
            .await?;
        catchup.requests += 1;
        self.e2ee_sessions = Some(catchup);
        Ok(())
    }

    pub(super) fn e2ee_session_deadline(&self) -> Option<tokio::time::Instant> {
        self.e2ee_sessions.as_ref().map(|active| active.deadline)
    }

    async fn resume_e2ee_messages(&mut self, messages: Option<Catchup>) -> Result<()> {
        if let Some(messages) = messages {
            self.request_e2ee_page(messages).await?;
        }
        Ok(())
    }

    pub(super) async fn expire_e2ee_sessions(&mut self) -> Result<()> {
        if let Some(active) = self.e2ee_sessions.take() {
            trace_queue(
                SESSION_COMMAND,
                crate::diagnostics::Kind::Failure,
                crate::diagnostics::Reason::Timeout,
            );
            self.resume_e2ee_messages(active.messages).await?;
        }
        Ok(())
    }

    pub(super) async fn process_e2ee_session_page(
        &mut self,
        frame: DecodedFrame,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        use crate::diagnostics::{Kind, Reason};
        if frame.sub_cmd != SUBCOMMAND || !self.decoder.authenticated() {
            return Ok(());
        }
        let Some(pending) = self.e2ee_sessions.as_ref() else {
            return Ok(());
        };
        if tokio::time::Instant::now() >= pending.deadline {
            return self.expire_e2ee_sessions().await;
        }
        if !matches_request(&frame.payload, &pending.request_id) {
            trace_queue(
                SESSION_COMMAND,
                Kind::Failure,
                Reason::CapabilityCorrelation,
            );
            return Ok(());
        }
        let mut active = self.e2ee_sessions.take().expect("active session queue");
        let complete = match active
            .pages
            .accept_desktop_receive(&active.scope, &frame.payload)
        {
            Ok(complete) => complete,
            Err(error) => {
                let reason = match error {
                    signal::SignalError::Scope | signal::SignalError::Correlation => {
                        Reason::CapabilityCorrelation
                    }
                    signal::SignalError::Resource => {
                        diagnostic_reason(wire::FrameReason::BatchLimit)
                    }
                    signal::SignalError::Incomplete | signal::SignalError::OutOfOrder => {
                        diagnostic_reason(wire::FrameReason::QueueProgress)
                    }
                    signal::SignalError::Unsupported => Reason::Unsupported,
                    _ => diagnostic_reason(wire::FrameReason::BatchEnvelope),
                };
                trace_queue(SESSION_COMMAND, Kind::Failure, reason);
                return self.resume_e2ee_messages(active.messages).await;
            }
        };
        if !complete {
            let reason = match position(frame.payload.pointer("/data/lastActionId")) {
                Ok(Some(next)) if next > active.last_id && active.requests < MAX_PAGES => {
                    active.last_id = next;
                    return self.request_e2ee_session_page(active).await;
                }
                _ if active.requests == MAX_PAGES => Reason::QueuePageLimit,
                _ => diagnostic_reason(wire::FrameReason::QueueProgress),
            };
            trace_queue(SESSION_COMMAND, Kind::Failure, reason);
            return self.resume_e2ee_messages(active.messages).await;
        }
        let account = events.account.clone();
        let epoch = active.scope.generation();
        if !db
            .call_with(StorageOperation::ReceiveCursor, move |store| {
                Ok(store.e2ee_current_local(&account)? == Some(epoch))
            })
            .await?
        {
            trace_queue(SESSION_COMMAND, Kind::Failure, Reason::CapabilityChanged);
            return Ok(());
        }
        let omitted = active.pages.had_omissions();
        let records = active.pages.finish(&active.scope)?;
        if !self.e2ee_receive_session_events(&records, db, events).await {
            trace_queue(
                SESSION_COMMAND,
                Kind::Failure,
                Reason::EncryptedReceiveFailed,
            );
        } else if omitted {
            trace_queue(SESSION_COMMAND, Kind::Failure, Reason::Unsupported);
        } else {
            trace_queue(SESSION_COMMAND, Kind::Transition, Reason::Ready);
        }
        self.resume_e2ee_messages(active.messages).await
    }

    pub(super) async fn receive_e2ee_push(
        &mut self,
        frame: &DecodedFrame,
        db: &Database,
        events: &Events,
    ) {
        use crate::diagnostics::{Kind, Reason};
        if !self.decoder.authenticated() || self.account != events.account {
            return;
        }
        if frame.cmd == 551 {
            self.e2ee_receive_push(frame, db, events).await;
            return;
        }
        let account = events.account.clone();
        let epoch = db
            .call_with(StorageOperation::ReceiveCursor, move |store| {
                store.e2ee_current_local(&account)
            })
            .await;
        let Ok(Some(epoch)) = epoch else {
            trace_queue(frame.cmd, Kind::Debug, Reason::EncryptedKeyUnavailable);
            return;
        };
        let Ok(scope) = signal::Scope::new(&self.account, epoch) else {
            return;
        };
        match signal::personal_session_push(&scope, &scope, frame.cmd, &frame.payload) {
            Ok(records) => {
                self.e2ee_receive_session_events(&records, db, events).await;
            }
            Err(_) => trace_queue(frame.cmd, Kind::Failure, Reason::EncryptedReceiveFailed),
        }
    }

    pub(super) async fn process_e2ee_catchup(
        &mut self,
        frame: DecodedFrame,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        use crate::diagnostics::{Kind, Reason};
        if frame.sub_cmd != SUBCOMMAND || !self.decoder.authenticated() {
            return Ok(());
        }
        let Some(active) = self.e2ee_catchup.as_ref() else {
            return Ok(());
        };
        let data = &frame.payload["data"];
        if !matches_request(&frame.payload, &active.request_id) {
            trace_queue(COMMAND, Kind::Failure, Reason::CapabilityCorrelation);
            return Ok(());
        }
        let mut active = self.e2ee_catchup.take().expect("active encrypted queue");
        let page = match Page::parse(data, active.requested) {
            Ok(page) => page,
            Err(reason) => {
                trace_queue(COMMAND, Kind::Failure, diagnostic_reason(reason));
                return Ok(());
            }
        };
        if page.evicted {
            trace_queue(COMMAND, Kind::Failure, Reason::QueueEvicted);
        }
        let account = events.account.clone();
        let expected_epoch = active.epoch;
        let key_matches = db
            .call_with(StorageOperation::ReceiveCursor, move |store| {
                Ok(store.e2ee_current_local(&account)? == Some(expected_epoch))
            })
            .await?;
        if !key_matches {
            trace_queue(COMMAND, Kind::Failure, Reason::CapabilityChanged);
            return Ok(());
        }
        let all_durable = self
            .e2ee_receive_batch(page.rows, db, events, COMMAND)
            .await;
        if let Some(checkpoint) = active.checkpoint(&page, all_durable) {
            let account = events.account.clone();
            db.call_with(StorageOperation::ReceiveCursorWrite, move |store| {
                if store.e2ee_current_local(&account)? == Some(expected_epoch) {
                    let queue = Catchup::cursor_key(expected_epoch);
                    store.receive_batch(&account, &[], Some((&queue, &checkpoint.to_string())))?;
                }
                Ok(())
            })
            .await?;
        }
        if page.more {
            if active.pages == MAX_PAGES {
                trace_queue(COMMAND, Kind::Failure, Reason::QueuePageLimit);
                return Ok(());
            }
            active.requested = page.next.expect("validated next encrypted cursor");
            self.request_e2ee_page(active).await?;
        } else if active.durable_prefix {
            trace_queue(COMMAND, Kind::Transition, Reason::Ready);
        } else {
            trace_queue(COMMAND, Kind::Failure, Reason::Unsupported);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(requested: u64) -> Catchup {
        Catchup {
            epoch: 10,
            requested,
            request_id: String::new(),
            pages: 1,
            durable_prefix: true,
        }
    }

    #[test]
    fn encrypted_queue_checkpoints_only_the_durable_prefix() {
        let mut catchup = active(10);
        let first = json!({"msgs":[{"actionId":"12"}],"more":true,"lastActionId":14});
        let first = Page::parse(&first, 10).unwrap();
        assert_eq!(catchup.checkpoint(&first, true), Some(12));
        catchup.requested = 14;
        let second = json!({"msgs":[{"actionId":15}],"more":1,"lastActionId":"16"});
        assert_eq!(
            catchup.checkpoint(&Page::parse(&second, 14).unwrap(), false),
            None
        );
        catchup.requested = 16;
        let third = json!({"msgs":[{"actionId":17}],"queueStatus":{"513_0":{"lastId":17}}});
        assert_eq!(
            catchup.checkpoint(&Page::parse(&third, 16).unwrap(), true),
            None
        );
        assert_ne!(Catchup::cursor_key(10), Catchup::cursor_key(11));
    }

    #[test]
    fn encrypted_queue_never_checkpoints_evicted_missing_or_mismatched_rows() {
        for data in [
            json!({"msgs":[{"actionId":11}],"queueStatus":{"513_0":{"lastId":12,"evict":1}}}),
            json!({"msgs":[{}],"lastActionId":12}),
            json!({"msgs":[{"actionId":13}],"lastActionId":12}),
        ] {
            let mut catchup = active(10);
            assert_eq!(
                catchup.checkpoint(&Page::parse(&data, 10).unwrap(), true),
                None
            );
            assert!(!catchup.durable_prefix);
        }
    }

    #[test]
    fn encrypted_queue_rejects_stalled_pagination_and_oversized_pages() {
        for data in [
            json!({"msgs":[],"more":true,"lastActionId":10}),
            json!({"msgs":[],"more":1,"lastActionId":0}),
            json!({"msgs":[],"more":true,"lastActionId":"9007199254740992"}),
            json!({"msgs": vec![Value::Null; MAX_ROWS+1]}),
        ] {
            assert!(Page::parse(&data, 10).is_err());
        }
    }
}
