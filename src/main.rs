use teamsfast::TeamsFastApp;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_title("TeamsFast"),
        ..Default::default()
    };

    eframe::run_native(
        "TeamsFast",
        options,
        Box::new(|cc| Ok(Box::new(TeamsFastApp::new(cc)))),
    )
}
