use teamsfast::TeamsFastApp;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer as _;

/// Log + panic file locations: `$XDG_STATE_HOME/teamsfast/` (default
/// `~/.local/state/teamsfast/`).
fn log_paths() -> (std::path::PathBuf, std::path::PathBuf) {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                std::path::PathBuf::from(h).join(".local").join("state")
            })
        })
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("teamsfast");
    (state.join("teamsfast.log"), state.join("panic.log"))
}

/// Install the subscriber: every event from both `tracing` (ost protocol)
/// and the `log` facade (app code) goes to the terminal AND to a per-run
/// log file. `RUST_LOG` overrides the filter for both layers.
fn init_logging(log_path: &std::path::Path, panic_path: &std::path::Path) {
    if let Some(dir) = log_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = match std::fs::File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open log file {log_path:?}: {e} (logging to stderr only)");
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "warn,teamsfast=info,ost=info".into()),
                )
                .with_target(false)
                .init();
            return;
        }
    };

    // Panic hook: backtrace into the panic log, then the default behaviour.
    let panic_file = panic_path.to_path_buf();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = std::fs::File::options()
            .create(true)
            .append(true)
            .open(&panic_file)
        {
            use std::io::Write;
            let _ = writeln!(f, "=== panic: {info} ===");
            let _ = writeln!(f, "{:?}", std::backtrace::Backtrace::force_capture());
        }
        default_hook(info);
    }));

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "warn,teamsfast=debug,ost=debug".into());
    let file_writer = std::sync::Mutex::new(file);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_filter(filter.clone()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_ansi(false)
                .with_writer(file_writer)
                .with_filter(filter),
        )
        .init();
}

/// Handle of the running window, for focusing from a second launch.
static APP_CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();


