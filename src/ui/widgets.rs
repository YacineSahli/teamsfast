//! UI widgets: avatars, HTML content rendering.

use crate::model::{avatar_color, initials};
use egui::{Color32, CornerRadius, FontId, Frame, RichText, Stroke, TextFormat, Ui, epaint};

/// A quiet icon button (Lucide icon, ghost style): 30x30 hit area,
/// transparent at rest, surface tint on hover, with a tooltip.
pub fn icon_button(ui: &mut Ui, icon: crate::theme::Icon, tip: &str) -> egui::Response {
    let tint = ui.style().visuals.text_color();
    let btn = egui::Button::new(icon.image(tint, 16.0))
        .fill(Color32::TRANSPARENT)
        .min_size(egui::vec2(30.0, 28.0));
    ui.add(btn).on_hover_text(tip)
}

/// Accent-filled icon button (for the composer's send action).
pub fn accent_icon_button(
    ui: &mut Ui,
    icon: crate::theme::Icon,
    tip: &str,
    accent: Color32,
    on_accent: Color32,
    enabled: bool,
) -> egui::Response {
    let tint = if enabled {
        on_accent
    } else {
        ui.style().visuals.weak_text_color()
    };
    let fill = if enabled {
        accent
    } else {
        ui.style().visuals.extreme_bg_color
    };
    let btn = egui::Button::new(icon.image(tint, 16.0))
        .fill(fill)
        .min_size(egui::vec2(36.0, 32.0));
    ui.add_enabled(enabled, btn).on_hover_text(tip)
}

/// A quiet text button (ghost style).
pub fn ghost_button(ui: &mut Ui, text: &str, tip: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).small())
        .fill(Color32::TRANSPARENT)
        .min_size(egui::vec2(28.0, 28.0));
    ui.add(btn).on_hover_text(tip)
}

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

/// URLs that point at downloadable files rather than web pages.
pub fn is_file_url(url: &str) -> Option<String> {
    let low = url.to_ascii_lowercase();
    let name = url.split('/').next_back().unwrap_or("");
    let obj_store = low.contains("asm.skype.com") || low.contains("/drive/items/");
    let exty = ["pdf", "docx", "xlsx", "pptx", "txt", "zip", "csv", "log", "yaml", "yml", "json"]
        .iter()
        .any(|e| name.to_ascii_lowercase().ends_with(e));
    if obj_store || exty {
        let clean = name
            .split('?')
            .next()
            .unwrap_or(name)
            .to_string();
        Some(if clean.is_empty() { "file".into() } else { clean })
    } else {
        None
    }
}

/// Drop leading LineBreaks (Teams HTML starts with <div>) and collapse
/// consecutive ones — they otherwise render as invisible empty rows that
/// push text to the bottom of every bubble.
fn strip_leading_breaks(segs: Vec<Seg>) -> Vec<Seg> {
    let mut segs = segs;
    while matches!(segs.first(), Some(Seg::LineBreak)) {
        segs.remove(0);
    }
    segs
}

/// Drop whitespace-only text that also contains a newline: it is the
/// `\r\n` between block tags (`</p>\r\n<p>`), which would otherwise add a
/// phantom empty line ON TOP of the LineBreak the tags already produce.
fn strip_block_noise(segs: Vec<Seg>) -> Vec<Seg> {
    // 1. Collapse whitespace runs (incl. newlines) inside text — Teams HTML
    //    is whitespace-liberal ("tool     that"). Block separation is
    //    carried by LineBreak segments, not raw whitespace.
    // 2. Drop text that becomes empty (the \r\n between </p> and <p>).
    let segs: Vec<Seg> = segs
        .into_iter()
        .filter_map(|seg| match seg {
            Seg::Text(t) => {
                let collapsed = t.split_whitespace().collect::<Vec<_>>().join(" ");
                if collapsed.is_empty() {
                    None
                } else {
                    Some(Seg::Text(collapsed))
                }
            }
            other => Some(other),
        })
        .collect();
    // 3. Collapse consecutive LineBreaks to one (blank paragraphs render as
    //    a single gap, like Teams), and trim leading/trailing breaks.
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len());
    for seg in segs {
        if matches!(seg, Seg::LineBreak)
            && matches!(out.last(), Some(Seg::LineBreak) | None)
        {
            continue;
        }
        out.push(seg);
    }
    while matches!(out.last(), Some(Seg::LineBreak) | Some(Seg::Text(_)))
        && out.len() > 1
    {
        match out.last() {
            Some(Seg::LineBreak) => {
                out.pop();
            }
            Some(Seg::Text(t)) if t.trim().is_empty() => {
                out.pop();
            }
            _ => break,
        }
    }
    out
}

