//! Live domain (AGENT 3 OWNS THIS FILE): trouter session, event-hub drainer,
//! parsed pushes (incoming messages, typing).

use super::Event;
use std::sync::mpsc::Sender;

/// `Command::StartTrouter` — start the websocket + drainer exactly once.
///
/// This is the CURRENT, WORKING implementation — keep its shape:
/// - `tokio::spawn` the trouter session task (it reconnects internally):
///   `ost::trouter::connect_and_run()`
/// - drain on the BLOCKING pool (never on the runtime):
///   `tokio::task::spawn_blocking(move || loop { for ev in
///    ost::event_hub::drain_wait(32, 250) { tx.send(Event::Trouter(ev)) } })`
/// - the caller emits `Event::TrouterConnected` itself after `start()`.
///
/// YOUR ADDITION: forward each raw event to `parse_event` and also emit the
/// higher-level events:
/// - `Event::IncomingMessage { chat_id, sender, preview }` when the JSON is a
///   new chat message (see `parse_event` docs), only for human messages.
/// - `Event::Typing { chat_id, user }` for typing/control pushes.
///
/// Parse defensively with `serde_json::Value`: chat-service push events look
/// like `{ "conversationId": "19:...", "messagetype": "ChatMessage" |
/// "Typing" | "ControlChatState" | ..., "content": "...", "from":
/// "8:orgid:<guid>", "imdisplayname": "...", ... }` but fields vary by event
/// family; never assume — check and fall through to plain `Event::Trouter`
/// whenever unsure. Strip HTML tags from `content` for the preview.
pub fn start(tx: &Sender<Event>) {
    let tx2 = tx.clone();
    tokio::spawn(async {
        if let Err(e) = ost::trouter::connect_and_run().await {
            log::warn!("trouter stopped: {e:#}");
        }
    });

    tokio::task::spawn_blocking(move || loop {
        for ev in ost::event_hub::drain_wait(32, 250) {
            if tx2.send(Event::Trouter(ev)).is_err() {
                return; // UI gone
            }
        }
    });
}
