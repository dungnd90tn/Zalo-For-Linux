//! Ordinary @member sends bind the composer request to a verified group roster.
use super::*;
use crate::{
    domain::mention::{self, Mention},
    protocol::mention as mention_wire,
};

pub(super) struct Request {
    pub generation: u64,
    pub thread: String,
    pub text: String,
    pub mentions: Vec<Mention>,
    pub privacy_revision: u64,
    pub request_id: u64,
}

impl Request {
    fn roster(&self) -> group_roster::Request {
        group_roster::Request {
            thread: self.thread.clone(),
            privacy_revision: self.privacy_revision,
            after: None,
        }
    }

    pub(super) async fn failed(
        self,
        events: &Events,
        error: &'static str,
        unknown_delivery: bool,
        server_accepted: bool,
    ) -> Result<()> {
        events
            .emit(LiveEvent::SendFailed {
                account_id: events.account.clone(),
                generation: self.generation,
                thread: self.thread,
                request_id: self.request_id,
                error: error.into(),
                unknown_delivery,
                server_accepted,
            })
            .await
    }

    pub(super) async fn unavailable(self, events: &Events) -> Result<CommandOutcome> {
        self.failed(
            events,
            "Chưa xác minh được nhóm hoặc thành viên được nhắc. Bản nháp được giữ lại.",
            false,
            false,
        )
        .await?;
        Ok(CommandOutcome::Continue)
    }
}

fn verified_ack(payload: &Value, target: &Target, client_id: u64) -> Result<wire::SendAck> {
    let ack = wire::send_ack(payload)?;
    let data = &payload["data"];
    if let Some(id) = data.get("clientId") {
        if wire::id_string(id)? != client_id.to_string() {
            return Err(WireError::Invalid.into());
        }
    }
    for key in ["grid", "groupId"] {
        if let Some(id) = data.get(key) {
            if wire::id_string(id)? != target.remote_id {
                return Err(WireError::Invalid.into());
            }
        }
    }
    Ok(ack)
}

pub(super) async fn handle(
    request: Request,
    online: bool,
    cache: &mut Option<group_roster::Proof>,
    db: &Database,
    events: &Events,
    session: &VerifiedSession,
    client: &reqwest::Client,
) -> Result<CommandOutcome> {
    let roster_request = request.roster();
    let Ok(target) = roster_request.target() else {
        return request.unavailable(events).await;
    };
    let mention_all = request.mentions.iter().any(Mention::is_all);
    let all_limit = session.config().mention_all_limit;
    if !online
        || request.request_id == 0
        || (mention_all && all_limit.is_none())
        || request
            .mentions
            .iter()
            .any(|mention| mention.uid == events.account)
        || mention::validate_outgoing(&request.text, &request.mentions).is_err()
        || !roster_request.permits(&events.privacy)
    {
        return request.unavailable(events).await;
    }
    let (owner, thread) = (events.account.clone(), request.thread.clone());
    if db
        .call(move |store| store.validate_group_scope(&owner, &thread))
        .await
        .is_err()
    {
        return request.unavailable(events).await;
    }

    let cached = !mention_all
        && cache
            .as_ref()
            .is_some_and(|proof| proof.applies(&roster_request, &events.account));
    let roster = async {
        if cached {
            Ok(None)
        } else {
            group_roster::verify_members(client, session, &events.privacy, &roster_request)
                .await
                .map(Some)
        }
    };
    let (visibility, ttl, recent) = match tokio::try_join!(
        verify_group(client, session, &target),
        forward::verify_target_ttl(client, session, &target),
        roster,
    ) {
        Ok(proofs) => proofs,
        Err(error) => {
            request.unavailable(events).await?;
            return connection_after_rejection(events, &error).await;
        }
    };
    if let Some(proof) = recent {
        *cache = Some(proof);
    }
    let still_verified = || {
        roster_request.permits(&events.privacy)
            && ttl.applies(&target)
            && cache.as_ref().is_some_and(|proof| {
                proof.applies(&roster_request, &events.account)
                    && request.mentions.iter().all(|mention| {
                        if mention.is_all() {
                            visibility == 0 && proof.allows_mention_all(all_limit)
                        } else {
                            proof.member_visible(&mention.uid)
                        }
                    })
            })
    };
    if !still_verified() {
        return request.unavailable(events).await;
    }

    let (owner, thread, text, mentions) = (
        events.account.clone(),
        request.thread.clone(),
        request.text.clone(),
        request.mentions.clone(),
    );
    let id = match db
        .call_with(StorageOperation::PrepareOutgoing, move |store| {
            store.prepare_outgoing_mention(&owner, &thread, &text, &mentions, now_ms())
        })
        .await
    {
        Ok(id) => id,
        Err(error) => {
            record_storage_failure(&error, StorageOperation::PrepareOutgoing);
            request.failed(
                events, "Chưa lưu được lượt gửi nhắc tên, hoặc lượt trước chưa xác định. Bản nháp được giữ lại.",
                true, false,
            ).await?;
            return Ok(CommandOutcome::Continue);
        }
    };
    let parameters =
        mention_wire::parameters(&target, &request.text, id, &request.mentions, visibility);
    let response = match parameters {
        Ok(parameters) if still_verified() => {
            group_roster::post(client, session, "group", mention_wire::PATH, parameters).await
        }
        _ => {
            let owner = events.account.clone();
            db.call_with(StorageOperation::FailOutgoing, move |store| {
                store.fail_outgoing(&owner, id, false)
            })
            .await?;
            return request.unavailable(events).await;
        }
    };

    match response.and_then(|payload| verified_ack(&payload, &target, id)) {
        Ok(ack) => match persist_ack(db, &events.account, id, ack).await {
            Ok((message, conversation)) => {
                events
                    .emit(LiveEvent::Sent {
                        account_id: events.account.clone(),
                        generation: request.generation,
                        thread: request.thread,
                        request_id: request.request_id,
                        message,
                        conversation,
                    })
                    .await?;
            }
            Err(error) => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Storage))
                    .await?;
                request
                    .failed(
                        events,
                        "Zalo đã nhận tin nhắc tên; chưa lưu được xác nhận. Không gửi lại.",
                        true,
                        true,
                    )
                    .await?;
                events.connection(LiveConnection::Offline).await?;
                return Ok(CommandOutcome::PauseTransport);
            }
        },
        Err(error) => {
            let known = matches!(
                wire_error(&error),
                Some(WireError::Authentication | WireError::Service(_))
            );
            let owner = events.account.clone();
            db.call_with(StorageOperation::FailOutgoing, move |store| {
                store.fail_outgoing(&owner, id, !known)
            })
            .await?;
            request
                .failed(
                    events,
                    if known {
                        "Zalo từ chối lượt gửi nhắc tên. Bản nháp được giữ lại."
                    } else {
                        "Chưa có xác nhận lượt gửi nhắc tên. Không tự gửi lại."
                    },
                    !known,
                    false,
                )
                .await?;
            return connection_after_rejection(events, &error).await;
        }
    }
    Ok(CommandOutcome::Continue)
}

async fn connection_after_rejection(
    events: &Events,
    error: &anyhow::Error,
) -> Result<CommandOutcome> {
    let detail = diagnostic(error, LiveStage::Send);
    if wire_error(error) == Some(&WireError::Authentication) {
        events.diagnostic(detail).await?;
        events.connection(LiveConnection::Expired).await?;
        Ok(CommandOutcome::PauseTransport)
    } else {
        Ok(CommandOutcome::Continue)
    }
}
