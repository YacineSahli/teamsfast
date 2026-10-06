use teamsfast::TeamsFastApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(res) = teamsfast::debug_dispatch(&args) {
        return res;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

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

    eframe::run_native(
        "TeamsFast",
        options,
        Box::new(|cc| {
            if std::env::var_os("TEAMSFAST_NO_EMOJI").is_none() {
                teamsfast::init_emoji(&cc.egui_ctx);
            }
            teamsfast::apply_style(&cc.egui_ctx);
            Ok(Box::new(TeamsFastApp::new(cc)))
        }),
    )
}
