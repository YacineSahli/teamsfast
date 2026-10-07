//! Meeting join: turn anything the user can paste (a join URL, a bare
//! meeting thread id, or a 9-15 digit meet ID) into a call leg on the
//! meeting thread, reusing the call driver.

use super::{Event, Session};
use std::sync::mpsc::Sender;

/// Resolve a user-supplied join source into a meeting thread id.
///
/// Accepts:
/// - a full Teams join URL (`/l/meetup-join/19%3ameeting_…` or `/l/meet/<id>`)
/// - a bare thread id (`19:meeting_…@thread.v2`)
/// - a bare meeting ID (digits, optionally with `?p=` passcode)
///
/// Returns `(thread_id, label)` on success; the caller places the call.
pub async fn resolve_join_target(
    ses: &Session,
    tx: &Sender<Event>,
    source: &str,
) -> Option<(String, String)> {
    let Some(c) = ses.client.as_ref() else {
        let _ = tx.send(Event::CallFailed("not signed in".into()));
        return None;
    };
    let target = ost::api::parse_join_url(source);
    match target.kind.as_str() {
        "thread" => {
            let tid = target.thread_id.clone()?;
            Some((tid, "Meeting".to_string()))
        }
        "meeting" | "meet" => {
            let id = target.meeting_id.clone().unwrap_or_default();
            if id.is_empty() {
                let _ = tx.send(Event::CallFailed(
                    "Could not read a meeting ID or thread from that link.".into(),
                ));
                return None;
            }
            let passcode = target.url.split("p=").nth(1).unwrap_or("").to_string();
            match ost::api::resolve_join_meeting_id_data(c, &id, &passcode).await {
                Ok(ost::api::JoinIdResolve::Found(resolved)) => {
                    // The lookup yields a joinWebUrl; parse its thread.
                    let t2 = ost::api::parse_join_url(&resolved.join_web_url);
                    match t2.thread_id {
                        Some(tid) => {
                            let label = resolved.subject.unwrap_or_else(|| "Meeting".into());
                            Some((tid, label))
                        }
                        None => {
                            let _ = tx.send(Event::CallFailed(format!(
                                "Resolved the meeting but could not read its thread from {}",
                                resolved.join_web_url
                            )));
                            None
                        }
                    }
                }
                Ok(ost::api::JoinIdResolve::NotFound) => {
                    let _ = tx.send(Event::CallFailed(
                        "No meeting found for that ID on this account (you must be invited; \
                         or open the link in a browser to join as a guest)."
                            .into(),
                    ));
                    None
                }
                Ok(ost::api::JoinIdResolve::PasscodeMismatch) => {
                    let _ = tx.send(Event::CallFailed(
                        "Wrong meeting passcode — check the invite and append ?p=CODE."
                            .into(),
                    ));
                    None
                }
                Err(e) => {
                    let _ = tx.send(Event::CallFailed(format!("meeting lookup: {e:#}")));
                    None
                }
            }
        }
        other => {
            let _ = tx.send(Event::CallFailed(format!(
                "Unrecognised join source ({other}): paste the full Teams join link, \
                 a meeting thread id, or a meet ID."
            )));
            None
        }
    }
}
