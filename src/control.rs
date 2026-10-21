//! Runtime control: the single-instance slot doubles as a control
//! channel. Later launches send verbs (`teamsfast --ctl <verb>`); the
//! running instance's claim handler queues them here and wakes the
//! frame loop; `App::ui` drains the queue and applies them as actions.
//! This is what lets the E2E call harness drive two instances from a
//! script — keyboard/mouse injection is impossible in the QA sandbox.

use std::sync::Mutex;

static CONTROL_QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// QA/multi-account profile from `TEAMSFAST_PROFILE`: names the token
/// keyring slot (`tokens-<profile>` in teams-core), the archive keyring
/// entry, and the single-instance slot. The default profile keeps the
/// historical identifiers byte-for-byte (existing installs unchanged).
pub fn profile_suffix() -> String {
    let p = std::env::var("TEAMSFAST_PROFILE").unwrap_or_default();
    let p = p.trim();
    if p.is_empty() || p.eq_ignore_ascii_case("default") {
        String::new()
    } else {
        let sanitized: String = p
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .take(32)
            .collect();
        format!("-{sanitized}")
    }
}

/// A verb the running instance accepts (checked in the claim handler
/// before queueing — anything else is declined with no reply).
pub fn is_control_verb(request: &str) -> bool {
    parse_control_verb(request).is_some()
}

/// Verbs: `ping`, `call <chat_id>`, `videocall <chat_id>`, `echo`
/// (the sanctioned echo-call QA target), `answer`, `decline`,
/// `hangup`, `mute on|off`, `share on|off` (the stage Share button —
/// with TEAMSFAST_SCREENSHARE=1 this starts/stops the synthetic
/// source, letting headless QA exercise the mid-call share
/// slot swap and stop-fallback).
pub fn parse_control_verb(request: &str) -> Option<ControlVerb> {
    let mut parts = request.split_whitespace();
    let head = parts.next()?;
    let arg = |parts: &mut std::str::SplitWhitespace| parts.next().map(str::to_string);
    match head {
        "ping" => Some(ControlVerb::Ping),
        "call" => Some(ControlVerb::Call {
            chat_id: arg(&mut parts)?,
            video: false,
        }),
        "videocall" => Some(ControlVerb::Call {
            chat_id: arg(&mut parts)?,
            video: true,
        }),
        "echo" => Some(ControlVerb::Echo),
        "open" => Some(ControlVerb::Open {
            chat_id: arg(&mut parts)?,
        }),
        "send" => {
            let chat_id = arg(&mut parts)?;
            let text = parts.collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                None
            } else {
                Some(ControlVerb::Send { chat_id, text })
            }
        }
        "calluser" => Some(ControlVerb::CallUser {
            upn: arg(&mut parts)?,
        }),
        "answer" => Some(ControlVerb::Answer),
        "decline" => Some(ControlVerb::Decline),
        "hangup" => Some(ControlVerb::HangUp),
        "mute" => match arg(&mut parts).as_deref() {
            Some("on") => Some(ControlVerb::Mute(true)),
            Some("off") => Some(ControlVerb::Mute(false)),
            _ => None,
        },
        "share" => match arg(&mut parts).as_deref() {
            Some("on") => Some(ControlVerb::Share(true)),
            Some("off") => Some(ControlVerb::Share(false)),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlVerb {
    Ping,
    Call { chat_id: String, video: bool },
    Echo,
    Open { chat_id: String },
    Send { chat_id: String, text: String },
    /// Cold-start call: create (or fetch) the 1:1 with the UPN, then
    /// place an audio call once the chat lands (CreateChatOk).
    CallUser { upn: String },
    Answer,
    Decline,
    HangUp,
    Mute(bool),
    /// Stage Share button: start (synthetic source under
    /// TEAMSFAST_SCREENSHARE=1) or stop the running share.
    Share(bool),
}

/// Queue one verb (called from the claim-handler thread).
pub fn push_control_verb(request: &str) {
    CONTROL_QUEUE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(request.to_owned());
}

/// Drain the queue (called once per frame on the UI thread).
pub fn take_control_verbs() -> Vec<String> {
    std::mem::take(&mut *CONTROL_QUEUE.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_verbs_parse() {
        assert_eq!(
            parse_control_verb("ping"),
            Some(ControlVerb::Ping)
        );
        assert_eq!(
            parse_control_verb("call 19:abc@thread.v2"),
            Some(ControlVerb::Call {
                chat_id: "19:abc@thread.v2".into(),
                video: false
            })
        );
        assert_eq!(
            parse_control_verb("videocall 19:abc@thread.v2"),
            Some(ControlVerb::Call {
                chat_id: "19:abc@thread.v2".into(),
                video: true
            })
        );
        assert_eq!(parse_control_verb("echo"), Some(ControlVerb::Echo));
        assert_eq!(
            parse_control_verb("calluser qa2@example.com"),
            Some(ControlVerb::CallUser {
                upn: "qa2@example.com".into()
            })
        );
        assert_eq!(parse_control_verb("calluser"), None);
        assert_eq!(parse_control_verb("answer"), Some(ControlVerb::Answer));
        assert_eq!(parse_control_verb("decline"), Some(ControlVerb::Decline));
        assert_eq!(parse_control_verb("hangup"), Some(ControlVerb::HangUp));
        assert_eq!(parse_control_verb("mute on"), Some(ControlVerb::Mute(true)));
        assert_eq!(parse_control_verb("mute off"), Some(ControlVerb::Mute(false)));
        assert_eq!(
            parse_control_verb("share on"),
            Some(ControlVerb::Share(true))
        );
        assert_eq!(
            parse_control_verb("share off"),
            Some(ControlVerb::Share(false))
        );
        // Unknown or malformed: declined, never queued.
        assert_eq!(parse_control_verb("show"), None);
        assert_eq!(parse_control_verb("rm -rf"), None);
        assert_eq!(parse_control_verb("call"), None);
        assert_eq!(parse_control_verb("mute"), None);
        assert_eq!(parse_control_verb("mute sideways"), None);
        assert_eq!(parse_control_verb("share"), None);
        assert_eq!(parse_control_verb("share maybe"), None);
        assert_eq!(parse_control_verb(""), None);
    }

    #[test]
    fn profile_suffix_defaults_to_empty() {
        // No env set in tests: the default profile must keep historical
        // identifiers ("teamsfast", "archive-key") byte-for-byte.
        // (TEAMSFAST_PROFILE is process-wide; if a test run ever sets it,
        // this asserts the sanitized shape instead.)
        match std::env::var("TEAMSFAST_PROFILE").as_deref() {
            Err(_) | Ok("") | Ok("default") | Ok("Default") => {
                assert_eq!(profile_suffix(), "");
            }
            _ => {
                assert!(profile_suffix().starts_with('-'));
                assert!(profile_suffix().len() <= 33);
            }
        }
    }
}
