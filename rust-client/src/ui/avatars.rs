//! Visible-only identity photos. Metadata and pixels remain account/policy scoped.
use crate::app::{
    live::{LiveCommand, LiveEvent, LiveWorker, MAX_AVATARS},
    media::{MediaEvent, MediaKey, MediaLoader},
};
use eframe::egui;
use lru::LruCache;
use std::{
    collections::BTreeSet,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const TEXTURES: usize = 64;
const BYTES: usize = 4 * 1024 * 1024;
const METADATA: usize = 256;
const RETRY: Duration = Duration::from_secs(30);
static NEXT_QUERY: AtomicU64 = AtomicU64::new(1);

struct Texture {
    handle: egui::TextureHandle,
    bytes: usize,
}
enum State {
    Pending,
    Retry(Instant),
}

pub(super) struct Avatars {
    account: String,
    revision: u64,
    urls: LruCache<String, Option<String>>,
    wanted: BTreeSet<String>,
    in_flight: Option<(u64, Vec<String>)>,
    due: Option<Instant>,
    loader: Option<MediaLoader>,
    loader_retry: Option<Instant>,
    textures: LruCache<MediaKey, Texture>,
    states: LruCache<MediaKey, State>,
    bytes: usize,
    #[cfg(test)]
    fixture_only: bool,
}
impl Default for Avatars {
    fn default() -> Self {
        Self {
            account: String::new(),
            revision: 0,
            urls: LruCache::new(NonZeroUsize::new(METADATA).unwrap()),
            wanted: BTreeSet::new(),
            in_flight: None,
            due: None,
            loader: None,
            loader_retry: None,
            textures: LruCache::new(NonZeroUsize::new(TEXTURES).unwrap()),
            states: LruCache::new(NonZeroUsize::new(TEXTURES).unwrap()),
            bytes: 0,
            #[cfg(test)]
            fixture_only: false,
        }
    }
}
impl Avatars {
    pub fn scope(&mut self, account: &str, revision: u64) {
        if self.account != account || self.revision != revision {
            *self = Self::default();
            self.account = account.into();
            self.revision = revision;
        }
    }
    pub fn invalidate_metadata(&mut self) {
        // A directory refresh may change a photo without changing the thread ID.
        self.urls.clear();
        self.in_flight = None;
        self.wanted.clear();
    }
    pub fn visible(&mut self, ctx: &egui::Context, identity: &str) -> Option<egui::TextureHandle> {
        let valid_identity = identity.split_once(':').is_some_and(|(kind, id)| {
            matches!(kind, "d" | "g")
                && !id.is_empty()
                && id.len() <= 24
                && !id.starts_with('0')
                && id.bytes().all(|byte| byte.is_ascii_digit())
        });
        if self.account.is_empty() || !valid_identity {
            return None;
        }
        let url = match self.urls.get(identity) {
            Some(Some(url)) => url.clone(),
            Some(None) => return None,
            None => {
                #[cfg(test)]
                if self.fixture_only {
                    return None;
                }
                if self
                    .in_flight
                    .as_ref()
                    .is_some_and(|(_, ids)| ids.iter().any(|id| id == identity))
                {
                    return None;
                }
                if self.wanted.len() < MAX_AVATARS {
                    self.wanted.insert(identity.into());
                    if self.due.is_none() {
                        self.due = Some(Instant::now() + Duration::from_millis(40));
                    }
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
                return None;
            }
        };
        let key = self.key(identity, url);
        if let Some(texture) = self.textures.get(&key) {
            return Some(texture.handle.clone());
        }
        match self.states.get(&key) {
            Some(State::Pending) => return None,
            Some(State::Retry(until)) if Instant::now() < *until => {
                ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
                return None;
            }
            _ => {}
        }
        #[cfg(test)]
        if self.fixture_only {
            return None;
        }
        if self.loader.as_ref().is_some_and(MediaLoader::is_closed) {
            self.loader = None;
            self.states.clear();
            self.loader_retry = Some(Instant::now() + RETRY);
        }
        if self.loader.is_none() {
            if let Some(until) = self.loader_retry.filter(|until| Instant::now() < *until) {
                ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
                return None;
            }
            let repaint = ctx.clone();
            match MediaLoader::new_avatar(
                self.account.clone(),
                Arc::new(move || repaint.request_repaint()),
            ) {
                Ok(loader) => {
                    self.loader = Some(loader);
                    self.loader_retry = None;
                }
                Err(_) => {
                    self.loader_retry = Some(Instant::now() + RETRY);
                    ctx.request_repaint_after(RETRY);
                    return None;
                }
            }
        }
        if self
            .loader
            .as_mut()
            .is_some_and(|loader| loader.request(key.clone(), self.revision))
        {
            self.states.put(key, State::Pending);
        } else {
            let retry = Duration::from_millis(250);
            self.states.put(key, State::Retry(Instant::now() + retry));
            ctx.request_repaint_after(retry);
        }
        None
    }
    fn key(&self, identity: &str, url: String) -> MediaKey {
        MediaKey {
            account_id: self.account.clone(),
            thread_id: identity.into(),
            message_id: "avatar".into(),
            url,
        }
    }
    pub fn poll(
        &mut self,
        worker: Option<&LiveWorker>,
        ctx: &egui::Context,
        deadline: Instant,
        remaining: &mut usize,
    ) {
        if self.in_flight.is_none()
            && self.due.is_some_and(|due| due <= Instant::now())
            && !self.wanted.is_empty()
        {
            let generation = NEXT_QUERY.fetch_add(1, Ordering::Relaxed);
            let ids: Vec<_> = self.wanted.iter().take(MAX_AVATARS).cloned().collect();
            if worker.is_some_and(|worker| {
                worker
                    .submit(LiveCommand::ResolveAvatars {
                        generation,
                        thread_ids: ids.clone(),
                    })
                    .is_ok()
            }) {
                for id in &ids {
                    self.wanted.remove(id);
                }
                self.in_flight = Some((generation, ids));
                self.due = None;
            } else {
                self.due = Some(Instant::now() + Duration::from_millis(100));
            }
        }
        if let Some(due) = self
            .due
            .filter(|due| self.in_flight.is_none() || *due > Instant::now())
        {
            ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
        }
        while *remaining > 0 && Instant::now() < deadline {
            let Some(event) = self.loader.as_mut().and_then(MediaLoader::try_event) else {
                break;
            };
            *remaining -= 1;
            self.apply_image(ctx, event);
            ctx.request_repaint();
        }
    }
    pub fn apply(&mut self, event: &LiveEvent) {
        let LiveEvent::Avatars {
            account_id,
            generation,
            avatars,
        } = event
        else {
            return;
        };
        let Some((pending, ids)) = self.in_flight.as_ref() else {
            return;
        };
        if account_id != &self.account || generation != pending || avatars.len() > MAX_AVATARS {
            return;
        }
        for id in ids {
            let url = avatars
                .iter()
                .find(|avatar| &avatar.thread_id == id)
                .map(|avatar| avatar.url.clone());
            self.urls.put(id.clone(), url);
        }
        self.in_flight = None;
        if !self.wanted.is_empty() {
            self.due = Some(Instant::now());
        }
    }
    fn apply_image(&mut self, ctx: &egui::Context, event: MediaEvent) {
        let (key, generation) = match &event {
            MediaEvent::Ready {
                key, generation, ..
            }
            | MediaEvent::Failed {
                key, generation, ..
            } => (key, generation),
        };
        if key.account_id != self.account
            || *generation != self.revision
            || key.message_id != "avatar"
            || self.urls.peek(&key.thread_id).and_then(|url| url.as_ref()) != Some(&key.url)
        {
            return;
        }
        match event {
            MediaEvent::Failed { key, .. } => {
                self.states.put(key, State::Retry(Instant::now() + RETRY));
            }
            MediaEvent::Ready {
                key,
                width,
                height,
                rgba,
                ..
            } => {
                let bytes = (width as usize)
                    .checked_mul(height as usize)
                    .and_then(|n| n.checked_mul(4));
                let Some(bytes) = bytes.filter(|bytes| {
                    width > 0
                        && height > 0
                        && width <= 256
                        && height <= 256
                        && *bytes == rgba.len()
                        && *bytes <= BYTES
                }) else {
                    self.states.put(key, State::Retry(Instant::now() + RETRY));
                    return;
                };
                if let Some(old) = self.textures.pop(&key) {
                    self.bytes -= old.bytes;
                }
                while self.textures.len() >= TEXTURES || self.bytes.saturating_add(bytes) > BYTES {
                    let Some((_, texture)) = self.textures.pop_lru() else {
                        break;
                    };
                    self.bytes -= texture.bytes;
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &rgba,
                );
                let handle =
                    ctx.load_texture("zalo-profile-avatar", image, egui::TextureOptions::LINEAR);
                self.textures.put(key.clone(), Texture { handle, bytes });
                self.bytes += bytes;
                self.states.pop(&key);
            }
        }
    }
    #[cfg(test)]
    pub fn seed(
        &mut self,
        ctx: &egui::Context,
        account: &str,
        revision: u64,
        identity: &str,
        rgba: Vec<u8>,
        side: u32,
    ) {
        self.scope(account, revision);
        self.fixture_only = true;
        let url = format!("https://avatar.talk.zdn.vn/synthetic-{identity}.png");
        self.urls.put(identity.into(), Some(url.clone()));
        self.apply_image(
            ctx,
            MediaEvent::Ready {
                key: self.key(identity, url),
                generation: revision,
                width: side,
                height: side,
                rgba: Arc::new(rgba),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::live::AvatarIdentity;
    fn ready(cache: &Avatars, identity: &str, generation: u64, color: u8) -> MediaEvent {
        MediaEvent::Ready {
            key: cache.key(
                identity,
                cache.urls.peek(identity).unwrap().clone().unwrap(),
            ),
            generation,
            width: 2,
            height: 2,
            rgba: Arc::new(vec![color; 16]),
        }
    }
    #[test]
    fn metadata_replies_require_current_account_request_and_requested_identity() {
        let mut cache = Avatars::default();
        cache.scope("11", 7);
        cache.in_flight = Some((42, vec!["d:22".into(), "g:22".into()]));
        let reply = |account: &str, generation| LiveEvent::Avatars {
            account_id: account.into(),
            generation,
            avatars: vec![
                AvatarIdentity {
                    thread_id: "d:22".into(),
                    url: "https://avatar.talk.zdn.vn/direct.png".into(),
                },
                AvatarIdentity {
                    thread_id: "g:22".into(),
                    url: "https://avatar.talk.zdn.vn/group.png".into(),
                },
                AvatarIdentity {
                    thread_id: "d:99".into(),
                    url: "https://avatar.talk.zdn.vn/unrequested.png".into(),
                },
            ],
        };
        cache.apply(&reply("other", 42));
        cache.apply(&reply("11", 41));
        assert!(cache.urls.is_empty());
        assert!(cache.in_flight.is_some());
        cache.apply(&reply("11", 42));
        assert!(cache.in_flight.is_none());
        assert!(cache
            .urls
            .peek("d:22")
            .unwrap()
            .as_deref()
            .unwrap()
            .ends_with("direct.png"));
        assert!(cache
            .urls
            .peek("g:22")
            .unwrap()
            .as_deref()
            .unwrap()
            .ends_with("group.png"));
        assert!(cache.urls.peek("d:99").is_none());
        cache.in_flight = Some((43, vec!["d:33".into()]));
        cache.apply(&LiveEvent::Avatars {
            account_id: "11".into(),
            generation: 43,
            avatars: vec![],
        });
        assert!(cache.in_flight.is_none());
        assert_eq!(cache.urls.peek("d:33"), Some(&None));
        cache.in_flight = Some((44, vec!["d:44".into()]));
        cache.invalidate_metadata();
        cache.apply(&reply("11", 44));
        assert!(
            cache.urls.is_empty(),
            "A late pre-refresh query must not restore old URLs"
        );
    }
    #[test]
    fn account_and_privacy_changes_drop_pixels_and_stale_images_cannot_restore_them() {
        let ctx = egui::Context::default();
        let mut cache = Avatars::default();
        cache.seed(&ctx, "11", 7, "d:22", vec![20; 16], 2);
        cache.seed(&ctx, "11", 7, "g:22", vec![80; 16], 2);
        let direct = cache.visible(&ctx, "d:22").unwrap().id();
        let group = cache.visible(&ctx, "g:22").unwrap().id();
        assert_ne!(
            direct, group,
            "A group's photo must never replace a member with the same numeric ID"
        );
        let late = ready(&cache, "g:22", 7, 30);
        cache.scope("11", 8);
        cache.apply_image(&ctx, late);
        assert!(cache.textures.is_empty());
        assert_eq!(cache.bytes, 0);
        cache.seed(&ctx, "11", 8, "d:22", vec![40; 16], 2);
        let late = ready(&cache, "d:22", 8, 90);
        cache.scope("12", 8);
        cache.apply_image(&ctx, late);
        assert!(cache.textures.is_empty());
        assert_eq!(cache.bytes, 0);
        assert!(cache.urls.is_empty());
        assert!(cache.loader.is_none());
    }
    #[test]
    fn queued_metadata_is_bounded_and_backpressure_uses_a_timed_retry() {
        let ctx = egui::Context::default();
        let mut cache = Avatars::default();
        cache.scope("11", 1);
        for invalid in [
            "d:0",
            "g:012",
            "d:Demo guide",
            "other:22",
            "d:9999999999999999999999999",
        ] {
            assert!(cache.visible(&ctx, invalid).is_none());
        }
        assert!(
            cache.wanted.is_empty(),
            "An invalid system sender must not poison a batch of valid identities"
        );
        for id in 1..101 {
            assert!(cache.visible(&ctx, &format!("d:{id}")).is_none());
        }
        assert_eq!(cache.wanted.len(), MAX_AVATARS);
        cache.due = Some(Instant::now());
        let mut remaining = 1;
        cache.poll(
            None,
            &ctx,
            Instant::now() + Duration::from_millis(3),
            &mut remaining,
        );
        assert!(cache.due.unwrap() > Instant::now());
        assert!(cache.in_flight.is_none());
        cache.in_flight = Some((99, vec!["d:500".into()]));
        cache.wanted.clear();
        cache.visible(&ctx, "d:500");
        assert!(
            cache.wanted.is_empty(),
            "An in-flight identity must not enqueue duplicate metadata work"
        );
    }
}
