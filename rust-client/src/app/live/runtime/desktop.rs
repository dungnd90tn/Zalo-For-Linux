//! Best-effort desktop delivery. D-Bus calls live in their own bounded task.
#[cfg(target_os = "linux")]
pub(super) use linux::DesktopNotifications;

#[cfg(not(target_os = "linux"))]
pub(super) struct DesktopNotifications;

#[cfg(not(target_os = "linux"))]
impl DesktopNotifications {
    pub(super) fn spawn() -> Self {
        Self
    }
    pub(super) fn notify(&self, _: &str, _: Option<&str>) {}
    pub(super) fn close_thread(&self, _: &str) {}
    pub(super) fn clear(&self) {}
    pub(super) async fn shutdown(self) {}
    pub(super) async fn wait_activation(&self) {
        std::future::pending::<()>().await;
    }
    pub(super) fn take_activated(&self) -> Option<String> {
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use futures_util::StreamExt;
    use std::{
        collections::{HashMap, VecDeque},
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::{sync::Notify, time::timeout};
    use zbus::{zvariant::Value, Connection, Proxy};

    const SERVICE: &str = "org.freedesktop.Notifications";
    const PATH: &str = "/org/freedesktop/Notifications";
    const APP: &str = "Zalo Native";
    const ICON: &str = "zalo-linux-rust-preview";
    const GENERIC: &str = "Bạn có tin nhắn mới";
    const CALL_TIMEOUT: Duration = Duration::from_secs(2);
    const CLOSE_TIMEOUT: Duration = Duration::from_millis(250);
    const CAPACITY: usize = 32;
    const PREVIEW_CHARS: usize = 128;

    enum Request {
        Show {
            thread: String,
            preview: Option<String>,
        },
        Close(String),
        Clear,
        Stop,
    }

    #[derive(Default)]
    struct State {
        queue: VecDeque<Request>,
        activated: VecDeque<String>,
        clear: bool,
        stop: bool,
    }

    impl State {
        fn next(&mut self) -> Option<Request> {
            if self.stop {
                Some(Request::Stop)
            } else if std::mem::take(&mut self.clear) {
                Some(Request::Clear)
            } else {
                self.queue.pop_front()
            }
        }

        fn cancelled(&self, thread: &str) -> bool {
            self.stop
                || self.clear
                || self
                    .queue
                    .iter()
                    .any(|request| matches!(request, Request::Close(target) if target == thread))
        }

        fn invalidate_all(&mut self) {
            self.queue.clear();
            self.activated.clear();
            self.clear = true;
        }
    }

    #[derive(Default)]
    struct Mailbox {
        state: Mutex<State>,
        ready: Notify,
        activation: Notify,
    }

    impl Mailbox {
        async fn next(&self) -> Request {
            loop {
                if let Some(request) = self.state.lock().unwrap().next() {
                    return request;
                }
                self.ready.notified().await;
            }
        }

        fn cancelled(&self, thread: &str) -> bool {
            self.state.lock().unwrap().cancelled(thread)
        }

        fn activate(&self, thread: &str) {
            let mut state = self.state.lock().unwrap();
            if state.cancelled(thread)
                || state.activated.len() >= CAPACITY
                || state.activated.iter().any(|id| id == thread)
            {
                return;
            }
            state.activated.push_back(thread.into());
            self.activation.notify_one();
        }
    }

    pub(in super::super) struct DesktopNotifications {
        mailbox: Arc<Mailbox>,
        task: Option<tokio::task::JoinHandle<()>>,
    }

    impl DesktopNotifications {
        pub(in super::super) fn spawn() -> Self {
            let mailbox = Arc::new(Mailbox::default());
            let task = tokio::spawn(worker(mailbox.clone(), Connection::session));
            Self {
                mailbox,
                task: Some(task),
            }
        }

        pub(in super::super) fn notify(&self, thread: &str, preview: Option<&str>) {
            let mut state = self.mailbox.state.lock().unwrap();
            state.queue.retain(
                |request| !matches!(request, Request::Show {thread: old, ..} if old == thread),
            );
            if state.queue.len() < CAPACITY {
                state.queue.push_back(Request::Show {
                    thread: thread.into(),
                    preview: preview.and_then(clean_preview),
                });
            }
            self.mailbox.ready.notify_one();
        }

        pub(in super::super) fn close_thread(&self, thread: &str) {
            let mut state = self.mailbox.state.lock().unwrap();
            state.queue.retain(|request| match request {
                Request::Show { thread: target, .. } | Request::Close(target) => target != thread,
                _ => true,
            });
            state.activated.retain(|target| target != thread);
            if state.queue.len() == CAPACITY {
                state.invalidate_all();
            } else {
                state.queue.push_back(Request::Close(thread.into()));
            }
            self.mailbox.ready.notify_one();
        }

        pub(in super::super) fn clear(&self) {
            self.mailbox.state.lock().unwrap().invalidate_all();
            self.mailbox.ready.notify_one();
        }

        pub(in super::super) async fn wait_activation(&self) {
            self.mailbox.activation.notified().await;
        }

        pub(in super::super) fn take_activated(&self) -> Option<String> {
            let mut state = self.mailbox.state.lock().unwrap();
            let next = state.activated.pop_front();
            if !state.activated.is_empty() {
                self.mailbox.activation.notify_one();
            }
            next
        }
        pub(in super::super) async fn shutdown(mut self) {
            self.stop();
            if let Some(task) = self.task.take() {
                let _ = timeout(Duration::from_millis(400), task).await;
            }
        }

        fn stop(&self) {
            let mut state = self.mailbox.state.lock().unwrap();
            state.invalidate_all();
            state.stop = true;
            self.mailbox.ready.notify_one();
        }
    }

    impl Drop for DesktopNotifications {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn clean_preview(text: &str) -> Option<String> {
        let text: String = text
            .chars()
            .take(PREVIEW_CHARS)
            .map(|ch| match ch {
                '<' => '‹',
                '>' => '›',
                '&' => '＆',
                ch if ch.is_control()
                    || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
                {
                    ' '
                }
                ch => ch,
            })
            .collect();
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    async fn worker<F: std::future::Future<Output = zbus::Result<Connection>>>(
        mailbox: Arc<Mailbox>,
        connect: impl Fn() -> F,
    ) {
        // Connect only when a candidate exists. A missing daemon must not
        // cause continuous connection retries or repeated diagnostics.
        let mut retry_after = tokio::time::Instant::now();
        loop {
            let first = match mailbox.next().await {
                request @ Request::Show { .. } if tokio::time::Instant::now() >= retry_after => {
                    request
                }
                Request::Stop => return,
                _ => continue,
            };
            if run(&mailbox, connect(), first).await.is_some() {
                return;
            }
            use crate::diagnostics::{record, Event, Kind, Reason, Stage};
            record(Event::new(
                Kind::Failure,
                Stage::Notifications,
                Reason::Transport,
            ));
            mailbox.state.lock().unwrap().invalidate_all();
            retry_after = tokio::time::Instant::now() + Duration::from_secs(30);
        }
    }

    async fn run(
        mailbox: &Mailbox,
        connect: impl std::future::Future<Output = zbus::Result<Connection>>,
        first: Request,
    ) -> Option<()> {
        let connection = timeout(CALL_TIMEOUT, connect).await.ok()?.ok()?;
        // Bind methods and signals to one daemon instance. IDs from a restarted
        // daemon cannot alias notifications assigned by the previous owner.
        let bus = timeout(CALL_TIMEOUT, zbus::fdo::DBusProxy::new(&connection))
            .await
            .ok()?
            .ok()?;
        let name = zbus::names::BusName::try_from(SERVICE).ok()?;
        let owner = timeout(CALL_TIMEOUT, bus.get_name_owner(name.clone()))
            .await
            .ok()?;
        let owner = match owner {
            Ok(owner) => owner,
            Err(_) => {
                let _ = timeout(
                    CALL_TIMEOUT,
                    bus.start_service_by_name(
                        zbus::names::WellKnownName::try_from(SERVICE).ok()?,
                        0,
                    ),
                )
                .await
                .ok()?
                .ok()?;
                timeout(CALL_TIMEOUT, bus.get_name_owner(name))
                    .await
                    .ok()?
                    .ok()?
            }
        };
        let proxy = timeout(
            CALL_TIMEOUT,
            Proxy::new(&connection, owner.as_str(), PATH, SERVICE),
        )
        .await
        .ok()?
        .ok()?;
        let mut invoked = timeout(CALL_TIMEOUT, proxy.receive_signal("ActionInvoked"))
            .await
            .ok()?
            .ok()?;
        let mut closed = timeout(CALL_TIMEOUT, proxy.receive_signal("NotificationClosed"))
            .await
            .ok()?
            .ok()?;
        let mut owner_changed = timeout(CALL_TIMEOUT, proxy.receive_owner_changed())
            .await
            .ok()?
            .ok()?;
        let caps: Vec<String> = timeout(CALL_TIMEOUT, proxy.call("GetCapabilities", &()))
            .await
            .ok()?
            .ok()?;
        let actions = if caps.iter().any(|cap| cap == "actions") {
            &["default", "Mở hội thoại"][..]
        } else {
            &[][..]
        };

        let mut active = lru::LruCache::<String, u32>::new(CAPACITY.try_into().unwrap());
        let mut request = first;
        loop {
            match request {
                Request::Show { thread, preview } if !mailbox.cancelled(&thread) => {
                    let replacement = active.peek(&thread).copied().unwrap_or(0);
                    let hints = HashMap::from([
                        ("desktop-entry", Value::from(ICON)),
                        ("transient", Value::from(true)),
                    ]);
                    let id: Option<u32> = timeout(
                        CALL_TIMEOUT,
                        proxy.call(
                            "Notify",
                            &(
                                APP,
                                replacement,
                                ICON,
                                GENERIC,
                                preview.as_deref().unwrap_or(""),
                                actions,
                                hints,
                                5000i32,
                            ),
                        ),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok);
                    if let Some(id) = id.filter(|id| *id != 0) {
                        if mailbox.cancelled(&thread) {
                            active.pop(&thread);
                            close(&proxy, id).await;
                        } else if let Some((_, evicted)) = active.push(thread, id) {
                            if id != evicted {
                                close(&proxy, evicted).await;
                            }
                        }
                    }
                }
                Request::Close(thread) => {
                    if let Some(id) = active.pop(&thread) {
                        close(&proxy, id).await;
                    }
                }
                Request::Clear | Request::Stop => {
                    let stop = matches!(request, Request::Stop);
                    let ids: Vec<_> = active.iter().map(|(_, id)| *id).collect();
                    active.clear();
                    futures_util::future::join_all(ids.into_iter().map(|id| close(&proxy, id)))
                        .await;
                    if stop {
                        return Some(());
                    }
                }
                Request::Show { .. } => {}
            }
            request = loop {
                tokio::select! {
                    next = mailbox.next() => break next,
                    _ = owner_changed.next() => return None,
                    signal = invoked.next() => {
                        let signal = signal?;
                        if let Ok((id, "default")) = signal.body().deserialize::<(u32, &str)>() {
                            if let Some((thread, _)) = active.iter().find(|(_, current)| **current == id) {
                                mailbox.activate(thread);
                            }
                        }
                    }
                    signal = closed.next() => {
                        let signal = signal?;
                        if let Ok((id, _)) = signal.body().deserialize::<(u32, u32)>() {
                            let thread = active.iter().find(|(_, current)| **current == id)
                                .map(|(thread, _)| thread.clone());
                            if let Some(thread) = thread { active.pop(&thread); }
                        }
                    }
                }
            };
        }
    }

    async fn close(proxy: &Proxy<'_>, id: u32) {
        let _ = timeout(
            CLOSE_TIMEOUT,
            proxy.call::<_, _, ()>("CloseNotification", &(id,)),
        )
        .await;
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct Daemon(std::process::Child);

        impl Drop for Daemon {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        #[derive(Debug)]
        struct Show {
            app: String,
            replacement: u32,
            icon: String,
            summary: String,
            body: String,
            actions: Vec<String>,
            desktop_entry: String,
        }

        struct FakeServer {
            shown: tokio::sync::mpsc::Sender<Show>,
            closed: tokio::sync::mpsc::Sender<u32>,
        }

        #[zbus::interface(name = "org.freedesktop.Notifications")]
        impl FakeServer {
            fn get_capabilities(&self) -> Vec<&str> {
                vec!["actions", "body", "body-markup"]
            }

            #[allow(clippy::too_many_arguments)]
            fn notify(
                &self,
                app: String,
                replacement: u32,
                icon: String,
                summary: String,
                body: String,
                actions: Vec<String>,
                hints: HashMap<&str, Value<'_>>,
                _expire: i32,
            ) -> u32 {
                let desktop_entry = hints["desktop-entry"]
                    .downcast_ref::<&str>()
                    .unwrap()
                    .into();
                self.shown
                    .try_send(Show {
                        app,
                        replacement,
                        icon,
                        summary,
                        body,
                        actions,
                        desktop_entry,
                    })
                    .unwrap();
                42
            }

            fn close_notification(&self, id: u32) {
                self.closed.try_send(id).unwrap();
            }
        }

        #[tokio::test]
        #[ignore = "requires private D-Bus daemon and UNIX sockets"]
        async fn private_bus_generic_replacement_activation_and_clear() {
            use std::{
                io::BufRead,
                process::{Command, Stdio},
            };

            let mut daemon = Daemon(
                Command::new("dbus-daemon")
                    .args([
                        "--session",
                        "--nofork",
                        "--print-address=1",
                        "--address=unix:tmpdir=/tmp",
                    ])
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let mut address = String::new();
            std::io::BufReader::new(daemon.0.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            let address = address.trim();
            assert!(address.starts_with("unix:"), "private bus failed to start");

            let (shown_tx, mut shown) = tokio::sync::mpsc::channel(4);
            let (closed_tx, mut closed) = tokio::sync::mpsc::channel(4);
            let server = zbus::connection::Builder::address(address)
                .unwrap()
                .name(SERVICE)
                .unwrap()
                .serve_at(
                    PATH,
                    FakeServer {
                        shown: shown_tx,
                        closed: closed_tx,
                    },
                )
                .unwrap()
                .build()
                .await
                .unwrap();

            let mailbox = Arc::new(Mailbox::default());
            let task = tokio::spawn(worker(mailbox.clone(), {
                let address = address.to_owned();
                move || {
                    let address = address.clone();
                    async move {
                        zbus::connection::Builder::address(address.as_str())?
                            .build()
                            .await
                    }
                }
            }));
            let client = DesktopNotifications {
                mailbox,
                task: Some(task),
            };

            client.notify("d:12345", None);
            let generic = timeout(CALL_TIMEOUT, shown.recv()).await.unwrap().unwrap();
            assert_eq!(generic.app, APP);
            assert_eq!(generic.replacement, 0);
            assert_eq!(generic.icon, ICON);
            assert_eq!(generic.summary, GENERIC);
            assert!(generic.body.is_empty());
            assert_eq!(generic.actions, ["default", "Mở hội thoại"]);
            assert_eq!(generic.desktop_entry, ICON);

            client.notify("d:12345", Some("<b>synthetic</b>"));
            let preview = timeout(CALL_TIMEOUT, shown.recv()).await.unwrap().unwrap();
            assert_eq!(preview.replacement, 42);
            assert_eq!(preview.body, "‹b›synthetic‹/b›");

            server
                .emit_signal(
                    None::<&str>,
                    PATH,
                    SERVICE,
                    "ActionInvoked",
                    &(42u32, "default"),
                )
                .await
                .unwrap();
            timeout(CALL_TIMEOUT, client.wait_activation())
                .await
                .unwrap();
            assert_eq!(client.take_activated().as_deref(), Some("d:12345"));

            client.clear();
            assert_eq!(
                timeout(CALL_TIMEOUT, closed.recv()).await.unwrap(),
                Some(42)
            );
            server
                .emit_signal(
                    None::<&str>,
                    PATH,
                    SERVICE,
                    "ActionInvoked",
                    &(42u32, "default"),
                )
                .await
                .unwrap();
            assert!(
                timeout(Duration::from_millis(100), client.wait_activation())
                    .await
                    .is_err()
            );
            assert!(client.take_activated().is_none());
            client.shutdown().await;
        }

        #[test]
        fn preview_has_no_markup_ctrl_or_bidi_and_is_bounded() {
            assert_eq!(
                clean_preview("<b>Hi</b>\u{202e}\0&").as_deref(),
                Some("‹b›Hi‹/b›  ＆")
            );
            assert_eq!(
                clean_preview(&"ộ".repeat(500)).unwrap().chars().count(),
                PREVIEW_CHARS
            );
            assert!(clean_preview("\r\n\0").is_none());
        }
    }
}
