//! Opt-in native presentation fixture. No production account, endpoint or verifier.
use super::*;
use crate::app::live::{
    HiddenCandidate, HiddenPin, HiddenUnlockOutcome, LiveCommand, LiveEvent, LivePrivacyStatus,
    LiveWorker,
};
use std::time::{Duration, Instant};

const PRIVATE_TITLE: &str = "Nguyễn Thị Ánh · hội thoại ẩn";
const PRIVATE_BODY: &str = "Nội dung riêng chỉ hiển thị sau khi xác minh mã PIN.";

fn conversation(thread: &str) -> Conversation {
    let private = thread == "d:22";
    Conversation {
        id: thread.into(),
        title: if private {
            PRIVATE_TITLE
        } else {
            "Hội thoại kiểm thử thường"
        }
        .into(),
        last_message_at_ms: 1_700_000_000_000,
        last_text: if private {
            PRIVATE_BODY
        } else {
            "Tin nhắn công khai trong bộ kiểm thử."
        }
        .into(),
        last_sender: "44".into(),
    }
}
fn message(thread: &str) -> Message {
    Message {
        account_id: "11".into(),
        thread_id: thread.into(),
        id: format!("synthetic-{thread}"),
        sender: if thread == "d:22" { "22" } else { "44" }.into(),
        text: if thread == "d:22" {
            PRIVATE_BODY
        } else {
            "Tin nhắn công khai trong bộ kiểm thử."
        }
        .into(),
        sticker: None,
        media: None,
        sent_at_ms: 1_700_000_000_000,
    }
}

struct Pending {
    generation: u64,
    revision: u64,
    thread: String,
    pin: HiddenPin,
    due: Instant,
}
struct PresentationFixture {
    app: NativeApp,
    commands: tokio::sync::mpsc::Receiver<LiveCommand>,
    _events: tokio::sync::mpsc::Sender<LiveEvent>,
    revision: u64,
    unlocked: Option<String>,
    pending: Option<Pending>,
    phase: LivePrivacyStatus,
    geometry: Option<[f32; 4]>,
    state_trace: Option<[bool; 6]>,
}
impl PresentationFixture {
    fn apply(&mut self, ctx: &egui::Context, event: LiveEvent) {
        let mut state = self.app.live.take().unwrap();
        self.app.apply_live_event(&mut state, event, ctx);
        self.app.live = Some(state);
    }
    fn policy(&mut self, ctx: &egui::Context) {
        self.apply(
            ctx,
            LiveEvent::PrivacyChanged {
                account_id: "11".into(),
                revision: self.revision,
                status: self.phase,
                hidden_threads: vec!["d:22".into()],
                unlocked_thread: self.unlocked.clone(),
            },
        );
    }
    fn commands(&mut self, ctx: &egui::Context) {
        // The same limits as production channels; no worker or network is started.
        for _ in 0..16 {
            let Ok(command) = self.commands.try_recv() else {
                break;
            };
            match command {
                LiveCommand::SearchHidden { generation, query } => {
                    let folded = fold_search(&query);
                    let candidates = if self.phase == LivePrivacyStatus::Ready
                        && !folded.is_empty()
                        && (fold_search(PRIVATE_TITLE).contains(&folded)
                            || "d:22".contains(&folded))
                    {
                        vec![HiddenCandidate {
                            thread_id: "d:22".into(),
                            title: PRIVATE_TITLE.into(),
                        }]
                    } else {
                        vec![]
                    };
                    self.apply(
                        ctx,
                        LiveEvent::HiddenMatches {
                            account_id: "11".into(),
                            generation,
                            revision: self.revision,
                            query,
                            candidates,
                        },
                    );
                }
                LiveCommand::UnlockHidden {
                    generation,
                    revision,
                    thread,
                    pin,
                } => {
                    self.pending = Some(Pending {
                        generation,
                        revision,
                        thread,
                        pin,
                        due: Instant::now() + Duration::from_millis(1500),
                    });
                    ctx.request_repaint_after(Duration::from_millis(1500));
                }
                LiveCommand::RelockHidden => {
                    self.pending = None;
                    if self.unlocked.take().is_some() {
                        self.revision += 1;
                        self.policy(ctx);
                    }
                }
                LiveCommand::Open {
                    generation,
                    kind,
                    remote_id,
                } => {
                    let target = crate::protocol::messaging::Target::new(kind, &remote_id).unwrap();
                    let thread = target.thread_id();
                    if thread == "d:44" || self.unlocked.as_deref() == Some(thread.as_str()) {
                        self.apply(
                            ctx,
                            LiveEvent::Opened {
                                account_id: "11".into(),
                                generation,
                                conversation: conversation(&thread),
                            },
                        );
                    }
                }
                LiveCommand::Load {
                    generation, thread, ..
                } => {
                    if self.phase == LivePrivacyStatus::Ready
                        && (thread == "d:44" || self.unlocked.as_deref() == Some(thread.as_str()))
                    {
                        // Use the actual Load generation, including the increment after Opened.
                        self.apply(
                            ctx,
                            LiveEvent::Page {
                                account_id: "11".into(),
                                generation,
                                messages: vec![message(&thread)],
                                thread,
                                has_older: false,
                            },
                        );
                    }
                }
                LiveCommand::Directory {
                    generation,
                    is_group,
                    query,
                    ..
                } => {
                    self.apply(
                        ctx,
                        LiveEvent::DirectoryPage {
                            account_id: "11".into(),
                            generation,
                            is_group,
                            query,
                            entries: vec![],
                            next: None,
                        },
                    );
                }
                LiveCommand::ResolveSenders {
                    generation, thread, ..
                } => {
                    self.apply(
                        ctx,
                        LiveEvent::SenderNames {
                            account_id: "11".into(),
                            generation,
                            thread,
                            names: vec![],
                        },
                    );
                }
                LiveCommand::Send {
                    generation,
                    thread,
                    request_id,
                    ..
                } => {
                    self.apply(
                        ctx,
                        LiveEvent::SendFailed {
                            account_id: "11".into(),
                            generation,
                            thread,
                            request_id,
                            error: "Bộ kiểm thử giao diện không gửi tin.".into(),
                            unknown_delivery: false,
                            server_accepted: false,
                        },
                    );
                }
                // Refreshes are stable in this presentation fixture; real HTTP,
                // failure and unchanged-policy behavior have independent TLS tests.
                _ => {}
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|request| request.due <= Instant::now())
        {
            let request = self.pending.take().unwrap();
            let accepted = self.phase == LivePrivacyStatus::Ready
                && request.revision == self.revision
                && request.thread == "d:22"
                && request.pin.digest() == HiddenPin::new("1234").unwrap().digest();
            if accepted {
                self.unlocked = Some(request.thread.clone());
                self.revision += 1;
                self.policy(ctx);
            }
            self.apply(
                ctx,
                LiveEvent::HiddenUnlockResult {
                    account_id: "11".into(),
                    generation: request.generation,
                    revision: self.revision,
                    thread: request.thread.clone(),
                    outcome: if accepted {
                        HiddenUnlockOutcome::Unlocked
                    } else {
                        HiddenUnlockOutcome::IncorrectPin
                    },
                },
            );
            if accepted {
                self.apply(
                    ctx,
                    LiveEvent::Opened {
                        account_id: "11".into(),
                        generation: request.generation,
                        conversation: conversation(&request.thread),
                    },
                );
            }
        }
    }
}
impl eframe::App for PresentationFixture {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let screen = ctx.screen_rect();
        let geometry = [
            ctx.input(|input| input.viewport().native_pixels_per_point.unwrap_or(1.0)),
            ctx.pixels_per_point(),
            screen.width(),
            screen.height(),
        ];
        if self.geometry != Some(geometry) {
            eprintln!(
                "UI_FIXTURE_DPI native_ppp={} effective_ppp={} logical_width={} logical_height={}",
                geometry[0], geometry[1], geometry[2], geometry[3]
            );
            self.geometry = Some(geometry);
        }
        self.commands(ctx);
        self.app.update(ctx, frame);
        let privacy = &self.app.live.as_ref().unwrap().privacy;
        let state = [
            privacy.prompt.is_some(),
            privacy
                .prompt
                .as_ref()
                .is_some_and(|prompt| prompt.pending.is_some()),
            self.app.modal_blocked,
            self.app.modal_open(),
            self.app.model.selected.as_deref() == Some("d:22"),
            privacy.ready(),
        ];
        if self.state_trace != Some(state) {
            eprintln!("UI_PRIVACY_STATE prompt={} pending={} blocked={} modal={} hidden_selected={} ready={}", state[0],state[1],state[2],state[3],state[4],state[5]);
            self.state_trace = Some(state);
        }
        if !self.commands.is_empty() {
            ctx.request_repaint();
        }
    }
}

