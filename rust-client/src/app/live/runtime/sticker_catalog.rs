//! Single cancellable catalog search per authenticated account.
use super::*;
use crate::{domain::StickerDescriptor, protocol::sticker_catalog as catalog_wire};
use tokio::{task::JoinHandle, time::Instant};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(12);
const SELECTION_TTL: Duration = Duration::from_secs(300);

pub(super) struct Request {
    pub id: u64,
    pub thread: String,
    pub query: String,
}

pub(super) async fn publish(
    events: &Events,
    request_id: u64,
    thread: String,
    stickers: Option<Vec<StickerDescriptor>>,
) -> Result<()> {
    events
        .emit(LiveEvent::StickersFound {
            account_id: events.account.clone(),
            request_id,
            thread,
            stickers,
        })
        .await
}

#[derive(Default)]
pub(super) struct Search {
    active: Option<Running>,
    resolved: Option<Resolved>,
}

struct Running {
    id: u64,
    thread: String,
    revision: u64,
    task: JoinHandle<Result<Vec<StickerDescriptor>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Resolved {
    id: u64,
    thread: String,
    revision: u64,
    checked_at: Instant,
    stickers: Vec<StickerDescriptor>,
}

pub(super) struct Completion {
    id: u64,
    thread: String,
    revision: u64,
    stickers: Option<Vec<StickerDescriptor>>,
}

impl Search {
    pub async fn start(
        &mut self,
        request: Request,
        online: bool,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        self.active = None;
        self.resolved = None;
        let scope = sticker_send::normal_scope(events, &request.thread).filter(|_| {
            online
                && request.id > 0
                && catalog_wire::valid_query(&request.query)
                && Target::from_thread(&request.thread).is_ok_and(|target| {
                    target.kind == ThreadKind::Group || target.remote_id != events.account
                })
        });
        let (Some(revision), Ok(origin)) = (scope, root_for(session, "sticker")) else {
            return publish(events, request.id, request.thread, None).await;
        };
        let session = session.clone();
        let client = client.clone();
        self.active = Some(Running {
            id: request.id,
            thread: request.thread,
            revision,
            task: tokio::spawn(async move {
                tokio::time::timeout(
                    LOOKUP_TIMEOUT,
                    resolve(&client, &session, &origin, &request.query),
                )
                .await?
            }),
        });
        Ok(())
    }

    pub async fn next(&mut self) -> Completion {
        let Some(running) = &mut self.active else {
            return std::future::pending().await;
        };
        let stickers = (&mut running.task).await.ok().and_then(Result::ok);
        let running = self
            .active
            .take()
            .expect("completed sticker lookup is active");
        Completion {
            id: running.id,
            thread: running.thread.clone(),
            revision: running.revision,
            stickers,
        }
    }

    pub async fn complete(&mut self, mut found: Completion, events: &Events) -> Result<()> {
        let allowed = sticker_send::normal_scope(events, &found.thread) == Some(found.revision);
        if !allowed {
            found.stickers = None;
        }
        self.resolved = found.stickers.clone().map(|stickers| Resolved {
            id: found.id,
            thread: found.thread.clone(),
            revision: found.revision,
            checked_at: Instant::now(),
            stickers,
        });
        publish(events, found.id, found.thread, found.stickers).await
    }

    pub fn permits(
        &self,
        search_id: u64,
        thread: &str,
        sticker: StickerDescriptor,
        events: &Events,
    ) -> bool {
        self.resolved.as_ref().is_some_and(|found| {
            found.id == search_id
                && found.thread == thread
                && found.checked_at.elapsed() <= SELECTION_TTL
                && sticker_send::normal_scope(events, thread) == Some(found.revision)
                && found.stickers.contains(&sticker)
        })
    }
}

async fn lookup(
    client: &reqwest::Client,
    session: &VerifiedSession,
    origin: &Url,
    path: &'static str,
    params: Value,
) -> Result<Value> {
    let params = session.encrypt_payload(&serde_json::to_vec(&params)?)?;
    let url = catalog_wire::url(origin, path, session.client_type(), &params)?;
    authenticated_request(client, session, url, None).await
}

async fn resolve(
    client: &reqwest::Client,
    session: &VerifiedSession,
    origin: &Url,
    query: &str,
) -> Result<Vec<StickerDescriptor>> {
    let body = lookup(
        client,
        session,
        origin,
        catalog_wire::SEARCH_PATH,
        catalog_wire::search_parameters(query, session.imei())?,
    )
    .await?;
    let addresses = catalog_wire::search_results(&body)?;
    let requested = addresses.len();
    let mut resolved = 0;
    let details = futures_util::stream::iter(addresses.into_iter().map(|address| async move {
        let response = lookup(
            client,
            session,
            origin,
            catalog_wire::DETAIL_PATH,
            catalog_wire::detail_parameters(address),
        )
        .await?;
        catalog_wire::detail(&response, address).map_err(anyhow::Error::new)
    }))
    .buffered(4);
    tokio::pin!(details);
    let mut usable = Vec::new();
    while let Some(detail) = details.next().await {
        if let Ok(sticker) = detail {
            resolved += 1;
            usable.extend(sticker);
        }
    }
    if resolved == 0 && requested > 0 {
        Err(WireError::Invalid.into())
    } else {
        Ok(usable)
    }
}
