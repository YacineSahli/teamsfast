//! Adaptive Card rendering (basic): bots and connectors post cards as a
//! base64 JSON payload in the message HTML (`Swift b64,…` inside a
//! URIObject). We decode it and render TextBlocks, FactSets, Images and
//! OpenUrl actions; anything fancier degrades to plain text.

use crate::theme::Palette;
use crate::ui::conversation::Action;
use base64::Engine as _;
use egui::{RichText, Ui};
use serde_json::Value;

/// Find the Adaptive Card payload in a message's raw HTML and decode its
/// JSON. Two wire shapes occur: `b64,<payload>` inside a param value and
/// `<Swift b64="<payload>"/>` inside the URIObject element.
pub fn extract_card(raw: &str) -> Option<Value> {
    // `<Swift b64="…"/>` — payload runs to the closing quote.
    if let Some(idx) = raw.find("b64=\"") {
        let rest = &raw[idx + 5..];
        let end = rest.find('"').unwrap_or(rest.len());
        if let Some(card) = decode_b64_card(&rest[..end]) {
            return Some(card);
        }
    }
    // `b64,<payload>` — payload runs to the next delimiter.
    let idx = raw.find("b64,")?;
    let rest = &raw[idx + 4..];
    let end = rest
        .find(|c: char| c == '"' || c == '<' || c.is_whitespace())
        .unwrap_or(rest.len());
    decode_b64_card(&rest[..end])
}

fn decode_b64_card(b64: &str) -> Option<Value> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    // Teams wraps the card: {"attachments":[{"content":{AdaptiveCard}}]}.
    if let Some(content) = json
        .get("attachments")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|att| att.get("content"))
        .filter(|c| c.get("body").is_some())
    {
        return Some(content.clone());
    }
    Some(json)
}

/// Render a decoded Adaptive Card into `ui`. Unknown elements are skipped
/// silently; `actions` receives OpenUrl presses. `salt` keeps widget ids
/// unique when several cards render in one view.
pub fn render_card(ui: &mut Ui, card: &Value, pal: &Palette, actions: &mut Vec<Action>) {
    let Some(body) = card.get("body").and_then(|v| v.as_array()) else {
        return;
    };
    // Unique per message content (same card JSON → same id, different cards
    // in one view never clash).
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    card.to_string().hash(&mut hasher);
    let salt = hasher.finish();
    let width = ui.available_width().min(560.0);
    egui::Frame::default()
        .fill(pal.surface)
        .stroke(egui::Stroke::new(1.0, pal.outline))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.vertical(|ui| {
                // Card summary line (subtle header).
                if let Some(summary) = card.get("summary").and_then(|v| v.as_str())
                    && !summary.trim().is_empty()
                {
                    ui.label(RichText::new(summary).small().weak());
                    ui.add_space(2.0);
                }
                for el in body {
                    render_element(ui, el, pal, salt, actions);
                }
                // Root-level actions.
                if let Some(acts) = card.get("actions").and_then(|v| v.as_array()) {
                    render_actions(ui, acts, pal, actions);
                }
            });
        });
}

