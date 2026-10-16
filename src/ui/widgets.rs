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

/// Rounded-square glyph for TEAMS (the circle is the PEOPLE shape — teams
/// rendered as circles read as persons in the sidebar).
pub fn avatar_tile(ui: &mut Ui, name: &str, size: f32) {
    let color = avatar_color(name);
    let text = initials(name);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, (size * 0.25) as u8, color);
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
    /// An @-mention. `mri` arrives inline on `<at id="…">`; the span-index
    /// shape (schema.skype.com/Mention) has none — the click handler falls
    /// back to resolving the name against the chat roster.
    Mention { name: String, mri: Option<String> },
    Image { url: String },
    Quote(String),
    LineBreak,
}

/// One piece of a markdown-split text span.
pub enum Piece {
    Plain(String),
    Bold(String),
    Italic(String),
    MdLink { text: String, url: String },
}

/// Split a text span into plain pieces and `[text](https://…)` markdown
/// links. Hand-rolled scan (no regex dep): find `](`, expand to the
/// enclosing `[` and the closing `)`, accept only when the paren part is
/// an absolute http(s) URL with no whitespace. Unbalanced or non-link
/// text passes through untouched; the scan always advances.
pub fn split_markdown_links(t: &str) -> Vec<Piece> {
    let bytes = t.as_bytes();
    let mut out: Vec<Piece> = Vec::new();
    let mut plain_start = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            // Text end: the '[' opening this link.
            let Some(open) = t[..i].rfind('[') else {
                i += 2;
                continue;
            };
            if open < plain_start {
                // '[' belongs to an already-emitted piece — not a link.
                i += 2;
                continue;
            }
            let Some(close_rel) = t[i + 2..].find(')') else {
                i += 2;
                continue;
            };
            let url = &t[i + 2..i + 2 + close_rel];
            let is_http = url.starts_with("http://") || url.starts_with("https://");
            let has_ws = url.bytes().any(|b| b.is_ascii_whitespace());
            if !is_http || has_ws {
                i += 2;
                continue;
            }
            if open > plain_start {
                out.extend(scan_emphasis(&t[plain_start..open]));
            }
            out.push(Piece::MdLink {
                text: t[open + 1..i].to_string(),
                url: url.to_string(),
            });
            plain_start = i + 2 + close_rel + 1;
            i = plain_start;
        } else {
            i += 1;
        }
    }
    if plain_start < t.len() {
        out.extend(scan_emphasis(&t[plain_start..]));
    }
    out
}

