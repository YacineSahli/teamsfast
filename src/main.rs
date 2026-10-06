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

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 820.0])
            .with_title("TeamsFast"),
        ..Default::default()
    };

    eframe::run_native(
        "TeamsFast",
        options,
        Box::new(|cc| {
            teamsfast::apply_style(&cc.egui_ctx);
            Ok(Box::new(TeamsFastApp::new(cc)))
        }),
    )
}
