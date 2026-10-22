//! Pure parsing of Trouter push payloads into app-level events.
//!
//! Chat-service pushes arrive as JSON with fields that vary by event family;
//! everything here is defensive: check, fall through to `Other`, never panic.

use serde_json::Value;

/// What a raw Trouter event means to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    /// A human chat message arrived in `chat_id`.
    Incoming {
        chat_id: String,
        sender: String,
        preview: String,
    },
    /// Someone is typing in `chat_id`.
    Typing { chat_id: String, user: String },
    /// An incoming call invitation (NGCallManagerWin push). `raw` is kept
    /// verbatim: accept/decline re-parse it backend-side for the links.
    IncomingCall {
        caller_name: String,
        caller_mri: String,
        has_video: bool,
        raw: String,
    },
    /// A call-signalling callback announced an end (caller cancelled while
    /// ringing, or the remote end hung up on the accepted call). Carries
    /// the push's callAgent id when the URL has one, so a stale call's
    /// gone-push can be told apart from the live call's.
    CallGone { call_id: Option<String> },
    /// Presence noise, acks — show in the log only.
    Other,
}

/// Parse one published event payload (already stripped of socket.io framing).
pub fn parse(raw: &str) -> Parsed {
    // Incoming-call invitations: any envelope that carries a callInvitation.
    // Checked before chat parsing (an invitation is not a chat event); the
    // raw JSON rides along for the backend's accept/decline.
    if ost::calling::parse_call_notification(raw).is_some() {
        if let Some(parsed) = incoming_call(raw) {
            return parsed;
        }
    }

    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return Parsed::Other;
    };

    // Walk: top level, `resource` (chat-service 3::: pushes nest the message
    // there), `payload`, and arrays like `channels`/`resources`.
    let mut candidates: Vec<&Value> = vec![&v];
    if let Some(res) = v.get("resource") {
        candidates.push(res);
    }
    if let Some(arr) = v
        .get("channels")
        .or_else(|| v.get("resources"))
        .and_then(|x| x.as_array())
    {
        for item in arr {
            candidates.push(item);
            if let Some(payload) = item.get("payload") {
                candidates.push(payload);
                if let Some(res) = payload.get("resource") {
                    candidates.push(res);
                }
            }
        }
    }
    if let Some(payload) = v.get("payload") {
        candidates.push(payload);
    }

    for c in candidates {
        if let Some(parsed) = parse_one(c) {
            return parsed;
        }
    }
    // Call-callback envelope (teams-core publishes conversationEnd / call-end
    // callbacks as `callCallback`): an end signal for a live or ringing call.
    if v.get("type").and_then(|t| t.as_str()) == Some("callCallback") {
        if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
            if url.contains("conversationEnd") || url.contains("/call/end") {
                return Parsed::CallGone {
                    call_id: call_agent_id(url),
                };
            }
        }
    }
    Parsed::Other
}

/// The callAgent id from a `…/callAgent/<epid>/<callId>/…` callback URL —
/// the id the CC scopes every callAgent path of ONE call with (the
/// invitation's own links carry it too), letting a gone-push be matched
/// against the call it belongs to.
pub fn call_agent_id(url: &str) -> Option<String> {
    let i = url.find("/callAgent/")? + "/callAgent/".len();
    let rest = &url[i..];
    let mut it = rest.split('/');
    let _epid = it.next()?;
    let id = it.next()?;
    (!id.is_empty()).then(|| id.to_string())
}

/// Scan an invitation raw JSON for any callAgent link and return its id
/// (invitation links: progress/mediaAnswer/acceptance/… all share the
/// caller-side callAgent id).
pub fn call_agent_id_from_invitation(raw: &str) -> Option<String> {
    fn walk(v: &Value, out: &mut Option<String>) {
        match v {
            Value::String(s) => {
                if out.is_none() {
                    if let Some(id) = call_agent_id(s) {
                        *out = Some(id);
                    }
                }
            }
            Value::Array(a) => {
                for x in a {
                    walk(x, out);
                }
            }
            Value::Object(m) => {
                for x in m.values() {
                    walk(x, out);
                }
            }
            _ => {}
        }
    }
    let v = serde_json::from_str::<Value>(raw).ok()?;
    let mut id = None;
    walk(&v, &mut id);
    id
}

