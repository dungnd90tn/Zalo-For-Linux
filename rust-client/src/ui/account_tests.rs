//! Account menus preserve routing, drafts and input isolation in real egui frames.
use super::*;
use std::time::{Duration, Instant};

const DRAFT: &str = "Bản nháp vẫn thuộc đúng hội thoại";

fn fixture(ctx: &egui::Context) -> (NativeApp, std::path::PathBuf) {
    let (mut app, root) = live::tests::fixture(ctx);
    app.model.conversations = vec![Conversation {
        id: "d:22".into(),
        title: "Hội thoại kiểm thử".into(),
        last_message_at_ms: 1_700_000_000_000,
        last_text: "Tin đã lưu".into(),
        last_sender: "22".into(),
    }];
    app.model.selected = Some("d:22".into());
    app.drafts.select("d:22").unwrap();
    app.drafts.active.text = DRAFT.into();
    (app, root)
}

fn frame(
    app: &mut NativeApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            app.update_modal_input(ctx);
            if !app.show_login(ctx) {
                app.workspace(ctx);
            } else {
                app.show_account_popup(ctx);
            }
            if app.settings_open {
                app.settings
                    .show(ctx, &mut app.settings_open, &mut app.theme);
            }
        },
    )
}
fn settle(app: &mut NativeApp, ctx: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
    for _ in 0..2 {
        frame(app, ctx, size, vec![]);
    }
    frame(app, ctx, size, vec![])
}
fn text_rect(output: &egui::FullOutput, value: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == value => {
            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
            shape.clip_rect.contains_rect(rect).then_some(rect)
        }
        _ => None,
    })
}
fn texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
            _ => None,
        })
        .collect()
}
fn tap(
    app: &mut NativeApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    pos: egui::Pos2,
) -> egui::FullOutput {
    frame(
        app,
        ctx,
        size,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        app,
        ctx,
        size,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    settle(app, ctx, size)
}
fn choose(
    app: &mut NativeApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    output: &egui::FullOutput,
    label: &str,
) -> egui::FullOutput {
    let rect = text_rect(output, label)
        .unwrap_or_else(|| panic!("Missing action {label:?}: {:?}", texts(output)));
    assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(rect));
    tap(app, ctx, size, rect.center())
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}
fn gear(size: egui::Vec2) -> egui::Pos2 {
    pos2(32.0, size.y - 42.0)
}