/// Render segments inside a wrapped flow. `image` is called for each inline
/// image URL (the app decides whether it is already loaded / must fetch);
/// `file` for downloadable file links (name, url).
pub fn render_segments(
    ui: &mut Ui,
    segs: &[Seg],
    mut image: impl FnMut(&mut Ui, &str) -> bool,
    mut file: impl FnMut(&mut Ui, &str, &str),
) {
    let segs = strip_block_noise(strip_leading_breaks(segs.to_vec()));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let mut row_has_content = false;
        for seg in &segs {
            match seg {
                Seg::Text(t) => {
                    ui.label(RichText::new(t));
                    row_has_content = true;
                }
                Seg::Bold(t) => {
                    ui.label(RichText::new(t).strong());
                    row_has_content = true;
                }
                Seg::Italic(t) => {
                    ui.label(RichText::new(t).italics());
                    row_has_content = true;
                }
                Seg::Code(t) => {
                    row_has_content = true;
                    Frame::default()
                        .fill(Color32::from_rgb(0x2b, 0x2d, 0x31))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(RichText::new(t).monospace().size(12.5));
                        });
                }
                Seg::Link { text, url } => {
                    row_has_content = true;
                    if let Some(fname) = is_file_url(url) {
                        file(ui, &fname, url);
                    } else if text.is_empty() {
                        ui.hyperlink(shorten_link_text(url));
                    } else {
                        ui.hyperlink_to(
                            RichText::new(shorten_link_text(text))
                                .underline()
                                .color(Color32::from_rgb(0x69, 0xa1, 0xe8)),
                            url,
                        );
                    }
                }
                Seg::Mention(t) => {
                    row_has_content = true;
                    ui.label(
                        RichText::new(format!("@{t}"))
                            .color(Color32::from_rgb(0x8a, 0x88, 0xff))
                            .strong(),
                    );
                }
                Seg::Quote(t) => {
                    row_has_content = true;
                    Frame::default()
                        .stroke(Stroke::new(2.0, Color32::from_rgb(0x69, 0xa1, 0xe8)))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(RichText::new(t).weak().small());
                        });
                }
                Seg::Image { url } => {
                    row_has_content = true;
                    image(ui, url);
                }
                Seg::LineBreak => {
                    if row_has_content {
                        ui.end_row();
                        row_has_content = false;
                    }
                }
            }
        }
    });
    let _ = (TextFormat::default(), epaint::Shadow::NONE);
}

/// Long unbreakable tokens (URLs) overflow egui's wrapping; shorten for display.
fn shorten_link_text(text: &str) -> String {
    if text.len() <= 48 {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 48 {
        return text.to_string();
    }
    format!(
        "{}…{}",
        chars[..30].iter().collect::<String>(),
        chars[chars.len() - 16..].iter().collect::<String>()
    )
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

/// Day separator: hairline rules flanking a centered date chip.
pub fn day_separator(ui: &mut Ui, label: &str) {
    let avail = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail, 20.0), egui::Sense::hover());
    let y = rect.center().y;
    let gal = ui.painter().layout(
        label.to_string(),
        egui::FontId::proportional(12.5),
        ui.style().visuals.weak_text_color(),
        avail,
    );
    let tw = gal.mesh_bounds.width();
    ui.painter().galley(
        egui::pos2(rect.center().x - tw / 2.0, y - gal.size().y / 2.0),
        gal,
        ui.style().visuals.weak_text_color(),
    );
    let stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22));
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + 12.0, y),
            egui::pos2(rect.center().x - tw / 2.0 - 10.0, y),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x + tw / 2.0 + 10.0, y),
            egui::pos2(rect.right() - 12.0, y),
        ],
        stroke,
    );
}
