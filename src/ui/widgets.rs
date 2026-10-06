//! UI widgets: avatars, HTML content rendering.

use crate::model::{avatar_color, initials};
use egui::{Color32, CornerRadius, FontId, Frame, RichText, Stroke, TextFormat, Ui, epaint};

/// One avatar circle with initials.
pub fn avatar(ui: &mut Ui, name: &str, size: f32) {
    let color = avatar_color(name);
    let text = initials(name);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::proportional(size * 0.42),
        Color32::WHITE,
    );
}

/// A parsed segment of Teams message HTML.
#[derive(Debug, Clone)]
pub enum Seg {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Link { text: String, url: String },
    Mention(String),
    Image { url: String },
    Quote(String),
    LineBreak,
}

/// Minimal Teams-HTML walker into segments. Unknown tags are unwrapped,
/// their text kept; scripts/styles dropped. Good enough for chat content.
pub fn parse_html(raw: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut text = String::new();
    let mut rest = raw;

    fn flush(text: &mut String, segs: &mut Vec<Seg>) {
        if !text.is_empty() {
            segs.push(Seg::Text(std::mem::take(text)));
        }
    }

    while let Some(lt) = rest.find('<') {
        text.push_str(&decode_entities(&rest[..lt]));
        let after = &rest[lt..];
        let Some(gt_rel) = after.find('>') else {
            rest = ""; // malformed tail
            break;
        };
        let tag = &after[1..gt_rel];
        rest = &after[gt_rel + 1..];
        let lower = tag.to_ascii_lowercase();
        let name = lower.split(|c: char| c == ' ' || c == '\t').next().unwrap_or("");
        match name {
            "br" => flush(&mut text, &mut segs),
            "b" | "strong" | "i" | "em" | "u" | "pre" | "code" | "a" | "at" | "img" | "blockquote"
            | "span" | "div" | "p" | "ol" | "ul" | "li" | "h1" | "h2" | "h3" => {
                // closing?
                if lower.starts_with('/') || name.starts_with('/') {
                    // closing tag — nothing to do, styles applied inline below
                }
                let self_closing = tag.ends_with('/');
                let attr = |key: &str| -> Option<String> {
                    let low = lower.as_str();
                    let pat = format!("{}=\"", key);
                    let p = low.find(&pat)? + pat.len();
                    let e = low[p..].find('"')? + p;
                    Some(low[p..e].to_string())
                };
                match name {
                    "br" => segs.push(Seg::LineBreak),
                    "b" | "strong" => {
                        let (inner, next) = take_until(rest, &format!("</{}", name));
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Bold(decode_entities(&strip_tags(&inner))));
                        rest = next;
                    }
                    "i" | "em" => {
                        let (inner, next) = take_until(rest, &format!("</{}", name));
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Italic(decode_entities(&strip_tags(&inner))));
                        rest = next;
                    }
                    "pre" | "code" => {
                        let (inner, next) = take_until(rest, &format!("</{}", name));
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Code(decode_entities(&strip_tags(&inner))));
                        rest = next;
                    }
                    "a" => {
                        let (inner, next) = take_until(rest, "</a>");
                        let url = attr("href").unwrap_or_default();
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Link {
                            text: decode_entities(&strip_tags(&inner)),
                            url,
                        });
                        rest = next;
                    }
                    "at" => {
                        let (inner, next) = take_until(rest, "</at>");
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Mention(decode_entities(&strip_tags(&inner))));
                        rest = next;
                    }
                    "img" => {
                        if let Some(url) = attr("src").or_else(|| attr("data-src")) {
                            flush(&mut text, &mut segs);
                            segs.push(Seg::Image { url });
                        }
                        if self_closing {
                            // already consumed
                        }
                    }
                    "blockquote" => {
                        let (inner, next) = take_until(rest, "</blockquote>");
                        flush(&mut text, &mut segs);
                        segs.push(Seg::Quote(decode_entities(&strip_tags(&inner))));
                        rest = next;
                    }
                    "div" | "p" | "h1" | "h2" | "h3" | "li" => {
                        segs.push(Seg::LineBreak);
                    }
                    _ => {}
                }
            }
            "script" | "style" => {
                let (_, next) = take_until(rest, &format!("</{}", name));
                rest = next;
            }
            _ => {}
        }
    }
    text.push_str(&decode_entities(rest));
    flush(&mut text, &mut segs);
    segs
}

fn take_until<'a>(s: &'a str, pat: &str) -> (&'a str, &'a str) {
    match s.find(pat) {
        Some(p) => (&s[..p], &s[p..]),
        None => (s, ""),
    }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Render segments inside a wrapped flow. `image` is called for each inline
/// image URL (the app decides whether it is already loaded / must fetch).
pub fn render_segments(
    ui: &mut Ui,
    segs: &[Seg],
    mut image: impl FnMut(&mut Ui, &str) -> bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for seg in segs {
            match seg {
                Seg::Text(t) => {
                    ui.label(RichText::new(t));
                }
                Seg::Bold(t) => {
                    ui.label(RichText::new(t).strong());
                }
                Seg::Italic(t) => {
                    ui.label(RichText::new(t).italics());
                }
                Seg::Code(t) => {
                    Frame::default()
                        .fill(Color32::from_rgb(0x2b, 0x2d, 0x31))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(RichText::new(t).monospace().size(12.5));
                        });
                }
                Seg::Link { text, url } => {
                    if text.is_empty() {
                        ui.hyperlink(url);
                    } else {
                        ui.hyperlink_to(RichText::new(text).underline().color(Color32::from_rgb(0x69, 0xa1, 0xe8)), url);
                    }
                }
                Seg::Mention(t) => {
                    ui.label(
                        RichText::new(format!("@{t}"))
                            .color(Color32::from_rgb(0x8a, 0x88, 0xff))
                            .strong(),
                    );
                }
                Seg::Quote(t) => {
                    Frame::default()
                        .stroke(Stroke::new(2.0, Color32::from_rgb(0x69, 0xa1, 0xe8)))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(RichText::new(t).weak().small());
                        });
                }
                Seg::Image { url } => {
                    image(ui, url);
                }
                Seg::LineBreak => {
                    ui.end_row();
                }
            }
        }
    });
    let _ = (TextFormat::default(), epaint::Shadow::NONE);
}

/// Plain-text projection of segments (for previews / quoted snippets).
pub fn segs_to_plain(segs: &[Seg]) -> String {
    let mut out = String::new();
    for s in segs {
        match s {
            Seg::Text(t) | Seg::Bold(t) | Seg::Italic(t) | Seg::Code(t) | Seg::Mention(t) => {
                out.push_str(t)
            }
            Seg::Link { text, .. } => out.push_str(text),
            Seg::Quote(t) => out.push_str(t),
            Seg::Image { .. } => out.push_str("[image]"),
            Seg::LineBreak => out.push('\n'),
        }
    }
    out
}
