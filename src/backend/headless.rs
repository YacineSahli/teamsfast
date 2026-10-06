//! Domain surface exposed for the headless debug tools.

use ost::api::client::TeamsClient;

/// Chat list as the GUI shows it (pseudo-feeds filtered).
pub fn list_chats_headless(
    client: &TeamsClient,
    limit: usize,
) -> impl std::future::Future<Output = anyhow::Result<Vec<ost::api::ChatInfo>>> {
    // Pinned to Send so the debug runtime can block on it.
    let fut = ost::api::list_chats_data(client, limit);
    async move {
        let mut chats = fut.await?;
        chats.retain(|c| !c.id.starts_with("48:"));
        chats.sort_by(|a, b| {
            let k = |c: &ost::api::ChatInfo| {
                c.last_message_time
                    .as_deref()
                    .and_then(|t| t.trim().parse::<u64>().ok())
                    .unwrap_or(0)
            };
            k(b).cmp(&k(a))
        });
        Ok(chats)
    }
}