/// Extract the ringing event from a callInvitation payload (already known
/// to parse). Caller identity falls back to "Unknown caller"; the raw JSON
/// is carried verbatim.
fn incoming_call(raw: &str) -> Option<Parsed> {
    let n = ost::calling::parse_call_notification(raw)?;
    let (name, mri) = n
        .participants
        .as_ref()
        .and_then(|p| p.from.as_ref())
        .map(|f| {
            (
                f.display_name.clone().unwrap_or_default(),
                f.id.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let has_video = n
        .call_invitation
        .as_ref()
        .and_then(|inv| inv.call_modalities.as_ref())
        .map(|mods| mods.iter().any(|m| m.eq_ignore_ascii_case("video")))
        .unwrap_or(false);
    Some(Parsed::IncomingCall {
        caller_name: if name.trim().is_empty() {
            "Unknown caller".into()
        } else {
            name.trim().to_string()
        },
        caller_mri: mri,
        has_video,
        raw: raw.to_string(),
    })
}

fn parse_one(v: &Value) -> Option<Parsed> {
    let mt = v
        .get("messagetype")
        .or_else(|| v.get("messageType"))
        .and_then(|x| x.as_str())?;
    // conversationId on chat-service pushes; `to`/`conversationLink` on
    // notification-hub pushes (id is the last path segment there).
    let chat_id = v
        .get("conversationId")
        .or_else(|| v.get("conversationid"))
        .or_else(|| v.get("to"))
        .and_then(|x| x.as_str())
        .map(String::from)
        .or_else(|| {
            v.get("conversationLink").and_then(|x| x.as_str()).and_then(|l| {
                l.split("/conversations/").nth(1).map(|rest| {
                    rest.split('/').next().unwrap_or(rest).to_string()
                })
            })
        })
        .unwrap_or_default();

    // Only user conversations (19:...): feeds (48:*), calls and noise are
    // never surfaced as messages.
    if !chat_id.starts_with("19:") {
        return None;
    }

    match mt.to_ascii_lowercase().as_str() {
        "typing" | "controlchatstate" => {
            if chat_id.is_empty() {
                return None;
            }
            let user = display_name(v).unwrap_or_else(|| "Someone".into());
            Some(Parsed::Typing { chat_id, user })
        }
        "chatmessage" | "richmessage" | "richtext/html" | "richtext/plain" | "text" => {
            if chat_id.is_empty() {
                return None;
            }
            // Only human messages: ignore system notes (e.g. "added to
            // thread"), threads/call summaries arrive as EventMessage.
            let content = v
                .get("content")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if content.is_empty() {
                return None;
            }
            let sender = display_name(v).unwrap_or_else(|| "Someone".into());
            let preview = strip_html(&content);
            let preview = preview.lines().next().unwrap_or("").trim().to_string();
            if preview.is_empty() {
                return None;
            }
            Some(Parsed::Incoming {
                chat_id,
                sender,
                preview: preview.chars().take(140).collect(),
            })
        }
        _ => None,
    }
}

fn display_name(v: &Value) -> Option<String> {
    v.get("imdisplayname")
        .or_else(|| v.get("imDisplayname"))
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            // `from` may be "Name <8:orgid:guid>" in some pushes.
            v.get("from")
                .and_then(|x| x.as_str())
                .and_then(|s| s.split('<').next())
                .map(|s| s.trim().trim_end_matches('<').trim().to_string())
                .filter(|s| !s.is_empty() && !s.contains(':'))
        })
}

/// Strip HTML tags and decode the handful of entities Teams uses.
pub fn strip_html(s: &str) -> String {
    if !s.contains('<') && !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut rest = s;
    while let Some(p) = rest.find(['<', '&']) {
        let (head, tail) = rest.split_at(p);
        if depth == 0 {
            out.push_str(head);
        }
        if tail.starts_with('&') {
            // entity up to ';'
            let end = tail.find(';').map(|e| e + 1).unwrap_or(tail.len());
            let ent = &tail[..end];
            let decoded = match ent {
                "&amp;" => "&",
                "&lt;" => "<",
                "&gt;" => ">",
                "&quot;" => "\"",
                "&#39;" | "&apos;" => "'",
                "&nbsp;" => " ",
                _ => ent,
            };
            if depth == 0 {
                out.push_str(decoded);
            }
            rest = &tail[end..];
        } else {
            // tag up to '>'
            let end = tail.find('>').map(|e| e + 1).unwrap_or(tail.len());
            depth = depth.saturating_sub(1);
            // opening <div> or <br> means a line break for previews
            let tag = &tail[..end];
            let low = tag.to_ascii_lowercase();
            if low.starts_with("<span") || low.starts_with("</span") {
                // Connector payloads wrap sentences in spans; glue-free.
                if depth == 0 && !out.is_empty() && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
            }
            if low.starts_with("<div") || low.starts_with("<br") || low.starts_with("</div") {
                if depth == 0 && !out.is_empty() && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
            }
            if !low.starts_with('/') && !low.starts_with("<!") && !low.ends_with("/>") {
                depth = depth.saturating_sub(1);
            }
            rest = &tail[end..];
        }
    }
    if depth == 0 {
        out.push_str(rest);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_message_full_shape() {
        let raw = r#"{"id":"1","type":"EventMessage","time":"2026-10-06T10:12:37Z",
            "conversationId":"19:abc_def@unq.gbl.spaces",
            "from":"8:orgid:11111111-2222-3333-4444-555555555555",
            "imdisplayname":"Ada Lovelace",
            "messagetype":"ChatMessage",
            "content":"<div><p>Hey there</p></div>"}"#;
        assert_eq!(
            parse(raw),
            Parsed::Incoming {
                chat_id: "19:abc_def@unq.gbl.spaces".into(),
                sender: "Ada Lovelace".into(),
                preview: "Hey there".into(),
            }
        );
    }

    #[test]
    fn typing_event() {
        let raw = r#"{"conversationId":"19:abc@thread.v2","messagetype":"Typing",
            "imdisplayname":"Grace Hopper","content":""}"#;
        assert_eq!(
            parse(raw),
            Parsed::Typing {
                chat_id: "19:abc@thread.v2".into(),
                user: "Grace Hopper".into(),
            }
        );
    }

    #[test]
    fn nested_channels_array() {
        let raw = r#"{"id":"9","channels":[{"brainData":[],
            "payload":{"conversationId":"19:x_y@thread.v2","messagetype":"ChatMessage",
            "imdisplayname":"Ada Lovelace","content":"<b>Bold</b> hello"}}]}"#;
        assert_eq!(
            parse(raw),
            Parsed::Incoming {
                chat_id: "19:x_y@thread.v2".into(),
                sender: "Ada Lovelace".into(),
                preview: "Bold hello".into(),
            }
        );
    }

    #[test]
    fn system_messages_are_noise() {
        let raw = r#"{"conversationId":"19:abc@thread.v2","messagetype":"EventMessage",
            "content":"<div>Ada added Ben to the team.</div>"}"#;
        assert_eq!(parse(raw), Parsed::Other);
    }

    #[test]
    fn empty_content_is_noise() {
        let raw = r#"{"conversationId":"19:abc@thread.v2","messagetype":"ChatMessage",
            "imdisplayname":"X","content":""}"#;
        assert_eq!(parse(raw), Parsed::Other);
    }

    #[test]
    fn real_teams_push_shape_richtext_html() {
        // Actual shape observed in production (trouter 3::: decode):
        // outer EventMessage envelope, message under `resource`.
        let raw = r#"{"time":"2026-10-06T15:42:45.0068492Z","type":"EventMessage",
            "resourceLink":"https://notifications.skype.net/v1/users/ME/conversations/48:notes/messages/1",
            "resourceType":"NewMessage",
            "resource":{"clientmessageid":"3669409457167652347",
                "content":"<p>test</p>",
                "from":"https://notifications.skype.net/v1/users/ME/contacts/8:orgid:22222222",
                "imdisplayname":"Grace Hopper",
                "id":"1791301364979",
                "messagetype":"RichText/Html",
                "originalarrivaltime":"2026-10-06T15:42:44.9790000Z",
                "composetime":"2026-10-06T15:42:44.9790000Z",
                "type":"Message",
                "conversationLink":"https://notifications.skype.net/v1/users/ME/conversations/19:abc_def@thread.v2",
                "to":"19:abc_def@thread.v2",
                "threadtype":"streamofnotes","isactive":false}}"#;
        // NOTE: 48:notes is a feed — but this test uses a 19: id in the
        // resource to prove the nested walk works.
        let raw = raw.replace("48:notes", "19:abc_def@thread.v2");
        assert_eq!(
            parse(&raw),
            Parsed::Incoming {
                chat_id: "19:abc_def@thread.v2".into(),
                sender: "Grace Hopper".into(),
                preview: "test".into(),
            }
        );
    }

    #[test]
    fn feeds_are_ignored() {
        // 48:* pseudo-conversations never notify.
        let raw = r#"{"resource":{"id":"48:notes","conversationId":"48:notes",
            "messagetype":"RichText/Html","imdisplayname":"Grace Hopper",
            "content":"<p>test</p>"}}"#;
        assert_eq!(parse(raw), Parsed::Other);
    }

    #[test]
    fn garbage_is_other() {
        assert_eq!(parse("not json at all"), Parsed::Other);
        assert_eq!(parse("[]"), Parsed::Other);
    }

    #[test]
    fn incoming_call_top_level_and_body_envelope() {
        let inv = r#"{"callInvitation":{"callModalities":["Audio"],
            "links":{"acceptance":"https://cc/acc","end":"https://cc/end",
            "mediaAnswer":"https://cc/ma"}},
            "participants":{"from":{"id":"8:orgid:aaaa-bbbb","displayName":"Grace Hopper"}},
            "debugContent":{"callId":"c1"}}"#;
        assert_eq!(
            parse(inv),
            Parsed::IncomingCall {
                caller_name: "Grace Hopper".into(),
                caller_mri: "8:orgid:aaaa-bbbb".into(),
                has_video: false,
                raw: inv.to_string(),
            }
        );

        let wrapped = serde_json::json!({"id": 7, "method": "POST", "body": inv}).to_string();
        match parse(&wrapped) {
            Parsed::IncomingCall {
                caller_name,
                has_video,
                ..
            } => {
                assert_eq!(caller_name, "Grace Hopper");
                assert!(!has_video);
            }
            other => panic!("expected IncomingCall, got {other:?}"),
        }
    }

    #[test]
    fn incoming_call_video_flag_and_name_fallback() {
        let inv = r#"{"callInvitation":{"callModalities":["Audio","Video"]},
            "participants":{"from":{"id":"8:orgid:x"}}}"#;
        match parse(inv) {
            Parsed::IncomingCall {
                caller_name,
                has_video,
                ..
            } => {
                assert_eq!(caller_name, "Unknown caller");
                assert!(has_video);
            }
            other => panic!("expected IncomingCall, got {other:?}"),
        }
    }

    #[test]
    fn call_callback_end_is_call_gone() {
        let gone = r#"{"type":"callCallback",
            "url":"/v4/f/EP/ab12/conversation/conversationEnd/",
            "body":{"reason":"hangup"}}"#;
        assert_eq!(parse(gone), Parsed::CallGone { call_id: None });

        let call_end = r#"{"type":"callCallback","url":"/v4/f/EP/ab12/call/end/","body":null}"#;
        assert_eq!(parse(call_end), Parsed::CallGone { call_id: None });

        // Other call callbacks (roster/progress) stay out of the UI path.
        let roster = r#"{"type":"callCallback","url":"/v4/f/EP/ab12/conversation/rosterUpdate/","body":{}}"#;
        assert_eq!(parse(roster), Parsed::Other);
    }

    #[test]
    fn call_gone_carries_callagent_id() {
        // Real shape (live 2026-10-21): the callback URL is scoped
        // …/callAgent/<epid>/<callId>/call/end/ — the id that ties a
        // gone-push to ONE call.
        let gone = r#"{"type":"callCallback",
            "url":"/v4/f/FLAG/callAgent/00000000-aaaa-bbbb-cccc-111111111111/a8ba95de/call/end/",
            "body":null}"#;
        assert_eq!(
            parse(gone),
            Parsed::CallGone {
                call_id: Some("a8ba95de".into())
            }
        );
        // Extractor edge cases: missing segments, empty id.
        assert_eq!(call_agent_id("/v4/f/x/callAgent/ep1/zz/"), Some("zz".into()));
        assert_eq!(call_agent_id("/v4/f/x/callAgent/ep1/"), None);
        assert_eq!(call_agent_id("/v4/f/x/other/"), None);
        // The invitation raw carries the same id in its own links —
        // the correlation key both sides share.
        let inv = r#"{"callInvitation":{"links":{
            "acceptance":"https://cc/v4/f/FLAG/callAgent/ep-9/deadbeef/call/acceptance/"},
            "callModalities":["Audio"]},
            "participants":{"from":{"id":"8:x","displayName":"A"}}}"#;
        assert_eq!(call_agent_id_from_invitation(inv), Some("deadbeef".into()));
        // Invitations without callAgent links correlate on nothing.
        let bare = r#"{"callInvitation":{"links":{"end":"https://cc/e/1/"},"callModalities":[]}}"#;
        assert_eq!(call_agent_id_from_invitation(bare), None);
    }

    #[test]
    fn chat_event_with_conversation_link_is_not_call_gone() {
        // Invitation-shaped links inside chat pushes must not trip CallGone.
        let msg = r#"{"conversationId":"19:a_b@thread.v2","messagetype":"ChatMessage",
            "imdisplayname":"Ada","content":"<p>links: conversationEnd</p>"}"#;
        assert!(matches!(parse(msg), Parsed::Incoming { .. }));
    }

    #[test]
    fn strip_html_cases() {
        assert_eq!(strip_html("<div>Hello <b>world</b></div>"), "Hello world");
        assert_eq!(strip_html("a &amp; b &lt;c&gt;"), "a & b <c>");
        assert_eq!(strip_html("plain"), "plain");
        assert_eq!(strip_html("<img src=\"x\"> pic"), "pic");
    }
}

#[cfg(test)]
mod hostile_tests {
    use super::*;

    /// Deeply-nested invitation JSON cannot blow the recursive
    /// call_agent_id_from_invitation walk: serde_json's default depth
    /// limit (128) rejects the parse before the walk ever runs.
    #[test]
    fn deep_nesting_is_bounded_by_serde() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(serde_json::from_str::<serde_json::Value>(&deep).is_err());
        assert_eq!(call_agent_id_from_invitation(&deep), None);
        // A flat-but-wide invitation (200 links) walks fine and finds
        // the id wherever it sits.
        let links: Vec<String> = (0..200)
            .map(|i| format!("\"l{i}\":\"https://cc/x/{i}/\""))
            .collect();
        let wide = format!(
            "{{\"links\":{{{}}},\"deep\":\"https://cc/callAgent/ep1/callid99/call/end/\"}}",
            links.join(",")
        );
        assert_eq!(
            call_agent_id_from_invitation(&wide),
            Some("callid99".into())
        );
    }
}
