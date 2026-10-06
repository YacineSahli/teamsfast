//! Live domain: trouter session, event-hub drainer, parsed pushes.

use super::live_parse::{parse, Parsed};
use super::Event;
use std::sync::mpsc::Sender;

/// `Command::StartTrouter` — start the websocket + drainer exactly once.
///
/// Architecture notes (do not regress):
/// - the trouter session is a plain async task (it reconnects internally);
/// - `event_hub::drain_wait` is a blocking Condvar and MUST run on the
///   blocking pool, never on the async runtime;
/// - every raw payload still goes out as `Event::Trouter` (the log panel),
///   parsed ones additionally as `IncomingMessage` / `Typing`.
pub fn start(tx: &Sender<Event>) {
    let tx2 = tx.clone();
    tokio::spawn(async {
        if let Err(e) = ost::trouter::connect_and_run().await {
            log::warn!("trouter stopped: {e:#}");
        }
    });

    tokio::task::spawn_blocking(move || loop {
        for ev in ost::event_hub::drain_wait(32, 250) {
            if tx2.send(Event::Trouter(ev.clone())).is_err() {
                return; // UI gone
            }
            match parse(&ev) {
                Parsed::Incoming {
                    chat_id,
                    sender,
                    preview,
                } => {
                    if tx2
                        .send(Event::IncomingMessage {
                            chat_id,
                            sender,
                            preview,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Parsed::Typing { chat_id, user } => {
                    if tx2.send(Event::Typing { chat_id, user }).is_err() {
                        return;
                    }
                }
                Parsed::Other => {}
            }
        }
    });
}
