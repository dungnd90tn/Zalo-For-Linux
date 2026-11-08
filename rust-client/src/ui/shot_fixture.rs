//! Opt-in synthetic screenshot of the offline demo, for design review.
//!
//! The window shows only the app's own synthetic demo conversations. The image comes from the
//! app's own framebuffer through egui's screenshot command, never from the desktop, so nothing
//! outside the window can appear in it. Writes a binary PPM (convert with any image tool).
//!
//! ```text
//! env -u WAYLAND_DISPLAY ZALO_UI_SHOT=/tmp/shot.ppm ZALO_UI_SHOT_THEME=dark \
//!   ZALO_UI_SHOT_SIZE=1200x780 cargo test --locked --offline --lib -- --ignored --exact ui::shot_fixture::demo_screenshot
//! ```
//!
//! Optional: `ZALO_UI_SHOT_SCENE=settings|info|attach`, `ZALO_UI_SHOT_POINTER=x,y` (hover),
//! `ZALO_UI_SHOT_PRESS=1` (hold the press at that point) and `ZALO_UI_SHOT_TABS=n` (keyboard focus).
use super::*;
use std::io::Write as _;

struct Shot {
    app: NativeApp,
    frames: u32,
    out: std::path::PathBuf,
    dark: bool,
    requested: bool,
    /// `settings` or `info` opens that surface before the capture.
    scene: String,
    /// Pointer position for hover, or for a held press when `press` is set.
    pointer: Option<egui::Pos2>,
    press: bool,
    /// Number of Tab presses, one per frame, to move keyboard focus.
    tabs: u32,
}