/// Conservative `**bold**` / `_italic_` pass over a plain (link-free)
/// segment — connector cards (Jira) post these markers literally. Rules
/// keep false positives out: no newlines inside, no `***` runs, and `_`
/// never applies intraword (`file_name_here` stays literal, markdown's
/// own intraword rule). Char-indexed — byte slicing on multi-byte input
/// panics.
fn scan_emphasis(s: &str) -> Vec<Piece> {
    if !s.contains('*') && !s.contains('_') {
        return vec![Piece::Plain(s.to_string())];
    }
    let ch: Vec<char> = s.chars().collect();
    let mut out: Vec<Piece> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < ch.len() {
        // **bold**: opening needs a non-*, non-space char after it; the
        // closing ** must not continue a run and needs a non-space before.
        if ch[i] == '*'
            && i + 3 < ch.len()
            && ch[i + 1] == '*'
            && ch[i + 2] != '*'
            && !ch[i + 2].is_whitespace()
        {
            let mut j = i + 3;
            let mut close = None;
            while j + 1 < ch.len() {
                if ch[j] == '*' && ch[j + 1] == '*' && ch[j - 1] != '*' && !ch[j - 1].is_whitespace()
                {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(j) = close {
                let inner: String = ch[i + 2..j].iter().collect();
                if !inner.contains('\n') {
                    if !plain.is_empty() {
                        out.push(Piece::Plain(std::mem::take(&mut plain)));
                    }
                    out.push(Piece::Bold(inner));
                    i = j + 2;
                    continue;
                }
            }
        }
        // _italic_: markdown's intraword rule — the opening _ must not sit
        // between alphanumerics, the content must not start with space,
        // the closing _ must not follow space or end inside a word.
        if ch[i] == '_'
            && (i == 0 || !ch[i - 1].is_alphanumeric())
            && i + 2 < ch.len()
            && !ch[i + 1].is_whitespace()
            && ch[i + 1] != '_'
        {
            let mut j = i + 2;
            let mut close = None;
            while j < ch.len() {
                if ch[j] == '_'
                    && !ch[j - 1].is_whitespace()
                    && (j + 1 == ch.len() || !ch[j + 1].is_alphanumeric())
                {
                    close = Some(j);
                    break;
                }
                if ch[j] == '\n' {
                    break;
                }
                j += 1;
            }
            if let Some(j) = close {
                let inner: String = ch[i + 1..j].iter().collect();
                if !plain.is_empty() {
                    out.push(Piece::Plain(std::mem::take(&mut plain)));
                }
                out.push(Piece::Italic(inner));
                i = j + 1;
                continue;
            }
        }
        // *bold* (Jira wiki, what this tenant's connectors actually post):
        // same boundary discipline as `_` — the opener must not sit
        // intraword or next to another asterisk, content must not start
        // with space (kills "* bullet" lists and "2 * 3" arithmetic).
        if ch[i] == '*'
            && i + 1 < ch.len()
            && ch[i + 1] != '*'
            && (i == 0 || (ch[i - 1] != '*' && !ch[i - 1].is_alphanumeric()))
            && !ch[i + 1].is_whitespace()
        {
            let mut j = i + 2;
            let mut close = None;
            while j < ch.len() {
                if ch[j] == '*'
                    && ch[j - 1] != '*'
                    && !ch[j - 1].is_whitespace()
                    && (j + 1 == ch.len()
                        || (!ch[j + 1].is_alphanumeric() && ch[j + 1] != '*'))
                {
                    close = Some(j);
                    break;
                }
                if ch[j] == '\n' {
                    break;
                }
                j += 1;
            }
            if let Some(j) = close {
                let inner: String = ch[i + 1..j].iter().collect();
                if !plain.is_empty() {
                    out.push(Piece::Plain(std::mem::take(&mut plain)));
                }
                out.push(Piece::Bold(inner));
                i = j + 1;
                continue;
            }
        }
        plain.push(ch[i]);
        i += 1;
    }
    if !plain.is_empty() {
        out.push(Piece::Plain(plain));
    }
    out
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
                        segs.push(Seg::Mention {
                            name: decode_entities(&strip_tags(&inner)),
                            // Real MRIs contain ':' ("8:orgid:…"); a bare
                            // index like itemid="0" is not an mri.
                            mri: attr("id").filter(|id| id.contains(':')),
                        });
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
                    "span" => {
                        if lower.contains("schema.skype.com/mention") {
                            let (inner, next) = take_until(rest, "</span>");
                            flush(&mut text, &mut segs);
                            segs.push(Seg::Mention {
                                name: decode_entities(&strip_tags(&inner)),
                                mri: None,
                            });
                            rest = next;
                        }
                        // other spans: unwrap, text flows
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
    // An @-mention chip was clicked (name, inline mri if the wire had one).
    mut mention: impl FnMut(&mut Ui, &str, Option<&str>),
) {
    // Segments from `parse_html`/`parse_html_cached` are already
    // render-ready (normalized at parse time); no clone or re-normalize.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let mut row_has_content = false;
        for seg in segs {
            match seg {
                Seg::Text(t) => {
                    row_has_content = true;
                    // Connectors (Jira etc.) post markdown links inside the
                    // message HTML; the stripper keeps them as literal
                    // "[text](url)" bracket soup. Render link-shaped spans
                    // as links (conservative: only ](http…) counts — any
                    // other bracket text is untouched).
                    if t.contains("](") || t.contains('*') || t.contains('_') {
                        for piece in split_markdown_links(t) {
                            match piece {
                                Piece::Plain(text) => {
                                    ui.label(RichText::new(text));
                                }
                                Piece::Bold(text) => {
                                    ui.label(RichText::new(text).strong());
                                }
                                Piece::Italic(text) => {
                                    ui.label(RichText::new(text).italics());
                                }
                                Piece::MdLink { text, url } => {
                                    if text.is_empty() {
                                        ui.hyperlink(shorten_link_text(&url));
                                    } else {
                                        ui.hyperlink_to(
                                            RichText::new(shorten_link_text(&text))
                                                .underline()
                                                .color(pal.link),
                                            url,
                                        );
                                    }
                                }
                            }
                        }
                    } else {
                        ui.label(RichText::new(t));
                    }
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
                Seg::Mention { name, mri } => {
                    row_has_content = true;
                    // Solid accent chip with on-accent text — reads on BOTH
                    // bubble fills (bare accent text vanished on the own
                    // indigo bubble; tinted chips matched one fill only).
                    // Sized to its text and CLICKABLE: hover lightens the
                    // fill and the hand cursor reads it as a link; click
                    // opens the person's contact card via `mention`.
                    let label = format!("@{name}");
                    let gal = ui.painter().layout_no_wrap(
                        label,
                        egui::FontId::proportional(12.5),
                        pal.on_accent,
                    );
                    let size = gal.size() + egui::vec2(10.0, 2.0);
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                    let t = hover_t(ui.ctx(), resp.id, resp.hovered());
                    let fill = if t > 0.01 {
                        crate::theme::tint_over(pal.accent, pal.on_accent, 0.22 * t)
                    } else {
                        pal.accent
                    };
                    ui.painter().rect_filled(rect, 4, fill);
                    ui.painter().galley(
                        rect.left_top() + egui::vec2(5.0, 1.0),
                        gal,
                        pal.on_accent,
                    );
                    if resp.clicked() {
                        mention(ui, name, mri.as_deref());
                    }
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("View profile");
                }
                Seg::Quote(t) => {
                    row_has_content = true;
                    // Left accent bar only — a full outline read as a
                    // selected/focused card; the threading cue is the bar.
                    let quote = Frame::default()
                        .fill(crate::theme::tint_over(pal.bubble_in, pal.text, 0.05))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            // Body-adjacent size — .small() read as fine
                            // print for content the user is meant to parse.
                            ui.label(RichText::new(t).weak().size(13.0));
                        });
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            quote.response.rect.left_top(),
                            egui::vec2(2.5, quote.response.rect.height()),
                        ),
                        1,
                        pal.accent.gamma_multiply(0.8),
                    );
                }
                Seg::Image { url } => {
                    // Images always own their line: break before AND after
                    // so a picture never hangs mid-sentence.
                    if row_has_content {
                        ui.end_row();
                    }
                    image(ui, url);
                    ui.end_row();
                    row_has_content = false;
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
            Seg::Mention { name, .. } => {
                if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
                out.push_str(name);
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

#[cfg(test)]
mod markdown_link_tests {
    use super::split_markdown_links;
    use super::Piece;

    fn pieces(t: &str) -> Vec<(String, Option<(String, String)>)> {
        split_markdown_links(t)
            .into_iter()
            .map(|p| match p {
                Piece::Plain(s) | Piece::Bold(s) | Piece::Italic(s) => (s, None),
                Piece::MdLink { text, url } => (text.clone(), Some((text, url))),
            })
            .collect()
    }

    fn emph(t: &str) -> Vec<Piece> {
        split_markdown_links(t)
    }

    #[test]
    fn bold_markers_become_bold() {
        let got = emph("a **b** c");
        assert_eq!(got.len(), 3);
        assert!(matches!(&got[0], Piece::Plain(s) if s == "a "));
        assert!(matches!(&got[1], Piece::Bold(s) if s == "b"));
        assert!(matches!(&got[2], Piece::Plain(s) if s == " c"));
    }

    #[test]
    fn italic_word_boundary_rules() {
        // Intraword underscores stay literal (markdown's own rule).
        let got = emph("file_name_here ok");
        assert_eq!(got.len(), 1);
        assert!(matches!(&got[0], Piece::Plain(s) if s == "file_name_here ok"));
        // Real emphasis around word boundaries.
        let got = emph("see _this note_ end");
        assert_eq!(got.len(), 3);
        assert!(matches!(&got[1], Piece::Italic(s) if s == "this note"));
    }

    #[test]
    fn emphasis_never_crosses_lines_or_runs() {
        // A newline inside kills the pair; so does a *** run.
        for t in ["**a\nb** tail", "**** head", "** no close ** x"] {
            let got = emph(t);
            assert_eq!(got.len(), 1, "{t}");
            assert!(matches!(&got[0], Piece::Plain(_)), "{t}");
        }
    }

    #[test]
    fn multibyte_text_with_emphasis() {
        let got = emph("á **vérifié** ü");
        assert_eq!(got.len(), 3);
        assert!(matches!(&got[1], Piece::Bold(s) if s == "vérifié"));
    }

    #[test]
    fn single_asterisk_bold_jira_style() {
        // The live connector payload shape: *bold* at word boundaries.
        let got = emph("*Scan of hardcoded values* tail");
        assert_eq!(got.len(), 2);
        assert!(matches!(&got[0], Piece::Bold(s) if s == "Scan of hardcoded values"));
        // Table-cell markers convert.
        let got = emph("|scp|*170*|");
        assert!(matches!(&got[1], Piece::Bold(s) if s == "170"));
    }

    #[test]
    fn single_asterisk_false_positives_stay_literal() {
        // Bullets, arithmetic, intraword: none may become bold.
        for t in [
            "* bullet item",
            "2 * 3 = 6",
            "a*b*c",
            "unpaired * star",
            "trailing star*",
        ] {
            let got = emph(t);
            assert_eq!(got.len(), 1, "{t}");
            assert!(matches!(&got[0], Piece::Plain(_)), "{t}");
        }
    }

    #[test]
    fn jira_style_emphasis_with_link() {
        // Link extraction wins; emphasis fills the plain tails.
        let got = emph("**Failed:** see [SCP-1](https://x.io) _now_");
        assert!(matches!(&got[0], Piece::Bold(s) if s == "Failed:"));
        assert!(matches!(&got[1], Piece::Plain(s) if s == " see "));
        assert!(matches!(&got[2], Piece::MdLink { text, .. } if text == "SCP-1"));
        assert!(matches!(&got[3], Piece::Plain(s) if s == " "));
        assert!(matches!(&got[4], Piece::Italic(s) if s == "now"));
    }

    #[test]
    fn jira_style_links_become_links() {
        let got = pieces("See [QA-1002 job failed](https://qa-tenant.atlassian.net/browse/QA-1002?atlOrigin=x) now");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].0, "See ");
        assert_eq!(got[1].0, "QA-1002 job failed");
        assert_eq!(
            got[1].1,
            Some((
                "QA-1002 job failed".to_string(),
                "https://qa-tenant.atlassian.net/browse/QA-1002?atlOrigin=x".to_string()
            ))
        );
        assert_eq!(got[2].0, " now");
    }

    #[test]
    fn plain_brackets_stay_plain() {
        // No http(s) → untouched; unbalanced → untouched.
        assert_eq!(pieces("array[0] (weird)"), vec![("array[0] (weird)".to_string(), None)]);
        assert_eq!(
            pieces("[not a link](ftp://x)"),
            vec![("[not a link](ftp://x)".to_string(), None)]
        );
        assert_eq!(pieces("[still not](https://no closing"), vec![("[still not](https://no closing".to_string(), None)]);
    }

    #[test]
    fn jira_real_payload_with_quotes_links() {
        let t = concat!(
            "[QA-1001 Statuspage userscript: add \"open public page\" and ",
            "\"copy public link\" buttons](https://qa-tenant.atlassian.net/browse/",
            "QA-1001?atlOrigin=eyJpIjoiQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQLCJw",
            "IjoibXN0ZWFtcy1qaXJhIn0)"
        );
        let got = pieces(t);
        assert_eq!(got.len(), 1);
        let (text, url) = got[0].1.clone().expect("must be a link");
        assert!(text.starts_with("QA-1001 Statuspage"));
        assert!(url.starts_with("https://qa-tenant.atlassian.net/browse/QA-1001"));
    }

    #[test]
    fn multiple_links_and_multibyte_text() {
        let got = pieces("á [a](https://x.io) mid [b](http://y.io) ü");
        assert_eq!(got.len(), 5);
        assert_eq!(got[0].0, "á ");
        assert_eq!(got[2].0, " mid ");
        assert_eq!(got[4].0, " ü");
        assert_eq!(
            got[3].1.as_ref().map(|(_, u)| u.as_str()),
            Some("http://y.io")
        );
    }
}

#[cfg(test)]
mod mention_tests {
    use super::{parse_html, render_segments, Seg};
    use crate::theme::Palette;

    #[test]
    fn at_tag_mri_is_mined_from_id() {
        let segs = parse_html(r#"<p>hi <at id="8:orgid:abc-def">Bob</at>!</p>"#);
        let mention = segs.iter().find_map(|s| match s {
            Seg::Mention { name, mri } => Some((name.clone(), mri.clone())),
            _ => None,
        });
        assert_eq!(
            mention,
            Some(("Bob".to_string(), Some("8:orgid:abc-def".to_string())))
        );
    }

    #[test]
    fn span_itemtype_mention_is_a_mention_without_mri() {
        let segs = parse_html(
            r#"<span itemtype="http://schema.skype.com/Mention" itemscope="" itemid="0">Mathias Schreurs</span>&nbsp;reviewed this"#,
        );
        let mention = segs.iter().find_map(|s| match s {
            Seg::Mention { name, mri } => Some((name.clone(), mri.clone())),
            _ => None,
        });
        assert_eq!(
            mention,
            Some(("Mathias Schreurs".to_string(), None))
        );
        assert!(
            segs.iter().any(|s| matches!(s, Seg::Text(t) if t.contains("reviewed"))),
            "the non-mention tail stays text"
        );
    }

    #[test]
    fn plain_span_is_not_a_mention() {
        let segs = parse_html(r#"<span style="font-weight:bold">just text</span>"#);
        assert!(
            !segs.iter().any(|s| matches!(s, Seg::Mention { .. })),
            "ordinary spans must not become mention chips"
        );
    }

    #[test]
    fn clicking_a_mention_chip_fires_the_hook() {
        crate::emoji::setup();
        let segs = parse_html(r#"<at id="8:orgid:abc">Bob</at> hello"#);
        let egui_ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let blank = egui::RawInput { screen_rect: Some(screen), ..Default::default() };

        let clicked: std::rc::Rc<std::cell::RefCell<Vec<(String, Option<String>)>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

        let draw = |egui_ctx: &egui::Context,
                    input: egui::RawInput,
                    clicked: &std::rc::Rc<std::cell::RefCell<Vec<(String, Option<String>)>>>| {
            let sink = clicked.clone();
            let mut out = egui_ctx.run_ui(input, |ui| {
                render_segments(
                    ui,
                    &segs,
                    &Palette::dark(),
                    |_, _| false,
                    |_, _, _| {},
                    |_, name, mri| {
                        sink.borrow_mut()
                            .push((name.to_string(), mri.map(str::to_string)));
                    },
                );
            });
            out.textures_delta.clear();
        };

        // Warm-up frames (hit tests run one frame behind).
        draw(&egui_ctx, blank.clone(), &clicked);
        draw(&egui_ctx, blank.clone(), &clicked);

        // Sweep-click the row; the chip sits in the first text line.
        'outer: for y in (6..200).step_by(7) {
            for x in (6..300).step_by(5) {
                let pos = egui::pos2(x as f32, y as f32);
                let hover = egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerMoved(pos)],
                    ..Default::default()
                };
                draw(&egui_ctx, hover, &clicked);
                let press = egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    }],
                    ..Default::default()
                };
                draw(&egui_ctx, press, &clicked);
                let release = egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    }],
                    ..Default::default()
                };
                draw(&egui_ctx, release, &clicked);
                if !clicked.borrow().is_empty() {
                    break 'outer;
                }
            }
        }
        let got = clicked.borrow().clone();
        assert_eq!(
            got,
            vec![("Bob".to_string(), Some("8:orgid:abc".to_string()))],
            "sweep-clicking the row must hit the mention chip exactly once"
        );
    }
}
