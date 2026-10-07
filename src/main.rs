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

fn main() -> eframe::Result<()> {
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

    let (log_path, panic_path) = log_paths();
    init_logging(&log_path, &panic_path);
    log::info!("TeamsFast {} starting; log file: {}", env!("CARGO_PKG_VERSION"), log_path.display());
    if std::env::args().any(|a| a == "--logs") {
        println!("{}", log_path.display());
        return Ok(());
    }
    println!("log: {}", log_path.display());

    // QA hook: TEAMSFAST_SIZE=WxH overrides the window size.
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
            Ok(Box::new(TeamsFastApp::new(cc, receipt, launch_error)))
        }),
    )
}
