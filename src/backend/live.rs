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
/// - only PARSED events reach the UI (`IncomingMessage` / `Typing` /
///   call events) — the raw-payload clone per push had no consumer and
///   made every message pay a cross-thread String clone.
pub fn start(tx: &Sender<Event>) {
    let tx2 = tx.clone();
    tokio::spawn(async {
        if let Err(e) = ost::trouter::connect_and_run().await {
            log::warn!("trouter stopped: {e:#}");
        }
    });

    tokio::task::spawn_blocking(move || loop {
        for ev in ost::event_hub::drain_wait(32, 250) {
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
                Parsed::IncomingCall {
                    caller_name,
                    caller_mri,
                    has_video,
                    raw,
                } => {
                    // Fork-loss detection: the consumer CC never tells a
                    // losing endpoint (another device answered / caller
                    // cancelled) — its ring otherwise plays the full 45s
                    // window straight THROUGH the ongoing call (live
                    // 2026-10-21, user-audible). Poll the invitation's
                    // progress link; a 4xx means the fork is gone.
                    if let Some(notification) =
                        ost::calling::parse_call_notification(&raw)
                    {
                        let tx3 = tx2.clone();
                        let raw_owned = raw.clone();
                        tokio::spawn(async move {
                            ring_gone_poll(tx3, &raw_owned, &notification).await;
                        });
                    }
                    if tx2
                        .send(Event::IncomingCall {
                            caller_name,
                            caller_mri,
                            has_video,
                            raw,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Parsed::CallGone { call_id } => {
                    if tx2.send(Event::CallGone(call_id)).is_err() {
                        return;
                    }
                }
                Parsed::Other => {}
            }
        }
    });
}


/// While a call rings, poll its invitation `progress` link every 5s. A
/// 4xx response means the fork no longer exists (answered on another
/// endpoint, declined elsewhere, or caller cancelled) — emit RingGone so
/// the UI can stop the ring immediately instead of playing it out the
/// full 45-second window (the consumer CC delivers no fork-loss push).
async fn ring_gone_poll(tx: std::sync::mpsc::Sender<Event>, raw: &str, notification: &ost::calling::CallNotification) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    interval.tick().await; // skip the immediate first tick
    for _ in 0..9 {
        interval.tick().await;
        // Still ringing? The UI re-arms this watcher per invitation; if
        // this call was answered/declined locally, the poll's 404 simply
        // lands on a UI with no matching banner (no-op).
        let config = match ost::config::Config::load_cached() {
            Ok(c) => c,
            Err(_) => return,
        };
        let Some(skype_token) = config.get_skype_token() else {
            return;
        };
        match ost::calling::signaling::send_call_progress(
            &ost::api::client::shared_http(),
            &skype_token.token,
            notification,
        )
        .await
        {
            Ok(()) => {
                log::debug!("ring progress poll ok (fork still live)");
            }
            Err(_gone) => {
                log::info!("CALLQA ring_gone (progress link rejected)");
                let _ = tx.send(Event::RingGone(raw.to_string()));
                return;
            }
        }
    }
}