fn main() -> eframe::Result<()> {
    // Incoming calls must ring in the UI, never auto-answer. teams-core's
    // trouter loop answers invitations unless this is set (the documented
    // embedder switch); set it before any thread spawns (set_var is racy
    // with concurrent getenv).
    unsafe { std::env::set_var("TEAMS_MANUAL_CALLS", "1") };

    // Multi-account profile: select teams-core's keyring/config profile
    // BEFORE any thread can load credentials. The launcher pairs this
    // with per-profile XDG_{CONFIG,STATE,DATA}_HOME so files split too.
    if !teamsfast::control::profile_suffix().is_empty() {
        let p = std::env::var("TEAMSFAST_PROFILE").unwrap_or_default();
        ost::config::set_active_profile(p.trim());
    }

    // `--ctl <verb>`: send a control verb to the running instance (same
    // profile + binary scope) and print its reply. Exit code 0 = "ok".
    if let Some(pos) = std::env::args().position(|a| a == "--ctl") {
        // The whole rest of the line is one verb ("call <id>", "mute on").
        let verb: String = std::env::args()
            .skip(pos + 1)
            .collect::<Vec<_>>()
            .join(" ");
        if verb.is_empty() {
            eprintln!("usage: teamsfast --ctl <verb>   (ping | call <chat_id> | videocall <chat_id> | calluser <upn> | echo | answer | decline | hangup | mute on|off)");
            std::process::exit(2);
        }
        let mut slot = fastframe_instance::Slot::new(&format!("teamsfast{}", teamsfast::control::profile_suffix()));
        if let Ok(exe) = std::env::current_exe() {
            slot = slot.scoped(exe.to_string_lossy().as_ref());
        }
        match slot.send(&verb) {
            Ok(reply) => {
                println!("{reply}");
                std::process::exit(if reply == "ok" { 0 } else { 1 });
            }
            Err(e) => {
                eprintln!("no running TeamsFast answered ({e})");
                std::process::exit(1);
            }
        }
    }

    // Update plumbing first: run the install helper when asked, and take
    // the update flags off the command line before anything else parses.
    let launch = fastframe_update::intercept(&teamsfast::updates::CONFIG);

    let args: Vec<String> = launch
        .arguments
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args.iter().any(|a| a == "--version") {
        // The updater's rollback probe expects `<slug> <version>`.
        println!("teamsfast {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Some(res) = teamsfast::debug_dispatch(&args) {
        return res;
    }

    // Single instance: a second GUI launch hands "show" to the running
    // copy (which focuses itself) and exits. Scoped per binary so the dev
    // build and the installed copy don't evict each other; QA runs
    // (TEAMSFAST_SIZE set, no profile) bypass the guard entirely. A
    // PROFILED run always claims its own slot (teamsfast-<profile>):
    // it cannot collide with the default instance, and the E2E rig's
    // --ctl channel needs the claim to exist.
    if std::env::var_os("TEAMSFAST_SIZE").is_none()
        || !teamsfast::control::profile_suffix().is_empty()
    {
        let mut slot = fastframe_instance::Slot::new(&format!("teamsfast{}", teamsfast::control::profile_suffix()));
        if let Ok(exe) = std::env::current_exe() {
            slot = slot.scoped(exe.to_string_lossy().as_ref());
        }
        let _instance = slot.claim("show", |request| {
            if request == "show"
                && let Some(ctx) = APP_CTX.get()
            {
                // Same triple the tray "show" sends: Visible re-maps an
                // X11-unmapped window, Minimized(false) attempts the
                // Wayland restore (compositor-side; winit warns but is
                // harmless where ignored), Focus raises where it can.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
                return Some("ok".to_owned());
            }
            // Control verbs (E2E call rig): queue for the app to drain on
            // the UI thread, wake the frame loop, confirm to the sender.
            if !request.is_empty()
                && let Some(ctx) = APP_CTX.get()
                && teamsfast::control::is_control_verb(request)
            {
                teamsfast::control::push_control_verb(request);
                ctx.request_repaint();
                return Some("ok".to_owned());
            }
            None
        });
        match _instance {
            fastframe_instance::Claim::First(guard) => {
                // Hold the slot for the process lifetime.
                std::mem::forget(guard);
            }
            _ => {
                println!("TeamsFast is already running; focusing that window.");
                return Ok(());
            }
        }
    }

    let (log_path, panic_path) = log_paths();
    init_logging(&log_path, &panic_path);
    log::info!("TeamsFast {} starting; log file: {}", env!("CARGO_PKG_VERSION"), log_path.display());
    if std::env::args().any(|a| a == "--logs") {
        println!("{}", log_path.display());
        return Ok(());
    }
    println!("log: {}", log_path.display());

    // QA hook: TEAMSFAST_SIZE=WxH overrides the window size. Explicitly
    // sized runs also drop the eframe-persisted geometry (saved on graceful
    // exit) — otherwise the restored 1095x582 from any previous session
    // wins and the QA window renders clipped.
    let size: [f32; 2] = std::env::var("TEAMSFAST_SIZE")
        .ok()
        .and_then(|s| {
            let mut it = s.split('x');
            Some([
                it.next()?.parse().ok()?,
                it.next()?.parse().ok()?,
            ])
        })
        .unwrap_or([1240.0, 820.0]);
    if std::env::var_os("TEAMSFAST_SIZE").is_some() {
        if let Some(home) = std::env::var_os("HOME") {
            let _ = std::fs::remove_file(
                std::path::PathBuf::from(home)
                    .join(".local/share/teamsfast/app.ron"),
            );
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_title("TeamsFast"),
        ..Default::default()
    };

    let receipt = launch.receipt;
    let launch_error = launch.error;
    eframe::run_native(
        "TeamsFast",
        options,
        Box::new(move |cc| {
            if std::env::var_os("TEAMSFAST_NO_EMOJI").is_none() {
                teamsfast::init_theme(&cc.egui_ctx);
            } else {
                teamsfast::apply_style(&cc.egui_ctx);
            }
            let _ = APP_CTX.set(cc.egui_ctx.clone());
            Ok(Box::new(TeamsFastApp::new(cc, receipt, launch_error)))
        }),
    )
}