fn render_element(
    ui: &mut Ui,
    el: &Value,
    pal: &Palette,
    salt: u64,
    actions: &mut Vec<Action>,
) {
    let kind = el.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "TextBlock" => {
            let text = el.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if text.trim().is_empty() {
                return;
            }
            let size = match el.get("size").and_then(|v| v.as_str()).unwrap_or("") {
                "small" => 11.5,
                "medium" => 15.0,
                "large" | "extraLarge" => 17.0,
                _ => 13.0,
            };
            let weight = el
                .get("weight")
                .and_then(|v| v.as_str())
                .map(|w| w.eq_ignore_ascii_case("bolder"))
                .unwrap_or(false);
            let color = match el.get("color").and_then(|v| v.as_str()).unwrap_or("") {
                "subtle" => pal.secondary,
                "attention" | "warning" => pal.danger,
                "good" => pal.ok,
                "accent" => pal.accent,
                _ => pal.text,
            };
            let mut rt = RichText::new(text).size(size).color(color);
            if weight {
                rt = rt.strong();
            }
            ui.label(rt);
            ui.add_space(2.0);
        }
        "FactSet" => {
            if let Some(facts) = el.get("facts").and_then(|v| v.as_array()) {
                egui::Grid::new(("card-facts", salt))
                    .num_columns(2)
                    .spacing([14.0, 3.0])
                    .show(ui, |ui| {
                        for fact in facts {
                            let title = fact.get("title").and_then(|v| v.as_str()).unwrap_or("");
                            let value = fact.get("value").and_then(|v| v.as_str()).unwrap_or("");
                            ui.label(RichText::new(title).small().color(pal.secondary));
                            ui.label(RichText::new(value).small());
                            ui.end_row();
                        }
                    });
                ui.add_space(2.0);
            }
        }
        "Image" => {
            if let Some(url) = el.get("url").and_then(|v| v.as_str()) {
                let label = el
                    .get("altText")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image");
                if ui
                    .hyperlink_to(
                        RichText::new(format!("🖼 {label}")).small().color(pal.link),
                        url,
                    )
                    .clicked()
                {
                    actions.push(Action::OpenLink(url.to_string()));
                }
                ui.add_space(2.0);
            }
        }
        "ActionSet" => {
            if let Some(acts) = el.get("actions").and_then(|v| v.as_array()) {
                render_actions(ui, acts, pal, actions);
            }
        }
        // Containers: flatten children in order (columns lose their exact
        // proportions, which is fine for a basic renderer).
        "Container" | "ColumnSet" | "Column" | "ShowCard" => {
            let children = el
                .get("items")
                .or_else(|| el.get("columns"))
                .or_else(|| el.get("body"))
                .and_then(|v| v.as_array());
            if let Some(children) = children {
                for child in children {
                    render_element(ui, child, pal, salt, actions);
                }
            }
        }
        _ => {}
    }
}

fn render_actions(ui: &mut Ui, acts: &[Value], pal: &Palette, actions: &mut Vec<Action>) {
    let mut any = false;
    for act in acts {
        let kind = act.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let title = act.get("title").and_then(|v| v.as_str()).unwrap_or("Action");
        match kind {
            "Action.OpenUrl" => {
                let url = act.get("url").and_then(|v| v.as_str()).unwrap_or("");
                if url.is_empty() {
                    continue;
                }
                any = true;
                if ui
                    .add(
                        egui::Button::new(RichText::new(title).small().color(pal.on_accent))
                            .fill(pal.accent)
                            .min_size(egui::vec2(64.0, 24.0)),
                    )
                    .on_hover_text(url)
                    .clicked()
                {
                    actions.push(Action::OpenLink(url.to_string()));
                }
            }
            // Submit/other actions need a bot round-trip — show them inert.
            _ => {
                any = true;
                ui.add_enabled(
                    false,
                    egui::Button::new(RichText::new(title).small())
                        .min_size(egui::vec2(64.0, 24.0)),
                );
            }
        }
    }
    if any {
        ui.add_space(2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_b64_card() {
        let json = r#"{"type":"AdaptiveCard","body":[{"type":"TextBlock","text":"Build ok"}]}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        let raw = format!(
            r#"<object name="swift"><param name="Swift" value="b64,{b64}"/></object>"#
        );
        let card = extract_card(&raw).expect("card decoded");
        assert_eq!(
            card["body"][0]["text"].as_str().unwrap(),
            "Build ok"
        );
    }

    #[test]
    fn no_card_in_plain_html() {
        assert!(extract_card("<div>hello</div>").is_none());
    }

    #[test]
    fn extracts_swift_b64_quote_form_with_envelope() {
        // Real wire shape (Opsgenie): <Swift b64="…"/> wrapping
        // {"attachments":[{"content":{AdaptiveCard…}}]}.
        let card = r#"{"type":"AdaptiveCard","body":[{"type":"TextBlock","text":"P1"}]}"#;
        let envelope = format!(r#"{{"attachments":[{{"content":{card}}}]}}"#);
        let b64 = base64::engine::general_purpose::STANDARD.encode(envelope);
        let raw = format!(r#"<URIObject type="SWIFT.1"><Swift b64="{b64}" /></URIObject>"#);
        let parsed = extract_card(&raw).expect("card decoded");
        assert_eq!(parsed["body"][0]["text"].as_str().unwrap(), "P1");
    }
}
