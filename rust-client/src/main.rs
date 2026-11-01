//! Native preview entry point. Diagnostic commands do not create profiles or access the network.
use anyhow::{bail, Result};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use zalo_rust::{
    app::{Command, Event, Worker, DEMO_ACCOUNT},
    migration,
    profile::Profile,
};

#[derive(Debug, Default)]
struct Cli {
    help: bool,
    version: bool,
    doctor: bool,
    debug_log: bool,
    demo: bool,
    live: bool,
    trust_module: Option<PathBuf>,
    headless: bool,
    profile: Option<String>,
    root: Option<PathBuf>,
    snapshot: Option<PathBuf>,
    account: Option<String>,
}
impl Cli {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut cli = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => cli.help = true,
                "--version" | "-V" => cli.version = true,
                "--doctor" => cli.doctor = true,
                "--debug-log" => cli.debug_log = true,
                "--demo" => cli.demo = true,
                "--live" => cli.live = true,
                "--headless" => cli.headless = true,
                "--gui" => {}
                "--profile"
                | "--profile-root"
                | "--inspect-electron-copy"
                | "--source-account"
                | "--trust-module" => {
                    let value = args
                        .next()
                        .filter(|v| !v.starts_with("--"))
                        .ok_or_else(|| anyhow::anyhow!("Missing value for {arg}"))?;
                    match arg.as_str() {
                        "--profile" => cli.profile = Some(value),
                        "--profile-root" => cli.root = Some(value.into()),
                        "--inspect-electron-copy" => cli.snapshot = Some(value.into()),
                        "--trust-module" => cli.trust_module = Some(value.into()),
                        _ => cli.account = Some(value),
                    }
                }
                _ => bail!("Unknown option {arg}; use --help"),
            }
        }
        if cli.headless && !cli.demo {
            bail!("--headless requires --demo");
        }
        if cli.live && (cli.demo || cli.headless || cli.doctor || cli.snapshot.is_some()) {
            bail!("Choose --live, --demo, --doctor, or inspection as separate operations");
        }
        if cli.trust_module.is_some() && !cli.live {
            bail!("--trust-module requires --live");
        }
        if cli
            .trust_module
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            bail!("--trust-module must be an absolute path");
        }
        if cli.snapshot.is_some() != cli.account.is_some() {
            bail!("Inspection requires --inspect-electron-copy SNAPSHOT --source-account ID");
        }
        if cli.snapshot.is_some() && (cli.demo || cli.doctor) {
            bail!("Choose inspection, doctor, or demo as separate operations");
        }
        if cli.doctor && cli.demo {
            bail!("Choose --doctor or --demo");
        }
        if let Some(profile) = &cli.profile {
            zalo_rust::profile::validate_profile_name(profile)?;
        }
        if cli.root.as_ref().is_some_and(|r| !r.is_absolute()) {
            bail!("--profile-root must be absolute");
        }
        Ok(cli)
    }
    fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or(if self.demo {
            "rust-demo"
        } else if self.live {
            "rust-live"
        } else {
            "rust-pilot"
        })
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse(std::env::args().skip(1))?;
    if cli.help {
        println!("Zalo Linux Rust preview\n");
        println!("Usage: zalo-rust [--gui] [--profile NAME] [--profile-root DIR]\n       zalo-rust --live [--trust-module ABSOLUTE_PATH] [--profile NAME]\n       zalo-rust --demo [--headless] [--profile NAME] [--profile-root DIR]\n       zalo-rust --doctor\n       zalo-rust --inspect-electron-copy SNAPSHOT --source-account ID\n\n--live       Enable live login and messaging compiled with Cargo feature 'live'\n             Network access starts only after an explicit login/restore action\n--trust-module  Optional advanced override of the bundled, hash-verified trust module\n--demo       Offline GUI with synthetic messages and local-only storage\n--headless   Exercise demo worker/storage without opening a window\n--doctor     Print offline environment/capability JSON without creating profiles\n--debug-log  Include additional fixed structural diagnostics; logs are always enabled\n--help, -h   Show help\n--version    Print version\n\nCalls, updates, and history import are unavailable.\nDefault profiles: rust-live for live, rust-demo for demo, rust-pilot otherwise. Electron data is never opened automatically.");
        return Ok(());
    }
    if cli.version {
        println!("zalo-linux-native-preview {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Some(snapshot) = &cli.snapshot {
        println!(
            "{}",
            serde_json::to_string_pretty(&migration::inspect_snapshot(
                snapshot,
                cli.account.as_deref().unwrap()
            )?)?
        );
        return Ok(());
    }
    if cli.doctor {
        #[derive(Serialize)]
        struct Doctor<'a> {
            version: &'a str,
            display_available: bool,
            wayland: bool,
            x11: bool,
            live_auth_available: bool,
            live_send_available: bool,
            live_auth_compiled: bool,
            live_messaging_compiled: bool,
            authenticated: bool,
            network_started: bool,
            history_import_available: bool,
            selected_profile: &'a str,
            diagnostic_log_path: PathBuf,
            command_queue_capacity: usize,
            event_queue_capacity: usize,
            max_page_size: usize,
        }
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let x11 = std::env::var_os("DISPLAY").is_some();
        println!(
            "{}",
            serde_json::to_string_pretty(&Doctor {
                version: env!("CARGO_PKG_VERSION"),
                display_available: wayland || x11,
                wayland,
                x11,
                live_auth_available: false,
                live_send_available: false,
                live_auth_compiled: cfg!(feature = "live-login"),
                live_messaging_compiled: cfg!(feature = "messaging"),
                authenticated: false,
                network_started: false,
                history_import_available: false,
                selected_profile: cli.profile_name(),
                diagnostic_log_path: zalo_rust::diagnostics::log_path(
                    &Profile::planned_paths(cli.root.as_deref(), cli.profile_name())?.cache_dir
                ),
                command_queue_capacity: zalo_rust::app::COMMAND_CAPACITY,
                event_queue_capacity: zalo_rust::app::EVENT_CAPACITY,
                max_page_size: zalo_rust::domain::MAX_PAGE_SIZE
            })?
        );
        return Ok(());
    }
    if cli.live && !cfg!(feature = "live-login") {
        bail!("This binary does not include live login. Build with cargo build --locked --release --features live, or use ./scripts/rust/run.sh --live");
    }
    let profile = if let Some(root) = &cli.root {
        Profile::at_root(root, cli.profile_name())?
    } else {
        Profile::open(cli.profile_name())?
    };
    let _diagnostics =
        match zalo_rust::diagnostics::Logger::start(&profile.paths.cache_dir, cli.debug_log) {
            Ok(logger) => {
                eprintln!("Diagnostic log: {}", logger.path().display());
                Some(logger)
            }
            Err(_) => {
                eprintln!(
                    "Diagnostic log unavailable: private path or I/O failure. Intended log: {}",
                    zalo_rust::diagnostics::log_path(&profile.paths.cache_dir).display()
                );
                None
            }
        };
    // Demo data must never become another account's cached history.
    if cli.demo {
        profile.bind_account(DEMO_ACCOUNT)?;
    }
    if cli.headless {
        return headless_demo(&profile);
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Zalo Linux — Rust Preview")
            .with_app_id("zalo-linux-rust-preview")
            .with_inner_size([1024.0, 720.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Zalo Linux — Rust Preview",
        options,
        Box::new(move |cc| {
            #[cfg(feature = "live-login")]
            if cli.live {
                return Ok(Box::new(zalo_rust::ui::NativeApp::new_live(
                    cc,
                    profile,
                    cli.trust_module,
                )?));
            }
            Ok(Box::new(zalo_rust::ui::NativeApp::new(
                cc, profile, cli.demo,
            )?))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Native window could not start: {e}"))
}

fn headless_demo(profile: &Profile) -> Result<()> {
    let mut worker = Worker::demo(
        profile.paths.data_dir.join("messages.sqlite3"),
        Arc::new(|| {}),
    )?;
    let ready = worker
        .events
        .blocking_recv()
        .ok_or_else(|| anyhow::anyhow!("Demo worker stopped before initialization"))?;
    let threads = match ready {
        Event::Ready(threads) => threads,
        Event::Fault(error) => bail!("{error}"),
        _ => bail!("Unexpected startup event"),
    };
    worker
        .submit(Command::Load {
            generation: 1,
            thread: "welcome".into(),
            before: None,
        })
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let page = worker
        .events
        .blocking_recv()
        .ok_or_else(|| anyhow::anyhow!("Demo worker stopped before loading messages"))?;
    let messages = match page {
        Event::Page { messages, .. } => messages,
        Event::Fault(error) => bail!("{error}"),
        _ => bail!("Unexpected page event"),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"mode":"offline-demo","account":"synthetic","threads":threads.len(),"messages_loaded":messages.len(),"network_used":false,"live_messages_sent":0,"profile":profile.name})
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Cli> {
        Cli::parse(args.iter().map(|s| s.to_string()))
    }
    #[test]
    fn invalid_or_ambiguous_cli_is_rejected() {
        for args in [
            &["--profile"][..],
            &["--headless"],
            &["--doctor", "--demo"],
            &["--live", "--demo"],
            &["--live", "--doctor"],
            &["--trust-module", "/tmp/trust.wasm"],
            &["--live", "--trust-module", "relative.wasm"],
            &["--profile-root", "relative"],
            &["--profile", "../bad"],
            &["--inspect-electron-copy", "copy.db"],
            &["--unknown"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn demo_has_a_distinct_default_profile() {
        assert_eq!(parse(&["--demo"]).unwrap().profile_name(), "rust-demo");
        assert_eq!(parse(&[]).unwrap().profile_name(), "rust-pilot");
        assert_eq!(parse(&["--live"]).unwrap().profile_name(), "rust-live");
    }
}
