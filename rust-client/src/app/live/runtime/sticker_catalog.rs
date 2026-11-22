//! Cancellable sticker catalog lookup with account, thread, and session-scoped selections.
use super::*;
use crate::{domain::StickerDescriptor, protocol::sticker_catalog as catalog_wire};
use tokio::{task::JoinHandle, time::Instant};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(12);
const SELECTION_TTL: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub(super) struct Request {
    pub id: u64,
    pub thread: String,
    pub kind: Kind,
}

#[derive(Clone)]
pub(super) enum Kind {
    Search(String),
    ListPacks,
    BrowsePack { list_id: u64, category_id: u64 },
}

enum Entries {
    Packs(Vec<StickerPack>),
    Stickers(Vec<StickerDescriptor>),
}

impl Request {
    pub fn from_command(command: &LiveCommand) -> Option<Self> {
        let (id, thread, kind) = match command {
            LiveCommand::SearchStickers {
                request_id,
                thread,
                query,
            } => (*request_id, thread, Kind::Search(query.clone())),
            LiveCommand::ListStickerPacks { request_id, thread } => {
                (*request_id, thread, Kind::ListPacks)
            }
            LiveCommand::BrowseStickerPack {
                request_id,
                thread,
                pack_list_id,
                category_id,
            } => (
                *request_id,
                thread,
                Kind::BrowsePack {
                    list_id: *pack_list_id,
                    category_id: *category_id,
                },
            ),
            _ => return None,
        };
        Some(Self {
            id,
            thread: thread.clone(),
            kind,
        })
    }

    pub async fn unavailable(self, events: &Events) -> Result<()> {
        self.publish(events, None).await
    }

    async fn publish(self, events: &Events, entries: Option<Entries>) -> Result<()> {
        let account_id = events.account.clone();
        let request_id = self.id;
        let thread = self.thread;
        let event = match self.kind {
            Kind::ListPacks => LiveEvent::StickerPacksFound {
                account_id,
                request_id,
                thread,
                packs: entries.and_then(|entries| match entries {
                    Entries::Packs(packs) => Some(packs),
                    _ => None,
                }),
            },
            kind => {
                let stickers = entries.and_then(|entries| match entries {
                    Entries::Stickers(stickers) => Some(stickers),
                    _ => None,
                });
                match kind {
                    Kind::BrowsePack { category_id, .. } => LiveEvent::StickerPackFound {
                        account_id,
                        request_id,
                        thread,
                        category_id,
                        stickers,
                    },
                    _ => LiveEvent::StickersFound {
                        account_id,
                        request_id,
                        thread,
                        stickers,
                    },
                }
            }
        };
        events.emit(event).await
    }
}

#[derive(Default)]
pub(super) struct Search {
    active: Option<Running>,
    pack_list: Option<Resolved<Vec<u64>>>,
    resolved: Option<Resolved<Vec<StickerDescriptor>>>,
}

struct Running {
    request: Request,
    revision: u64,
    authenticated: Instant,
    task: JoinHandle<Result<Entries>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Resolved<T> {
    id: u64,
    thread: String,
    revision: u64,
    authenticated: Instant,
    checked_at: Instant,
    entries: T,
}

impl<T> Resolved<T> {
    fn matches(
        &self,
        id: u64,
        thread: &str,
        authenticated: Option<Instant>,
        events: &Events,
    ) -> bool {
        self.id == id
            && self.thread == thread
            && authenticated == Some(self.authenticated)
            && self.checked_at.elapsed() <= SELECTION_TTL
            && sticker_send::normal_scope(events, thread) == Some(self.revision)
    }
}

pub(super) struct Completion {
    request: Request,
    revision: u64,
    authenticated: Instant,
    entries: Option<Entries>,
}

impl Search {
    pub async fn start(
        &mut self,
        request: Request,
        authenticated: Option<Instant>,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        self.active = None;
        self.resolved = None;
        let allowed_action = match &request.kind {
            Kind::Search(query) => catalog_wire::valid_query(query),
            Kind::ListPacks => {
                self.pack_list = None;
                true
            }
            Kind::BrowsePack {
                list_id,
                category_id,
            } => self.pack_list.as_ref().is_some_and(|list| {
                list.matches(*list_id, &request.thread, authenticated, events)
                    && list.entries.contains(category_id)
            }),
        };
        let scope = sticker_send::normal_scope(events, &request.thread).filter(|_| {
            request.id > 0
                && allowed_action
                && Target::from_thread(&request.thread).is_ok_and(|target| {
                    target.kind == ThreadKind::Group || target.remote_id != events.account
                })
        });
        let (Some(revision), Some(authenticated), Ok(origin)) =
            (scope, authenticated, root_for(session, "sticker"))
        else {
            return request.unavailable(events).await;
        };
        let session = session.clone();
        let client = client.clone();
        let job = request.kind.clone();
        self.active = Some(Running {
            request,
            revision,
            authenticated,
            task: tokio::spawn(async move {
                tokio::time::timeout(LOOKUP_TIMEOUT, load(&client, &session, &origin, job)).await?
            }),
        });
        Ok(())
    }