#[test]
fn gear_routes_appearance_and_logout_separately_and_cancel_keeps_identity_and_draft() {
    for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let account = app.account_id().unwrap().to_owned();
        let generation = app.live.as_ref().unwrap().login.generation();
        settle(&mut app, &ctx, size);
        let menu = tap(&mut app, &ctx, size, gear(size));
        assert!(app.account_popup.is_some());
        assert!(!app.settings_open);
        assert!(!app.live.as_ref().unwrap().logout_dialog);
        choose(&mut app, &ctx, size, &menu, "Thiết lập giao diện");
        assert!(app.settings_open);
        assert!(app.account_popup.is_none());
        assert!(!app.live.as_ref().unwrap().logout_dialog);
        frame(
            &mut app,
            &ctx,
            size,
            key(egui::Key::Escape, egui::Modifiers::NONE),
        );
        settle(&mut app, &ctx, size);
        assert!(!app.settings_open);
        let menu = tap(&mut app, &ctx, size, gear(size));
        let confirmation = choose(&mut app, &ctx, size, &menu, "Đăng xuất");
        assert!(app.live.as_ref().unwrap().logout_dialog);
        assert!(!app.settings_open);
        assert_eq!(app.account_id(), Some(account.as_str()));
        choose(&mut app, &ctx, size, &confirmation, "Hủy");
        assert!(!app.live.as_ref().unwrap().logout_dialog);
        assert_eq!(app.account_id(), Some(account.as_str()));
        assert_eq!(app.live.as_ref().unwrap().login.generation(), generation);
        assert_eq!(app.drafts.active.text, DRAFT);
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn popup_blocks_submit_and_outside_click_then_restores_the_scoped_editor() {
    let ctx = egui::Context::default();
    let (mut app, root) = fixture(&ctx);
    let size = vec2(1200.0, 780.0);
    app.live = None;
    app.demo = true;
    app.worker = Some(
        Worker::demo(
            app._profile
                .paths
                .data_dir
                .join("account-menu-demo.sqlite3"),
            Arc::new(|| {}),
        )
        .unwrap(),
    );
    app.model.conversations.push(Conversation {
        id: "d:33".into(),
        title: "Hội thoại thứ hai".into(),
        last_message_at_ms: 0,
        last_text: String::new(),
        last_sender: "33".into(),
    });
    app.focus_composer = true;
    settle(&mut app, &ctx, size);
    let editor = ctx.memory(|m| m.focused()).unwrap();
    let ordinary = settle(&mut app, &ctx, size);
    let other = text_rect(&ordinary, "Hội thoại thứ hai").unwrap().center();
    tap(&mut app, &ctx, size, gear(size));
    tap(&mut app, &ctx, size, other);
    assert!(app.account_popup.is_none());
    assert_eq!(
        app.model.selected.as_deref(),
        Some("d:22"),
        "An outside dismissal must not also navigate the background"
    );
    assert_eq!(app.drafts.active.text, DRAFT);
    tap(&mut app, &ctx, size, gear(size));
    assert!(app.account_popup.is_some());
    let submit = || {
        key(
            egui::Key::Enter,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        )
    };
    let output = frame(&mut app, &ctx, size, submit());
    app.send_local();
    assert!(!app.drafts.active.in_flight());
    assert_eq!(app.drafts.active.text, DRAFT);
    let send = text_rect(&output, "Lưu").unwrap().center();
    tap(&mut app, &ctx, size, send);
    assert!(app.account_popup.is_none());
    assert!(!app.drafts.active.in_flight());
    assert_eq!(app.drafts.active.text, DRAFT);
    assert_eq!(ctx.memory(|m| m.focused()), Some(editor));
    frame(&mut app, &ctx, size, submit());
    assert!(app.drafts.active.in_flight());
    let until = Instant::now() + Duration::from_secs(2);
    let mut stored = false;
    while Instant::now() < until {
        while let Ok(event) = app.worker.as_mut().unwrap().events.try_recv() {
            stored |= matches!(event, Event::DemoStored { .. });
        }
        if stored {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        stored,
        "The same editor shortcut must still work after dismissal"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn popup_scope_drops_stale_account_thread_history_and_authentication_attempt_actions() {
    for change in 0..4 {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let size = vec2(1200.0, 780.0);
        settle(&mut app, &ctx, size);
        let menu = tap(&mut app, &ctx, size, gear(size));
        let old_action = text_rect(&menu, "Đăng xuất").unwrap().center();
        match change {
            0 => app.live.as_mut().unwrap().account.as_mut().unwrap().id = "44".into(),
            1 => {
                app.model.selected = Some("d:33".into());
                app.drafts.select("d:33").unwrap();
                app.drafts.active.text = "Nháp khác".into();
            }
            2 => app.model.generation = app.model.generation.wrapping_add(1),
            _ => app.live.as_mut().unwrap().login.start(99, true),
        }
        tap(&mut app, &ctx, size, old_action);
        assert!(app.account_popup.is_none());
        assert!(!app.settings_open);
        assert!(!app.live.as_ref().unwrap().logout_dialog);
        assert_eq!(
            app.drafts.active.text,
            if change == 1 { "Nháp khác" } else { DRAFT }
        );
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn signed_out_gear_exposes_appearance_without_logout() {
    for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        app.live.as_mut().unwrap().account = None;
        settle(&mut app, &ctx, size);
        let menu = tap(&mut app, &ctx, size, pos2(size.x - 16.0, 16.0));
        assert!(app.account_popup.is_some());
        assert!(!app.settings_open);
        assert!(text_rect(&menu, "Thiết lập giao diện").is_some());
        assert!(text_rect(&menu, "Đăng xuất").is_none());
        choose(&mut app, &ctx, size, &menu, "Thiết lập giao diện");
        assert!(app.settings_open);
        assert!(!app.live.as_ref().unwrap().logout_dialog);
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn popup_anchor_second_click_and_escape_dismiss_at_both_window_sizes() {
    for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        settle(&mut app, &ctx, size);
        for anchor in [pos2(32.0, 43.0), gear(size)] {
            tap(&mut app, &ctx, size, anchor);
            assert!(app.account_popup.is_some());
            tap(&mut app, &ctx, size, anchor);
            assert!(
                app.account_popup.is_none(),
                "The same anchor must remain reachable while its popup is open"
            );
            tap(&mut app, &ctx, size, anchor);
            frame(
                &mut app,
                &ctx,
                size,
                key(egui::Key::Escape, egui::Modifiers::NONE),
            );
            settle(&mut app, &ctx, size);
            assert!(app.account_popup.is_none());
            assert_eq!(app.drafts.active.text, DRAFT);
        }
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn removed_status_bars_reclaim_content_while_avatar_keeps_recovery_and_offline_input() {
    use crate::app::live::LiveConnection;
    for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let account_label = app.account_label().to_owned();
        app.focus_composer = true;
        let ordinary = settle(&mut app, &ctx, size);
        assert!(!texts(&ordinary).contains(&account_label.as_str()));
        assert!(text_rect(&ordinary, "Kết nối lại").is_none());
        assert!(ordinary.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.rect.height()==68.0 && r.rect.top()==0.0 && r.rect.left()>=64.0)),"The chat header must start at the top, without a persistent status panel");
        frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::Text(" nhập liệu".into())],
        );
        assert_eq!(app.drafts.active.text.replace(" nhập liệu", ""), DRAFT);
        assert!(!app.live_can_send());
        for (connection, label) in [
            (LiveConnection::Offline, "Ngoại tuyến"),
            (LiveConnection::Connecting, "Đang kết nối…"),
            (LiveConnection::Reconnecting, "Đang kết nối lại…"),
            (LiveConnection::Expired, "Phiên đã hết hạn"),
            (LiveConnection::Online, "Đã kết nối Zalo"),
        ] {
            app.live.as_mut().unwrap().connection = connection;
            let closed = settle(&mut app, &ctx, size);
            let colors = crate::theme::palette(&ctx);
            let expected = match connection {
                LiveConnection::Online => colors.status_online,
                LiveConnection::Connecting | LiveConnection::Reconnecting => {
                    colors.status_connecting
                }
                LiveConnection::Expired => colors.error,
                LiveConnection::Offline => colors.status_offline,
            };
            assert!(closed.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c)
                if c.center.x < 64.0 && c.center.y < 80.0 && c.radius == 5.0 && c.fill == expected)),
                "The avatar badge must reflect the authenticated transport state");
            // Reuse the same menu area after the shorter settings contents.
            // Recovery labels must really fit the painter clip, not merely exist as shapes.
            tap(&mut app, &ctx, size, gear(size));
            frame(
                &mut app,
                &ctx,
                size,
                key(egui::Key::Escape, egui::Modifiers::NONE),
            );
            settle(&mut app, &ctx, size);
            let menu = tap(&mut app, &ctx, size, pos2(32.0, 43.0));
            assert!(app.account_popup.is_some());
            assert!(
                text_rect(&menu, label).is_some(),
                "Missing {label}: {:?}",
                texts(&menu)
            );
            assert!(
                text_rect(&menu, "Đăng xuất").is_none(),
                "Logout belongs to the settings menu"
            );
            if connection != LiveConnection::Online {
                assert!(text_rect(&menu, "Kết nối lại").is_some());
                assert!(text_rect(&menu, "Kết nối TLS chưa hoàn tất.").is_some());
            }
            frame(
                &mut app,
                &ctx,
                size,
                key(egui::Key::Escape, egui::Modifiers::NONE),
            );
            settle(&mut app, &ctx, size);
        }
        app.account_popup = None;
        if size.x < 940.0 {
            app.compact_list = true;
        }
        let list = settle(&mut app, &ctx, size);
        assert!(list.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.pos.x>64.0 && t.pos.x<(if size.x<940.0 {size.x}else{408.0}) && t.galley.job.text==clock_label(1_700_000_000_000) && s.clip_rect.bottom()>=size.y-1.0)),"The signed-in conversation list must retain its bottom area instead of a hidden account footer");
        assert!(text_rect(&list, "Đăng xuất").is_none());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn an_open_settings_menu_tracks_its_real_gear_when_the_window_resizes() {
    let ctx = egui::Context::default();
    let (mut app, root) = fixture(&ctx);
    let compact = vec2(640.0, 480.0);
    let wide = vec2(1200.0, 780.0);
    settle(&mut app, &ctx, compact);
    let first = tap(&mut app, &ctx, compact, gear(compact));
    let initial = text_rect(&first, "Đăng xuất").unwrap();
    let grown = settle(&mut app, &ctx, wide);
    assert!(app.account_popup.is_some());
    let moved = text_rect(&grown, "Đăng xuất").unwrap();
    assert!(
        (moved.center().y - initial.center().y - (wide.y - compact.y)).abs() < 1.0,
        "The open menu must follow the actual bottom gear, not its old screen position"
    );
    assert!((moved.center().x - initial.center().x).abs() < 1.0);
    let restored = settle(&mut app, &ctx, compact);
    assert!(app.account_popup.is_some());
    let final_rect = text_rect(&restored, "Đăng xuất").unwrap();
    assert!((final_rect.center() - initial.center()).length() < 1.0);
    assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, compact).contains_rect(final_rect));
    assert_eq!(app.drafts.active.text, DRAFT);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
