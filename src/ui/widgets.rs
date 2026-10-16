//! UI widgets: avatars, HTML content rendering.

use crate::model::{avatar_color, initials};
use crate::theme::Palette;
use egui::{Color32, CornerRadius, FontId, Frame, RichText, Stroke, TextFormat, Ui, epaint};
use std::collections::HashMap;

// ---------------------------------------------------------------- buttons

/// Button heights/radii shared across the app so every surface reads as
/// one design system.
pub mod btn {
    use super::*;

    pub const H: f32 = 28.0; // standard control height
    pub const R: u8 = 8; // control radius (bubbles/cards use 12, chips 6)

    fn base(text: &str, color: Color32) -> egui::Button<'_> {
        egui::Button::new(RichText::new(text).size(12.5).color(color))
            .min_size(egui::vec2(0.0, H))
            .corner_radius(egui::CornerRadius::same(R))
    }

    /// Filled accent — the ONE primary action on a surface.
    pub fn primary(ui: &mut Ui, pal: &Palette, text: &str) -> egui::Response {
        ui.add(base(text, pal.on_accent).fill(pal.accent))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Outlined surface — secondary action.
    pub fn secondary(ui: &mut Ui, pal: &Palette, text: &str) -> egui::Response {
        ui.add(
            base(text, pal.text)
                .fill(pal.surface)
                .stroke(egui::Stroke::new(1.0, pal.outline)),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Filled danger — destructive / hang up.
    pub fn danger(ui: &mut Ui, pal: &Palette, text: &str) -> egui::Response {
        ui.add(base(text, Color32::WHITE).fill(pal.danger))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Text-only quiet action (headers, toolbars).
    pub fn ghost(ui: &mut Ui, pal: &Palette, text: &str) -> egui::Response {
        ui.add(base(text, pal.secondary).fill(Color32::TRANSPARENT))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

/// Ease-in-out tint fade used by every hand-painted hover state: `t` is
/// 0→1 eased over ~120 ms (egui's `animate_value_with_time`), so rows,
/// tiles and icon buttons glide instead of snapping.
pub fn hover_t(ctx: &egui::Context, id: egui::Id, hovered: bool) -> f32 {
    ctx.animate_value_with_time(id, if hovered { 1.0 } else { 0.0 }, 0.12)
}

/// A quiet icon button (Lucide icon, ghost style): 30x28 hit area,
/// transparent at rest, a soft rounded tint fades in on hover.
pub fn icon_button(ui: &mut Ui, icon: crate::theme::Icon, tip: &str) -> egui::Response {
    let tint = ui.style().visuals.text_color();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 28.0), egui::Sense::click());
    let t = hover_t(ui.ctx(), resp.id, resp.hovered());
    if t > 0.01 {
        let pal_fill = ui.style().visuals.widgets.hovered.bg_fill;
        ui.painter()
            .rect_filled(rect, 8.0, tint_over_color(pal_fill, t));
    }
    paint_icon_centered(ui, rect, icon, tint, 16.0);
    resp.on_hover_text(tip)
}

/// Accent-filled icon button (the composer's send action): accent disc at
/// rest armed, recessed tile when disabled, animated hover.
pub fn accent_icon_button(
    ui: &mut Ui,
    icon: crate::theme::Icon,
    tip: &str,
    accent: Color32,
    on_accent: Color32,
    enabled: bool,
) -> egui::Response {
    let size = egui::vec2(32.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hot = resp.hovered() || resp.is_pointer_button_down_on();
    let t = hover_t(ui.ctx(), resp.id, hot);
    if enabled {
        let fill = if resp.is_pointer_button_down_on() {
            tint_over_color(accent, 0.82) // pressed: dip toward the surface
        } else {
            tint_over_color(accent, 1.0 + 0.08 * t) // hover: lighten a touch
        };
        ui.painter().rect_filled(rect, 10.0, fill);
        paint_icon_centered(ui, rect, icon, on_accent, 16.0);
    } else {
        let fill = ui.style().visuals.extreme_bg_color;
        ui.painter().rect_filled(rect, 10.0, fill);
        paint_icon_centered(
            ui,
            rect,
            icon,
            ui.style().visuals.weak_text_color(),
            16.0,
        );
    }
    resp.on_hover_text(tip)
}

/// Blend a color toward white by `t` (0–1) — a cheap "lighten on hover".
fn tint_over_color(c: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |v: u8| (v as f32 + (255.0 - v as f32) * t * 0.12).round() as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

/// Paint a Lucide icon image centered in `rect` (bytes URI loader).
fn paint_icon_centered(
    ui: &mut Ui,
    rect: egui::Rect,
    icon: crate::theme::Icon,
    tint: Color32,
    px: f32,
) {
    let img = egui::Image::from_bytes(icon.uri(), icon.bytes())
        .tint(tint)
        .fit_to_exact_size(egui::vec2(px, px));
    let center = rect.center() - egui::vec2(px / 2.0, px / 2.0);
    img.paint_at(ui, egui::Rect::from_min_size(center, egui::vec2(px, px)));
}

/// A quiet text button (ghost style).


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
    // Render-ready output: the block-noise/leading-break normalization used
    // to run per frame inside render_segments (Vec clone + re-normalize per
    // bubble). Doing it once here means the cache holds final segments and
    // render_segments can borrow them directly.
    let segs = strip_block_noise(strip_leading_breaks(segs));
    segs
}

/// Cached parse of one message's Teams HTML (segments + plain projection).
struct HtmlCacheEntry {
    raw: String,
    segs: std::sync::Arc<Vec<Seg>>,
    plain: std::sync::Arc<String>,
}

/// The conversation re-renders every frame; without this every visible
/// bubble re-parsed its (multi-KB) HTML each frame, several times over
/// (bubble body + own-row measure + previews). Keyed by content hash with
/// a full-string equality check, so edits and collisions can't go stale.
fn html_cache() -> &'static std::sync::Mutex<HashMap<u64, HtmlCacheEntry>> {
    use std::sync::Mutex;
    static CACHE: std::sync::OnceLock<Mutex<HashMap<u64, HtmlCacheEntry>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ensure_html_cached(raw: &str) -> (std::sync::Arc<Vec<Seg>>, std::sync::Arc<String>) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    raw.hash(&mut h);
    let key = h.finish();

    let mut cache = html_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = cache.get(&key)
        && e.raw == raw
    {
        return (e.segs.clone(), e.plain.clone());
    }
    let segs = std::sync::Arc::new(parse_html(raw));
    let plain = std::sync::Arc::new(segs_to_plain(&segs));
    if cache.len() >= 2048 {
        cache.clear(); // crude bound; a miss costs one parse
    }
    cache.insert(
        key,
        HtmlCacheEntry {
            raw: raw.to_string(),
            segs: segs.clone(),
            plain: plain.clone(),
        },
    );
    (segs, plain)
}

/// [`parse_html`] with the per-frame cache — use this on the hot message
/// render path; plain `parse_html` stays for one-off parses.
pub fn parse_html_cached(raw: &str) -> std::sync::Arc<Vec<Seg>> {
    ensure_html_cached(raw).0
}

/// Cached [`segs_to_plain`] over the cached parse (previews, snippets,
/// own-row width measurement).
pub fn plain_cached(raw: &str) -> std::sync::Arc<String> {
    ensure_html_cached(raw).1
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
    pal: &crate::theme::Palette,
    mut image: impl FnMut(&mut Ui, &str) -> bool,
    mut file: impl FnMut(&mut Ui, &str, &str),
) {
    // Segments from `parse_html`/`parse_html_cached` are already
    // render-ready (normalized at parse time); no clone or re-normalize.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let mut row_has_content = false;
        for seg in segs {
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
                    // Chip/block colours derive from the palette — hardcoded
                    // dark fills rendered light-theme code as a dark slab and
                    // same-fill-on-bubble made the card invisible in dark.
                    let chip_fill = crate::theme::tint_over(pal.bubble_in, pal.text, 0.09);
                    let block_fill = crate::theme::tint_over(pal.bubble_in, pal.text, 0.12);
                    // Long chips can't extend past the bubble either —
                    // render them as wrapping blocks instead.
                    if t.contains('\n') || t.chars().count() > 48 {
                        // Multi-line code: own full-width block below the
                        // current row. A Frame inside horizontal_wrapped
                        // inherits the wrapping layout — a wide label
                        // would wrap into a narrow garbled column and
                        // overlap the following segments.
                        if row_has_content {
                            ui.end_row();
                        }
                        Frame::default()
                            .fill(block_fill)
                            .stroke(Stroke::new(1.0, pal.outline.gamma_multiply(0.5)))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new(t.trim_end())
                                        .monospace()
                                        .size(12.5)
                                        .color(pal.text),
                                );
                            });
                        ui.end_row();
                        row_has_content = false;
                    } else {
                        // Inline chip: single line, never wraps inside the
                        // chip (extend mode) — horizontal_wrapped moves the
                        // whole chip to the next line instead.
                        Frame::default()
                            .fill(chip_fill)
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(4, 1))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(t).monospace().size(12.5).color(pal.text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            });
                    }
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
                                .color(pal.link),
                            url,
                        );
                    }
                }
                Seg::Mention(t) => {
                    row_has_content = true;
                    // Solid accent chip with on-accent text — reads on BOTH
                    // bubble fills (bare accent text vanished on the own
                    // indigo bubble; tinted chips matched one fill only).
                    Frame::default()
                        .fill(pal.accent)
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(5, 1))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("@{t}"))
                                        .size(12.5)
                                        .color(pal.on_accent),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .selectable(false),
                            );
                        });
                }
                Seg::Quote(t) => {
                    row_has_content = true;
                    Frame::default()
                        .fill(crate::theme::tint_over(pal.bubble_in, pal.text, 0.05))
                        .stroke(Stroke::new(2.0, pal.accent.gamma_multiply(0.7)))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(8, 4))
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
            Seg::Text(t) | Seg::Bold(t) | Seg::Italic(t) | Seg::Code(t) => out.push_str(t),
            // Mentions render as padded chips in the UI; in PLAIN text
            // (previews, snippets) a chip needs its own spaces — the
            // source often omits them ("…comment<at>Name</at>").
            Seg::Mention(t) => {
                if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
                out.push_str(t);
                out.push('\u{2009}');
            }
            Seg::Link { text, .. } => out.push_str(text),
            Seg::Quote(t) => out.push_str(t),
            Seg::Image { .. } => out.push_str("[image]"),
            Seg::LineBreak => out.push('\n'),
        }
    }
    out.trim_end_matches('\u{2009}').to_string()
}

/// Day separator: hairline rules flanking a centered date chip.
pub fn day_separator(ui: &mut Ui, pal: &Palette, label: &str) {
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
    let stroke = egui::Stroke::new(1.0, pal.outline.gamma_multiply(0.55));
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
