//! Adaptive Card rendering (basic): bots and connectors post cards as a
//! base64 JSON payload in the message HTML (`Swift b64,…` inside a
//! URIObject). We decode it and render TextBlocks, FactSets, Images and
//! OpenUrl actions; anything fancier degrades to plain text.

use crate::theme::Palette;
use crate::ui::conversation::Action;
use base64::Engine as _;
use egui::{RichText, Ui};
use serde_json::Value;
use std::collections::HashMap;
use crate::theme::RichTextBold;

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

struct CardCacheEntry {
    raw: String,
    card: Option<std::sync::Arc<Value>>,
    /// Stable widget-id salt derived from the card JSON (see `render_card`).
    salt: u64,
}

/// The conversation re-renders every frame; without this every card-bearing
/// bubble re-ran the multi-KB HTML scan + base64 decode + serde_json parse
/// (and `render_card` re-stringified the whole card for its id salt) per
/// frame. Keyed by content hash with an equality check, like the segment
/// cache in widgets.rs.
fn card_cache() -> &'static std::sync::Mutex<HashMap<u64, CardCacheEntry>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<u64, CardCacheEntry>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// [`extract_card`] with the per-frame cache; returns the decoded card (if
/// this message carries one) plus its stable id salt.
pub fn extract_card_cached(raw: &str) -> (Option<std::sync::Arc<Value>>, u64) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    raw.hash(&mut h);
    let key = h.finish();

    let mut cache = card_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = cache.get(&key)
        && e.raw == raw
    {
        return (e.card.clone(), e.salt);
    }
    let card = extract_card(raw).map(std::sync::Arc::new);
    let salt = card
        .as_ref()
        .map(|c| {
            let mut sh = DefaultHasher::new();
            c.to_string().hash(&mut sh);
            sh.finish()
        })
        .unwrap_or(0);
    if cache.len() >= 1024 {
        cache.clear();
    }
    cache.insert(
        key,
        CardCacheEntry {
            raw: raw.to_string(),
            card: card.clone(),
            salt,
        },
    );
    (card, salt)
}

/// Render a decoded Adaptive Card into `ui`. Unknown elements are skipped
/// silently; `actions` receives OpenUrl presses. `salt` keeps widget ids
/// unique when several cards render in one view — precomputed once per
/// message content by `extract_card_cached` (hashing the card JSON per
/// frame was a full `to_string` of the payload).
pub fn render_card(
    ui: &mut Ui,
    card: &Value,
    pal: &Palette,
    actions: &mut Vec<Action>,
    salt: u64,
) {
    let Some(body) = card.get("body").and_then(|v| v.as_array()) else {
        return;
    };
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
            // Connector card text (Jira etc.) carries markdown links,
            // literal */_ markers AND pipe tables (the k8s capacity scans
            // post Jira-wiki `||header||` tables) — render link-shaped
            // spans as links, emphasis as styled text, table runs as a
            // striped grid. horizontal_wrapped keeps the pieces INLINE
            // (a label per piece in the vertical layout tore multi-piece
            // text one fragment per line).
            let markdown = text.contains("](") || text.contains('*') || text.contains('_');
            let tableish = text.contains('|');
            if markdown || tableish {
                let mut tbl_idx = 0usize;
                for chunk in crate::ui::widgets::split_pipe_tables(text) {
                    match chunk {
                        crate::ui::widgets::TextChunk::Prose(p) => {
                            let text = p.as_str();
                            if text.trim().is_empty() {
                                continue;
                            }
                            ui.horizontal_wrapped(|ui| {
                                // Tight piece spacing like the chat path —
                                // the default 8px read as a triple space at
                                // every piece boundary carrying a space.
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for piece in crate::ui::widgets::split_markdown_links(text) {
                                    match piece {
                                        crate::ui::widgets::Piece::Plain(t) => {
                                            let mut rt = RichText::new(t).size(size).color(color);
                                            if weight {
                                                rt = rt.bold();
                                            }
                                            ui.label(rt);
                                        }
                                        crate::ui::widgets::Piece::Bold(t) => {
                                            ui.label(RichText::new(t).size(size).color(color).bold());
                                        }
                                        crate::ui::widgets::Piece::Italic(t) => {
                                            ui.label(RichText::new(t).size(size).color(color).italics());
                                        }
                                        crate::ui::widgets::Piece::MdLink { text: lt, url } => {
                                            ui.hyperlink_to(
                                                RichText::new(lt).size(size).underline().color(pal.link),
                                                url,
                                            );
                                        }
                                    }
                                }
                            });
                        }
                        crate::ui::widgets::TextChunk::Table(t) => {
                            ui.add_space(2.0);
                            // ui.id() (unique per bubble) + content salt +
                            // table index: the same card JSON rendered
                            // twice in view (bot resends, forwards) would
                            // collide on a pure content hash and share
                            // column-width memory.
                            let id = ui.id().with((salt, tbl_idx));
                            crate::ui::widgets::render_pipe_table(ui, pal, &t, id, size);
                            ui.add_space(3.0);
                            tbl_idx += 1;
                        }
                    }
                }
            } else {
                let mut rt = RichText::new(text).size(size).color(color);
                if weight {
                    rt = rt.bold();
                }
                ui.label(rt);
            }
            ui.add_space(2.0);
        }
        "FactSet" => {
            if let Some(facts) = el.get("facts").and_then(|v| v.as_array()) {
                // Rows, not a Grid: the grid's value column auto-sized to
                // its content, so one long URL stretched past the card
                // edge and clipped. The value now wraps inside the card
                // width (unbreakable tokens break mid-word — correct for
                // URLs); the title keeps a fixed column so rows align.
                for fact in facts {
                    let title = fact.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let value = fact.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    ui.horizontal(|ui| {
                        // Fixed title column with a hard gutter: the value
                        // starts right after the 92px slot, so long titles
                        // can't crowd it ("Priority: P1" sat 6px apart).
                        let (tr, _) = ui.allocate_exact_size(
                            egui::vec2(92.0 + 14.0, 15.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            tr.left_top(),
                            egui::Align2::LEFT_TOP,
                            title,
                            egui::FontId::proportional(11.5),
                            pal.secondary,
                        );
                        ui.label(RichText::new(value).small());
                    });
                    ui.add_space(2.0);
                }
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
