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
    /// Presence noise, calls, acks — show in the log only.
    Other,
}

/// Parse one published event payload (already stripped of socket.io framing).
pub fn parse(raw: &str) -> Parsed {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return Parsed::Other;
    };

    // Walk: top level, or inside arrays like `channels`/`resources` (each
    // element may carry its own payload).
    let mut candidates: Vec<&Value> = vec![&v];
    if let Some(arr) = v
        .get("channels")
        .or_else(|| v.get("resources"))
        .and_then(|x| x.as_array())
    {
        for item in arr {
            candidates.push(item);
            if let Some(payload) = item.get("payload") {
                candidates.push(payload);
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
    Parsed::Other
}

fn parse_one(v: &Value) -> Option<Parsed> {
    let mt = v
        .get("messagetype")
        .or_else(|| v.get("messageType"))
        .and_then(|x| x.as_str())?;
    let chat_id = v
        .get("conversationId")
        .or_else(|| v.get("conversationid"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    match mt.to_ascii_lowercase().as_str() {
        "typing" | "controlchatstate" => {
            if chat_id.is_empty() {
                return None;
            }
            let user = display_name(v).unwrap_or_else(|| "Someone".into());
            Some(Parsed::Typing { chat_id, user })
        }
        "chatmessage" | "richmessage" => {
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
    out
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
    fn garbage_is_other() {
        assert_eq!(parse("not json at all"), Parsed::Other);
        assert_eq!(parse("[]"), Parsed::Other);
    }

    #[test]
    fn strip_html_cases() {
        assert_eq!(strip_html("<div>Hello <b>world</b></div>"), "Hello world ");
        assert_eq!(strip_html("a &amp; b &lt;c&gt;"), "a & b <c>");
        assert_eq!(strip_html("plain"), "plain");
        assert_eq!(strip_html("<img src=\"x\"> pic"), " pic");
    }
}
