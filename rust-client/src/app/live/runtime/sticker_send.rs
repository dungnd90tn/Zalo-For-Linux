//! Preflight, authenticated delivery and durable projection for ordinary stickers.
use super::*;
use crate::{
    domain::StickerDescriptor, protocol::sticker_send as sticker_wire, store::StickerReservation,
};

pub(super) struct Request {
    pub request_id: u64,
    pub thread: String,
    pub sticker: StickerDescriptor,
}

impl Request {
    pub async fn finish(self, events: &Events, outcome: StickerOutcome) -> Result<()> {
        events
            .emit(LiveEvent::StickerFinished {
                account_id: events.account.clone(),
                request_id: self.request_id,
                thread: self.thread,
                outcome,
            })
            .await
    }
}

pub(super) fn normal_scope(events: &Events, thread: &str) -> Option<u64> {
    let state = events.privacy.read().ok()?;
    (state.confirmed && state.status == LivePrivacyStatus::Ready && !state.hidden.contains(thread))
        .then_some(state.revision)
}

pub(super) async fn handle(
    request: Request,
    catalog_authorized: bool,
    connection: Option<&mut Connection>,
    db: &Database,
    events: &Events,
    session: &VerifiedSession,
    client: &reqwest::Client,
) -> Result<CommandOutcome> {
    use StickerOutcome::*;
    let allowed = Target::from_thread(&request.thread).ok().filter(|target| {
        request.request_id > 0
            && (request.sticker.is_builtin()
                || (catalog_authorized && request.sticker.is_catalog()))
            && (target.kind == ThreadKind::Group || target.remote_id != events.account)
    });
    let (Some(connection), Some(revision), Some(target)) =
        (connection, normal_scope(events, &request.thread), allowed)
    else {
        request.finish(events, Unavailable).await?;
        return Ok(CommandOutcome::Continue);
    };
    trace(
        crate::diagnostics::Stage::Send,
        crate::diagnostics::Reason::Requested,
    );
    let root = root_for(
        session,
        match target.kind {
            ThreadKind::Direct => "chat",
            ThreadKind::Group => "group",
        },
    )
    .and_then(|root| {
        Ok(sticker_wire::url(
            &root,
            target.kind,
            session.client_type(),
        )?)
    });
    let Ok(url) = root else {
        request.finish(events, Unavailable).await?;
        return Ok(CommandOutcome::Continue);
    };

    let eligibility = async {
        match target.kind {
            ThreadKind::Direct => connection.verify_direct(&target, db, events).await,
            ThreadKind::Group => {
                // The original sticker request has no visibility parameter.
                // Unknown or nonstandard groups use no ordinary sticker route.
                if verify_group(client, session, &target).await? == 0 {
                    Ok(())
                } else {
                    Err(WireError::Unsupported.into())
                }
            }
        }
    };
    // Always drain the already-started direct proof even if HTTP rejects TTL.
    // Leaving its reply in the socket could break the next correlated proof.
    let (eligible, ttl) = tokio::join!(
        eligibility,
        forward::verify_target_ttl(client, session, &target)
    );
    let ttl = match eligible.and(ttl) {
        Ok(ttl) if normal_scope(events, &request.thread) == Some(revision) => ttl,
        Ok(_) => {
            request.finish(events, Unavailable).await?;
            return Ok(CommandOutcome::Continue);
        }
        Err(error) => {
            request.finish(events, Unavailable).await?;
            return handle_preflight_error(events, &error).await;
        }
    };
    let account = events.account.clone();
    let thread = request.thread.clone();
    let sticker = request.sticker;
    let reserved = db
        .call_with(StorageOperation::PrepareOutgoing, move |store| {
            store.prepare_sticker(&account, &thread, sticker, now_ms())
        })
        .await;
    let id = match reserved {
        Ok(StickerReservation::Ready(id)) => id,
        Ok(StickerReservation::Unconfirmed) => {
            request.finish(events, Unknown).await?;
            return Ok(CommandOutcome::Continue);
        }
        Err(error) => {
            record_storage_failure(&error, StorageOperation::PrepareOutgoing);
            request.finish(events, Unavailable).await?;
            return Ok(CommandOutcome::Continue);
        }
    };
    let body = request_body(session, &target, sticker, id);
    let body = match body {
        Ok(body)
            if normal_scope(events, &request.thread) == Some(revision) && ttl.applies(&target) =>
        {
            body
        }
        _ => {
            release(db, events, id).await;
            request.finish(events, Unavailable).await?;
            return Ok(CommandOutcome::Continue);
        }
    };
    let ack = send(client, session, url, body, &target, id).await;
    let mut pause = false;
    let outcome = match ack {
        Ok(ack) => {
            let account = events.account.clone();
            match db
                .call_with(StorageOperation::PersistAck, move |store| {
                    store.accept_sticker(&account, id, &ack)
                })
                .await
            {
                Ok((message, conversation)) => {
                    events
                        .emit(LiveEvent::Received {
                            account_id: events.account.clone(),
                            message,
                            conversation,
                        })
                        .await?;
                    Accepted
                }
                Err(error) => {
                    record_storage_failure(&error, StorageOperation::PersistAck);
                    Unknown
                }
            }
        }
        Err(error) => {
            let detail = diagnostic(&error, LiveStage::Send);
            let explicit_rejection = matches!(
                wire_error(&error),
                Some(WireError::Authentication | WireError::Service(_))
            );
            pause = wire_error(&error) == Some(&WireError::Authentication);
            if pause {
                events.diagnostic(detail).await?;
            }
            if explicit_rejection && release(db, events, id).await {
                Rejected
            } else {
                Unknown
            }
        }
    };
    request.finish(events, outcome).await?;
    if pause {
        events.connection(LiveConnection::Expired).await?;
        Ok(CommandOutcome::PauseTransport)
    } else {
        Ok(CommandOutcome::Continue)
    }
}

async fn handle_preflight_error(events: &Events, error: &anyhow::Error) -> Result<CommandOutcome> {
    let diagnostic = diagnostic(error, LiveStage::Send);
    if wire_error(error) == Some(&WireError::Authentication) {
        events.diagnostic(diagnostic).await?;
        events.connection(LiveConnection::Expired).await?;
        Ok(CommandOutcome::PauseTransport)
    } else if error.downcast_ref::<StorageContext>().is_some() {
        events.diagnostic(diagnostic).await?;
        events.connection(LiveConnection::Offline).await?;
        Ok(CommandOutcome::PauseTransport)
    } else {
        Ok(CommandOutcome::Continue)
    }
}

async fn release(db: &Database, events: &Events, client: u64) -> bool {
    let account = events.account.clone();
    match db
        .call_with(StorageOperation::FailOutgoing, move |store| {
            store.release_sticker(&account, client)
        })
        .await
    {
        Ok(()) => true,
        Err(error) => {
            record_storage_failure(&error, StorageOperation::FailOutgoing);
            false
        }
    }
}

fn request_body(
    session: &VerifiedSession,
    target: &Target,
    sticker: StickerDescriptor,
    id: u64,
) -> Result<String> {
    let params = sticker_wire::parameters(target, sticker, id, session.imei())?;
    let encrypted = session.encrypt_payload(&serde_json::to_vec(&params)?)?;
    Ok(url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &encrypted)
        .finish())
}

async fn send(
    client: &reqwest::Client,
    session: &VerifiedSession,
    url: Url,
    body: String,
    target: &Target,
    id: u64,
) -> Result<wire::SendAck> {
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        authenticated_request(client, session, url, Some(body)),
    )
    .await??;
    Ok(sticker_wire::ack(&response, target, id)?)
}
