use teamsfast::TeamsFastApp;

fn main() -> eframe::Result<()> {
    // Debug helper: dump the raw chat list and exit. Headless, no GUI.
    if std::env::args().any(|a| a == "--dump-chats") {
        return dump_chats();
    }
    // Debug helper: dump one chat's member roster.
    if let Some(chat) = std::env::args().nth(2).filter(|_| std::env::args().nth(1).as_deref() == Some("--probe-chat")) {
        return probe_chat(&chat);
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

/// `teamsfast --probe-chat <id>`: dump the chat's member roster.
fn probe_chat(chat_id: &str) -> eframe::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let client = ost::api::client::TeamsClient::new()
            .await
            .expect("not signed in");
        let members = ost::api::list_chat_members_data(&client, chat_id)
            .await
            .expect("members")
            .1;
        for m in &members {
            println!(
                "{:45.45} | uid={:?} | {:25.25} | {:?}",
                m.mri,
                m.user_id,
                if m.display_name.is_empty() { "(no name)" } else { &m.display_name },
                m.email
            );
        }
    });
    Ok(())
}

/// `teamsfast --dump-chats`: print id/name/kind for every conversation the
/// server returns, so filters can be built against real data.
fn dump_chats() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let client = ost::api::client::TeamsClient::new()
            .await
            .expect("not signed in (run the GUI once and sign in)");
        let chats = ost::api::list_chats_data(&client, 60)
            .await
            .expect("chat list");
        for c in &chats {
            println!(
                "{} | group={:5} | {}",
                c.id,
                c.is_group,
                if c.name.is_empty() { "(no name)" } else { &c.name }
            );
        }
    });
    Ok(())
}