#[test]
#[ignore = "requires approved isolated X11; synthetic presentation, not live authentication"]
fn hidden_conversation_native_window_fixture() {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let root = Arc::new(std::sync::Mutex::new(None));
    let cleanup = root.clone();
    eframe::run_native(
        "Zalo Native · Hidden conversation presentation fixture",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1200.0, 780.0])
                .with_min_inner_size([640.0, 480.0]),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let (mut app, path) = live::tests::fixture(&cc.egui_ctx);
            *root.lock().unwrap() = Some(path);
            let (worker, commands, events) = LiveWorker::presentation_fixture("11");
            let state = app.live.as_mut().unwrap();
            state.worker = Some(worker);
            state.privacy = Default::default();
            state.connection = crate::app::live::LiveConnection::Online;
            app.fixture_connection = Some(crate::app::live::LiveConnection::Online);
            app.theme.set_mode(
                if std::env::var("ZALO_UI_FIXTURE_THEME").as_deref() == Ok("dark") {
                    crate::theme::ThemeMode::Dark
                } else {
                    crate::theme::ThemeMode::Light
                },
            );
            let phase = match std::env::var("ZALO_UI_FIXTURE_PRIVACY").as_deref() {
                Ok("checking") => LivePrivacyStatus::Checking,
                Ok("failed") => LivePrivacyStatus::Failed,
                _ => LivePrivacyStatus::Ready,
            };
            let mut fixture = PresentationFixture {
                app,
                commands,
                _events: events,
                revision: 2,
                unlocked: None,
                pending: None,
                phase,
                geometry: None,
                state_trace: None,
            };
            fixture.policy(&cc.egui_ctx);
            fixture.apply(
                &cc.egui_ctx,
                LiveEvent::Ready {
                    account_id: "11".into(),
                    conversations: vec![conversation("d:22"), conversation("d:44")],
                },
            );
            Ok(Box::new(fixture))
        }),
    )
    .unwrap();
    if let Some(path) = cleanup.lock().unwrap().take() {
        std::fs::remove_dir_all(path).unwrap();
    };
}