impl eframe::App for Shot {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Synthetic input for state captures; the window never needs a real pointer.
        if self.frames < 20 {
            return;
        }
        if let Some(pos) = self.pointer {
            raw.events.push(egui::Event::PointerMoved(pos));
            if self.press && self.frames == 20 {
                raw.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
        if self.frames >= 22 && self.frames < 22 + self.tabs {
            for pressed in [true, false] {
                raw.events.push(egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: Some(egui::Key::Tab),
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
        self.frames += 1;
        if self.frames == 1 {
            self.app.theme.set_mode(if self.dark {
                crate::theme::ThemeMode::Dark
            } else {
                crate::theme::ThemeMode::Light
            });
        }
        if self.frames == 15 {
            match self.scene.as_str() {
                "settings" => self.app.settings_open = true,
                "attach" => {
                    // A synthetic file in the profile's temp root, attached to the open draft.
                    let path = std::env::temp_dir().join("Bao-cao-thiet-ke-quy-4.pdf");
                    std::fs::write(&path, vec![7u8; 1_843_200]).expect("synthetic file");
                    if let Ok(attachment) = crate::domain::Attachment::inspect(&path) {
                        self.app.drafts.active.set_attachment(attachment);
                    }
                }
                "info" => {
                    self.app.conversation_details = self
                        .app
                        .model
                        .selected
                        .clone()
                        .map(|thread| (crate::app::DEMO_ACCOUNT.into(), thread));
                }
                _ => {}
            }
        }
        // Let the demo worker deliver its conversations and the first frames settle.
        if self.frames >= 60 && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let mut data = format!("P6\n{} {}\n255\n", image.width(), image.height()).into_bytes();
            for pixel in &image.pixels {
                data.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
            }
            std::fs::File::create(&self.out)
                .and_then(|mut file| file.write_all(&data))
                .expect("write synthetic screenshot");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

#[test]
#[ignore = "opens a native window; set ZALO_UI_SHOT to the output .ppm path"]
fn demo_screenshot() {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let Some(out) = std::env::var_os("ZALO_UI_SHOT") else {
        return;
    };
    let size = std::env::var("ZALO_UI_SHOT_SIZE").unwrap_or_else(|_| "1200x780".into());
    let (width, height) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
        .filter(|(w, h)| (640.0..=1920.0).contains(w) && (480.0..=1200.0).contains(h))
        .unwrap_or((1200.0, 780.0));
    let dark = std::env::var("ZALO_UI_SHOT_THEME").as_deref() == Ok("dark");
    let scene = std::env::var("ZALO_UI_SHOT_SCENE").unwrap_or_default();
    let pointer = std::env::var("ZALO_UI_SHOT_POINTER")
        .ok()
        .and_then(|value| {
            let (x, y) = value.split_once(',')?;
            Some(egui::pos2(x.parse().ok()?, y.parse().ok()?))
        });
    let root = std::env::temp_dir().join(format!(
        "zalo-ui-shot-{}",
        crate::domain::local_message_id().unwrap()
    ));
    let profile_root = root.clone();
    eframe::run_native(
        "Zalo Native · synthetic screenshot",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([width, height])
                .with_min_inner_size([640.0, 480.0]),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let profile = Profile::at_root(&profile_root, "fixture")?;
            if std::env::var("ZALO_UI_SHOT_SCENE").as_deref() == Ok("info") {
                seed_shared_items(&profile)?;
            }
            let app = NativeApp::new(cc, profile, true)?;
            Ok(Box::new(Shot {
                app,
                frames: 0,
                out: out.into(),
                dark,
                requested: false,
                scene,
                pointer,
                press: std::env::var_os("ZALO_UI_SHOT_PRESS").is_some(),
                tabs: std::env::var("ZALO_UI_SHOT_TABS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            }))
        }),
    )
    .expect("native window");
    let _ = std::fs::remove_dir_all(root);
}

/// Screenshot helper for the other native fixtures: set `ZALO_UI_SHOT` to a `.ppm` path and call
/// [`Grabber::tick`] from `update`. It waits for the window to settle, saves the app's own
/// framebuffer and closes the window. Without the variable it does nothing.
#[cfg(feature = "messaging")]
#[derive(Default)]
pub(super) struct Grabber {
    frames: u32,
    requested: bool,
}

#[cfg(feature = "messaging")]
impl Grabber {
    pub(super) fn tick(&mut self, ctx: &egui::Context) {
        let Some(out) = std::env::var_os("ZALO_UI_SHOT") else {
            return;
        };
        self.frames += 1;
        let settle = std::env::var("ZALO_UI_SHOT_FRAMES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(60);
        if self.frames >= settle && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let mut data = format!("P6\n{} {}\n255\n", image.width(), image.height()).into_bytes();
            for pixel in &image.pixels {
                data.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
            }
            std::fs::write(out, data).expect("write synthetic screenshot");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// Synthetic shared content for the information panel capture: a few pictures, files and links in
/// the demo design room. Every address is under example.com.
fn seed_shared_items(profile: &Profile) -> anyhow::Result<()> {
    use crate::domain::{MediaKind, MediaMessageContent};
    use serde_json::json;
    let mut store =
        crate::store::MessageStore::open(&profile.paths.data_dir.join("messages.sqlite3"))?;
    let mut batch = Vec::new();
    let mut text = |id: &str, body: &str, at: i64| {
        batch.push((
            crate::domain::Message {
                account_id: crate::app::DEMO_ACCOUNT.into(),
                thread_id: "demo-design".into(),
                id: id.into(),
                sender: "Xưởng thiết kế".into(),
                text: body.into(),
                sticker: None,
                media: None,
                sent_at_ms: at,
            },
            None,
            None,
            None,
        ));
    };
    text(
        "shot-link",
        "Tài liệu hướng dẫn giao diện: https://example.com/huong-dan-giao-dien",
        1_700_001_500_000,
    );
    let media = |id: &str, kind: MediaKind, content: serde_json::Value, at: i64| {
        let content = MediaMessageContent::new(kind, &content).expect("synthetic content");
        let message = crate::domain::Message {
            account_id: crate::app::DEMO_ACCOUNT.into(),
            thread_id: "demo-design".into(),
            id: id.into(),
            sender: "Xưởng thiết kế".into(),
            text: format!("[Tin nhắn {} chưa được hỗ trợ]", kind.as_str()),
            sticker: None,
            media: Some(content.descriptor().clone()),
            sent_at_ms: at,
        };
        (message, None::<String>, None, Some(content))
    };
    let items = [
        media(
            "shot-photo-1",
            MediaKind::Photo,
            json!({"title":"Ảnh","description":"Bản phác thảo màn hình danh bạ","href":"https://example.com/anh/phac-thao.jpg"}),
            1_700_001_300_000,
        ),
        media(
            "shot-photo-2",
            MediaKind::Photo,
            json!({"title":"Ảnh","href":"https://example.com/anh/mau-sac.jpg"}),
            1_700_001_350_000,
        ),
        media(
            "shot-file-1",
            MediaKind::File,
            json!({"title":"bao-cao-thiet-ke","params":"{\"fileExt\":\"pdf\",\"fileSize\":\"1843200\"}","href":"https://example.com/tep/bao-cao-thiet-ke.pdf"}),
            1_700_001_400_000,
        ),
        media(
            "shot-file-2",
            MediaKind::File,
            json!({"title":"bang-so-sanh","params":"{\"fileExt\":\"xlsx\",\"fileSize\":\"52000\"}","href":"https://example.com/tep/bang-so-sanh.xlsx"}),
            1_700_001_450_000,
        ),
        media(
            "shot-card",
            MediaKind::Recommended,
            json!({"action":"recommened.link","title":"Hướng dẫn thiết kế giao diện native","href":"https://example.com/bai-viet/thiet-ke-native"}),
            1_700_001_550_000,
        ),
    ];
    batch.extend(items);
    store.receive_batch_with_media(crate::app::DEMO_ACCOUNT, &batch, None)?;
    Ok(())
}