    pub async fn next(&mut self) -> Completion {
        let Some(running) = &mut self.active else {
            return std::future::pending().await;
        };
        let entries = (&mut running.task).await.ok().and_then(Result::ok);
        let running = self
            .active
            .take()
            .expect("completed sticker lookup is active");
        Completion {
            request: running.request.clone(),
            revision: running.revision,
            authenticated: running.authenticated,
            entries,
        }
    }

    pub async fn complete(
        &mut self,
        found: Completion,
        authenticated: Option<Instant>,
        events: &Events,
    ) -> Result<()> {
        let allowed = authenticated == Some(found.authenticated)
            && sticker_send::normal_scope(events, &found.request.thread) == Some(found.revision);
        let entries = found.entries.filter(|_| allowed);
        let checked_at = Instant::now();
        let (id, thread, revision, authenticated) = (
            found.request.id,
            &found.request.thread,
            found.revision,
            found.authenticated,
        );
        match &entries {
            Some(Entries::Packs(packs)) => {
                self.pack_list = Some(Resolved {
                    id,
                    thread: thread.clone(),
                    revision,
                    authenticated,
                    checked_at,
                    entries: packs.iter().map(|pack| pack.category_id).collect(),
                })
            }
            Some(Entries::Stickers(stickers)) => {
                self.resolved = Some(Resolved {
                    id,
                    thread: thread.clone(),
                    revision,
                    authenticated,
                    checked_at,
                    entries: stickers.clone(),
                })
            }
            None => {}
        }
        found.request.publish(events, entries).await
    }

    pub fn permits(
        &self,
        search_id: u64,
        thread: &str,
        sticker: StickerDescriptor,
        authenticated: Option<Instant>,
        events: &Events,
    ) -> bool {
        self.resolved.as_ref().is_some_and(|found| {
            found.matches(search_id, thread, authenticated, events)
                && found.entries.contains(&sticker)
        })
    }
}

pub(super) fn authenticated_at(connection: Option<&Connection>) -> Option<Instant> {
    connection
        .filter(|connection| connection.decoder.authenticated())
        .and_then(|connection| connection.authenticated_at)
}

async fn load(
    client: &reqwest::Client,
    session: &VerifiedSession,
    origin: &Url,
    kind: Kind,
) -> Result<Entries> {
    match kind {
        Kind::Search(query) => resolve(client, session, origin, &query)
            .await
            .map(Entries::Stickers),
        Kind::ListPacks => {
            let value = get(client, session, origin, catalog_wire::PACK_LIST_PATH, b"").await?;
            Ok(Entries::Packs(catalog_wire::packs(&value)?))
        }
        Kind::BrowsePack { category_id, .. } => {
            let value = lookup(
                client,
                session,
                origin,
                catalog_wire::PACK_DETAIL_PATH,
                catalog_wire::pack_parameters(category_id)?,
            )
            .await?;
            Ok(Entries::Stickers(catalog_wire::pack_stickers(
                &value,
                category_id,
            )?))
        }
    }
}

async fn lookup(
    client: &reqwest::Client,
    session: &VerifiedSession,
    origin: &Url,
    path: &'static str,
    params: Value,
) -> Result<Value> {
    get(client, session, origin, path, &serde_json::to_vec(&params)?).await
}

async fn get(
    client: &reqwest::Client,
    session: &VerifiedSession,
    origin: &Url,
    path: &'static str,
    plaintext: &[u8],
) -> Result<Value> {
    let params = session.encrypt_payload(plaintext)?;
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
