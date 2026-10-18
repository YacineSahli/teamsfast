//! UI widgets: avatars, HTML content rendering.

use crate::model::{avatar_color, initials};
use crate::theme::Palette;
use egui::{Color32, CornerRadius, FontId, Frame, RichText, Stroke, TextFormat, Ui, epaint};
use std::collections::HashMap;
use crate::theme::RichTextBold;

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

/// One parsed pipe table from connector text. This tenant's k8s capacity
/// scans post Jira-wiki tables (`||header||` first row, `|cell|` data
/// rows) inside card TextBlocks; markdown-shaped tables (`|h|h|` +
/// `|---|---|` + rows) are accepted too.
#[derive(Debug, Clone, PartialEq)]
pub struct PipeTable {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Width/length caps: a pasted bomb degrades to truncated cells/rows
/// instead of a pathological layout pass.
pub const TABLE_MAX_COLS: usize = 12;
const TABLE_MAX_ROWS: usize = 60;

/// A line belongs to a table when it starts with `|` after optional
/// spaces. Wireshark filters, Jira attachment markup and SQL pastes all
/// start with other characters, so this alone is conservative.
fn table_line(l: &str) -> bool {
    l.trim_start().starts_with('|')
}

/// Markdown separator row: pipe-separated runs of dashes/colons/spaces
/// only (`| --- | :---: |`).
fn is_sep_row(line: &str) -> bool {
    let t = line.trim();
    let inner = t.strip_prefix('|').unwrap_or(t);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    !inner.trim().is_empty()
        && inner.split('|').all(|seg| {
            !seg.trim().is_empty()
                && seg.trim().chars().all(|c| c == '-' || c == ':' || c == ' ' || c == '\t')
        })
}

/// Split one table line into cells. `\|` is a literal pipe; the leading
/// and trailing delimiters are dropped, so a source-truncated row (no
/// trailing `|`) still parses. Jira headers delimit with `||`.
fn split_cells(line: &str, jira_header: bool) -> Vec<String> {
    let t = line.trim();
    let delim = if jira_header { "||" } else { "|" };
    let body = t.strip_prefix(delim).unwrap_or(t);
    let body = body.strip_suffix(delim).unwrap_or(body);
    let mut cells: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => {
                if !jira_header || chars.peek() == Some(&'|') {
                    cells.push(cur.trim().to_string());
                    cur.clear();
                    if jira_header {
                        chars.next();
                    }
                } else {
                    // lone '|' inside a header line is data
                    cur.push('|');
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !cells.is_empty() {
        cells.push(cur.trim().to_string());
    }
    cells
}

/// Parse a run of table lines (caller guarantees ≥2 via `table_line`).
fn parse_pipe_run(run: &[&str]) -> PipeTable {
    let mut header: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let first = run[0].trim();
    if first.starts_with("||") {
        header = split_cells(first, true);
    }
    for (k, line) in run.iter().enumerate() {
        if k == 0 && !header.is_empty() {
            continue;
        }
        if is_sep_row(line) {
            // markdown separator directly after the first line: that line
            // was the header, not a data row
            if k == 1 && header.is_empty() && !rows.is_empty() {
                header = rows.remove(0);
            }
            continue;
        }
        let cells = split_cells(line, false);
        if !cells.is_empty() {
            rows.push(cells);
        }
    }
    // Normalize to a bounded rectangle.
    let cols = header
        .len()
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0))
        .clamp(1, TABLE_MAX_COLS);
    header.truncate(cols);
    for r in &mut rows {
        if r.len() > cols {
            // keep overflow data visible inside the last kept cell
            let rest = r.split_off(cols);
            let joined = rest.join(" | ");
            if let Some(last) = r.last_mut() {
                if last.is_empty() {
                    *last = joined;
                } else {
                    last.push_str(" | ");
                    last.push_str(&joined);
                }
            }
        }
        r.resize(cols, String::new());
    }
    if rows.len() > TABLE_MAX_ROWS {
        let extra = rows.len() - TABLE_MAX_ROWS;
        rows.truncate(TABLE_MAX_ROWS);
        let mut note = vec![format!("… +{extra} more rows")];
        note.resize(cols, String::new());
        rows.push(note);
    }
    PipeTable { header, rows }
}

/// Alternating prose/table decomposition of one text.
#[derive(Debug, Clone, PartialEq)]
pub enum TextChunk {
    Prose(String),
    Table(PipeTable),
}

/// Split text into prose paragraphs and pipe tables. A table is a run of
/// ≥2 consecutive lines that each start with `|` (after trim); shorter
/// runs — and pipe-free text — pass through as prose with every `\n`
/// kept. Used by the card TextBlock path; the chat path groups segments
/// first (strip_block_noise collapses raw newlines) and calls
/// `parse_pipe_run` directly.
pub fn split_pipe_tables(text: &str) -> Vec<TextChunk> {
    if !text.contains('|') {
        return vec![TextChunk::Prose(text.to_string())];
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<TextChunk> = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if table_line(lines[i]) {
            let mut j = i;
            while j < lines.len() && table_line(lines[j]) {
                j += 1;
            }
            if j - i >= 2 {
                if !prose.is_empty() {
                    out.push(TextChunk::Prose(prose.join("\n")));
                    prose.clear();
                }
                out.push(TextChunk::Table(parse_pipe_run(&lines[i..j])));
                i = j;
                continue;
            }
        }
        prose.push(lines[i]);
        i += 1;
    }
    if !prose.is_empty() {
        out.push(TextChunk::Prose(prose.join("\n")));
    }
    if out.is_empty() {
        out.push(TextChunk::Prose(String::new()));
    }
    out
}

/// Render a parsed pipe table as a striped grid. `id` must be stable
/// across frames (Grid persists column widths under it) and unique per
/// table position in the view.
pub fn render_pipe_table(ui: &mut Ui, pal: &crate::theme::Palette, tbl: &PipeTable, id: egui::Id, size: f32) {
    let cols = tbl
        .header
        .len()
        .max(tbl.rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if cols == 0 || (tbl.header.is_empty() && tbl.rows.is_empty()) {
        return;
    }
    let cap = pipe_cell_cap(ui, tbl, cols, size);
    egui::Grid::new(id)
        .striped(true)
        .num_columns(cols)
        .min_col_width(24.0)
        .spacing(egui::vec2(12.0, 4.0))
        .show(ui, |ui| {
            for h in &tbl.header {
                ui.label(RichText::new(h).size(size).bold());
            }
            if !tbl.header.is_empty() {
                ui.end_row();
            }
            for row in &tbl.rows {
                for c in row {
                    render_table_cell(ui, pal, c, size, cap);
                }
                ui.end_row();
            }
        });
}

/// Rough average glyph width of the proportional chat font (fraction of
/// the font size). Only used to PREDICT whether a table MIGHT overflow —
/// tables that clearly fit skip the real measurement entirely.
const PIPE_CHAR_W: f32 = 0.62;

/// Column spacing of the table Grid (kept in sync with `.spacing` above).
const PIPE_COL_GAP: f32 = 12.0;

/// Per-cell character budget: the natural 36-char look whenever the table
/// fits the available width, uniformly shrunk when it clearly doesn't — a
/// wide paste must never escape its bubble (an escaped row grows the
/// parent's min_rect and poisons the layout math of every row after it).
///
/// The cheap estimate errs wide; borderline tables get a real painter
/// measurement (one galley per column — its widest cell) and a 10% grace
/// band so a table over by a few px keeps its rendered look (the k8s
/// right-sizing card sits in that band). Only genuinely wide pastes are
/// crushed, floored at 4 chars per column.
fn pipe_cell_cap(ui: &Ui, tbl: &PipeTable, cols: usize, size: f32) -> usize {
    const NATURAL_CAP: usize = 36;
    let budget = ui.available_width();
    let mut col_chars = vec![0usize; cols];
    let mut widest: Vec<&str> = vec![""; cols];
    for row in std::iter::once(&tbl.header).chain(tbl.rows.iter()) {
        for (j, cell) in row.iter().enumerate() {
            if j < cols {
                let n = cell.chars().count().min(NATURAL_CAP);
                if n >= col_chars[j] {
                    col_chars[j] = n;
                    widest[j] = cell.as_str();
                }
            }
        }
    }
    let spacing = PIPE_COL_GAP * cols.saturating_sub(1) as f32;
    let est_natural: f32 = col_chars.iter().map(|&c| c as f32 * size * PIPE_CHAR_W).sum::<f32>()
        + spacing;
    // Wide-glyph content (CJK, emoji) runs ~1.0x size per char, not the
    // 0.62 Latin average — the estimate would err NARROW, skip the real
    // measurement, and let the table escape (a self-latching overflow:
    // the escaped row inflates every later row's available width).
    // Non-ASCII anywhere: straight to the measured path.
    let has_wide = tbl
        .header
        .iter()
        .chain(tbl.rows.iter().flatten())
        .any(|c| c.chars().any(|ch| !ch.is_ascii()));
    if budget <= 0.0 || (!has_wide && est_natural <= budget * 0.95) {
        return NATURAL_CAP;
    }
    // Borderline or wide: measure the real per-column widths before
    // deciding (a few galleys, only for tables the estimate flags).
    let font = egui::FontId::proportional(size);
    let mut natural = spacing;
    for w in &widest {
        natural += ui
            .painter()
            .layout(
                (*w).to_owned(),
                font.clone(),
                egui::Color32::WHITE,
                f32::INFINITY,
            )
            .mesh_bounds
            .width();
    }
    // 10% grace: the k8s right-sizing card's natural width lands in the
    // (1.03, 1.10] band of its budget — at 3% its cells truncated
    // ("management-…", "512Mi → 384…"; vision-verified) — so the wider
    // band is load-bearing. Residual: an OWN message with a table at the
    // slot edge can overhang ~10% (rare content; degrades that row's
    // anchor, self-heals on relayout; the wide-glyph latch is closed
    // above).
    if natural <= budget * 1.10 {
        return NATURAL_CAP;
    }
    let total_chars: f32 = col_chars.iter().map(|&c| c as f32).sum::<f32>().max(1.0);
    let avg_char_w = (natural - spacing) / total_chars;
    let avail_chars = ((budget - spacing - 8.0) / avg_char_w.max(0.5) / cols as f32)
        .floor()
        .max(4.0) as usize;
    avail_chars.min(NATURAL_CAP)
}

/// One grid cell. Cells keep emphasis/links (`*Increase*` action cells are
/// real payloads), but grid cells don't wrap — long values ellipsize
/// (char-safe) with the full text on hover.
fn render_table_cell(ui: &mut Ui, pal: &crate::theme::Palette, text: &str, size: f32, cap: usize) {
    let shown: String = if text.chars().count() > cap {
        let mut s: String = text.chars().take(cap - 1).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    };
    let truncated = shown.len() < text.len();
    if !shown.contains('*') && !shown.contains('_') && !shown.contains("](") {
        let rt = ui.label(RichText::new(&shown).size(size));
        if truncated {
            rt.on_hover_text(text);
        }
        return;
    }
    let rt = ui
        .horizontal(|ui| {
            for piece in split_markdown_links(&shown) {
                match piece {
                    Piece::Plain(t) => {
                        ui.label(RichText::new(t).size(size));
                    }
                    // Cells keep their emphasis: the Jira payload's
                    // "*Increase*" action column must render bold.
                    Piece::Bold(t) => {
                        ui.label(RichText::new(t).size(size).bold());
                    }
                    Piece::Italic(t) => {
                        ui.label(RichText::new(t).size(size).italics());
                    }
                    Piece::MdLink { text: lt, url } => {
                        ui.hyperlink_to(
                            RichText::new(shorten_link_text(&lt)).size(size).underline().color(pal.link),
                            url,
                        );
                    }
                }
            }
        })
        .response;
    if truncated {
        rt.on_hover_text(text);
    }
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
        // Normalize the tag name before matching: the first token of
        // `<br/>` is "br/" and of `</div>` is "/div" — neither matched
        // any arm, so self-closing tags were dropped entirely (`<p>a
        // <br/>b</p>` fused to "ab") and closing block tags never broke
        // the line ("x</div>y" concatenated).
        let base = name.trim_matches('/');
        let closing = name.starts_with('/');
        match base {
            // `<br>` must BOTH flush pending text and break the line. A
            // long-lived bug kept only the flush (the LineBreak push sat
            // in an unreachable inner arm), so multi-line messages —
            // every `<br>`-separated list Teams produces — rendered
            // flattened into one wrapped paragraph.
            "br" => {
                flush(&mut text, &mut segs);
                segs.push(Seg::LineBreak);
            }
            "b" | "strong" | "i" | "em" | "u" | "pre" | "code" | "a" | "at" | "img" | "blockquote"
            | "span" | "div" | "p" | "ol" | "ul" | "li" | "h1" | "h2" | "h3" => {
                if closing {
                    // Closing block tags break the line like their opens
                    // (the open's break fires BEFORE its text, so without
                    // this the close of "x</div>y" fused the neighbors).
                    // Inline closings are no-ops: their content was
                    // consumed at the open tag. Consecutive breaks
                    // collapse in strip_block_noise.
                    if matches!(base, "div" | "p" | "h1" | "h2" | "h3" | "li" | "ol" | "ul") {
                        flush(&mut text, &mut segs);
                        segs.push(Seg::LineBreak);
                    }
                } else {
                    let self_closing = tag.ends_with('/');
                    let attr = |key: &str| -> Option<String> {
                        let low = lower.as_str();
                        let pat = format!("{}=\"", key);
                        let p = low.find(&pat)? + pat.len();
                        let e = low[p..].find('"')? + p;
                        Some(low[p..e].to_string())
                    };
                    // XHTML self-closing non-img tags are EMPTY: drop the
                    // tag and let text flow. The consuming arms' take_until
                    // has no closer to find — "<b/>rest" swallowed the
                    // whole remainder as one Bold run (red-team catch).
                    if self_closing && base != "img" {
                        continue;
                    }
                    match base {
                        "b" | "strong" => {
                            let (inner, next) = take_until(rest, &format!("</{}>", base));
                            flush(&mut text, &mut segs);
                            segs.push(Seg::Bold(decode_entities(&strip_tags(&inner))));
                            rest = next;
                        }
                        "i" | "em" => {
                            let (inner, next) = take_until(rest, &format!("</{}>", base));
                            flush(&mut text, &mut segs);
                            segs.push(Seg::Italic(decode_entities(&strip_tags(&inner))));
                            rest = next;
                        }
                        "pre" | "code" => {
                            let (inner, next) = take_until(rest, &format!("</{}>", base));
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
                            // Block open breaks the line — and must FLUSH the
                            // pending text first: without the flush, "a</p><p>b"
                            // stayed in one text buffer and the paragraphs
                            // concatenated ("ab") while the LineBreaks piled up
                            // at the front (strip_block_noise then dropped them).
                            flush(&mut text, &mut segs);
                            segs.push(Seg::LineBreak);
                        }
                        _ => {}
                    }
                }
            }
            "script" | "style" => {
                let (_, next) = take_until(rest, &format!("</{}>", base));
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
    let mut rest = s;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after = &rest[lt..];
        let Some(gt_rel) = after.find('>') else {
            // malformed tail: drop it (an unclosed '<' skips to the end)
            return out;
        };
        let tag = &after[1..gt_rel];
        let name = tag
            .trim_matches('/')
            .split(|c: char| c == ' ' || c == '\t')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        // Block tags imply whitespace: quote payloads run
        // "…</strong><p itemprop=preview>And…" and plain concatenation
        // fused the author into the preview ("GrguricinAnd we are…").
        if matches!(
            name.as_str(),
            "p" | "div" | "br" | "blockquote" | "li" | "h1" | "h2" | "h3"
        ) && !out.is_empty()
            && !out.ends_with(char::is_whitespace)
        {
            out.push(' ');
        }
        rest = &after[gt_rel + 1..];
    }
    out.push_str(rest);
    // Closing block tags append a boundary space even at the very end
    // ("<strong>A<p>B</p></strong>" -> "A B "); previews read better
    // without the trailing one.
    while out.ends_with(' ') {
        out.pop();
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
/// True when the text contains an unbreakable run too long for a chat
/// line (URLs, Jira's `name.mp4|width=1219,alt="…"` media params). Such a
/// token overflows the wrap and grows the central panel's min_rect past
/// the screen edge; the wider wrap width then differs between the cached
/// paint and the network settle, and every affected row re-wraps by a
/// line (run-5 layout-shift forensics).
fn has_pathological_token(t: &str) -> bool {
    t.split_whitespace().any(|w| w.chars().count() > 48)
}

/// Plain label that breaks over-long tokens mid-word instead of
/// overflowing. egui 0.36's `Label` Wrap mode never sets
/// `wrap.break_anywhere`; a hand-built LayoutJob is the only access.
fn label_breaking_long_tokens(ui: &mut Ui, t: &str) -> egui::Response {
    if has_pathological_token(t) {
        let font_id = ui
            .style()
            .text_styles
            .get(&egui::TextStyle::Body)
            .cloned()
            .unwrap_or_else(|| egui::FontId::proportional(14.0));
        // Wrap at the CONTAINER width, not the remaining line: this label
        // follows earlier pieces on a wrapped row, and available_width()
        // there is the current line's leftover — wrapping there produced
        // a tall per-character column. The over-wide galley moves to the
        // next full line (red-team P2-3).
        let wrap_w = ui.max_rect().width().max(1.0);
        let mut job = egui::text::LayoutJob::simple(
            t.to_owned(),
            font_id,
            ui.visuals().text_color(),
            wrap_w,
        );
        job.wrap.break_anywhere = true;
        ui.add(egui::Label::new(job).selectable(false))
    } else {
        ui.label(RichText::new(t))
    }
}

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
        let mut idx = 0;
        while idx < segs.len() {
            // Pipe-table run: `<br>`-separated rows arrive as Text segs
            // split by LineBreak segs (strip_block_noise collapses raw
            // newlines, so multi-line text only takes this shape). Group
            // ≥2 leading-pipe rows and render as a grid before the
            // per-segment match; a lone pipe line falls through as text.
            if let Seg::Text(t) = &segs[idx]
                && t.trim_start().starts_with('|')
                && idx + 2 < segs.len()
            {
                let mut lines: Vec<&str> = vec![t.as_str()];
                let mut j = idx + 1;
                while j + 1 < segs.len() {
                    if matches!(segs[j], Seg::LineBreak)
                        && let Seg::Text(t2) = &segs[j + 1]
                        && t2.trim_start().starts_with('|')
                    {
                        lines.push(t2.as_str());
                        j += 2;
                    } else {
                        break;
                    }
                }
                if lines.len() >= 2 {
                    if row_has_content {
                        ui.end_row();
                    }
                    let tbl = parse_pipe_run(&lines);
                    let id = ui.id().with(("pipe-table", idx, lines.len()));
                    render_pipe_table(ui, pal, &tbl, id, 13.0);
                    ui.end_row();
                    row_has_content = false;
                    idx = j;
                    continue;
                }
            }
            let seg = &segs[idx];
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
                                    label_breaking_long_tokens(ui, &text);
                                }
                                Piece::Bold(text) => {
                                    ui.label(RichText::new(text).bold());
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
                        label_breaking_long_tokens(ui, t);
                    }
                }
                Seg::Bold(t) => {
                    ui.label(RichText::new(t).bold());
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
            idx += 1;
        }
    });
    let _ = (TextFormat::default(), epaint::Shadow::NONE);
}

/// Long unbreakable tokens (URLs) overflow egui's wrapping; shorten for display.
pub fn shorten_link_text(text: &str) -> String {
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
                // Full-width trailing space: the thin space (U+2009) read
                // as fused with the next word in one-line previews
                // ("TimmermansIt seems…").
                out.push(' ');
            }
            Seg::Link { text, .. } => out.push_str(text),
            // Quotes are block-level: the text after one starts a fresh
            // sentence ("NameAnd we are still not done" fused in previews).
            Seg::Quote(t) => {
                out.push_str(t);
                if !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
            }
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
        let egui_ctx = crate::theme::test_ctx();
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

#[cfg(test)]
mod br_tests {
    // `<br>` used to only flush pending text (the LineBreak push sat in an
    // unreachable inner match arm), so multi-line messages rendered as one
    // flattened paragraph. This pins the fix.
    use super::{parse_html, Seg};

    #[test]
    fn br_produces_linebreak() {
        let segs = parse_html("line one<br>line two");
        let shape: Vec<&str> = segs
            .iter()
            .map(|s| match s {
                Seg::Text(t) => t.as_str(),
                Seg::LineBreak => "\n",
                _ => "?",
            })
            .collect();
        assert_eq!(shape, vec!["line one", "\n", "line two"]);
    }

    #[test]
    fn blank_br_runs_collapse_to_one_break() {
        let segs = parse_html("a<br><br>b");
        let breaks = segs.iter().filter(|s| matches!(s, Seg::LineBreak)).count();
        assert_eq!(breaks, 1, "Teams renders blank lines as a single gap");
    }

    fn shape(segs: &[Seg]) -> Vec<&str> {
        segs.iter()
            .map(|s| match s {
                Seg::Text(t) => t.as_str(),
                Seg::LineBreak => "\n",
                _ => "?",
            })
            .collect()
    }

    // Self-closing `<br/>` (no space before the slash): the first name
    // token was "br/", which matched no arm, so the tag vanished —
    // "<p>a<br/>b</p>" rendered as one fused line "ab".
    #[test]
    fn self_closing_br_breaks() {
        assert_eq!(
            shape(&parse_html("<p>a<br/>b</p>")),
            vec!["a", "\n", "b"]
        );
    }

    // Closing block tags must break like their opens: "x</div>y"
    // concatenated to "xy" because "/div" matched nothing.
    #[test]
    fn closing_block_tag_breaks() {
        assert_eq!(shape(&parse_html("<div>x</div>y")), vec!["x", "\n", "y"]);
        assert_eq!(shape(&parse_html("<p>a</p>b")), vec!["a", "\n", "b"]);
    }

    // The run-3 fix must survive the closing-tag break: consecutive
    // breaks (close + reopen) collapse to one gap.
    #[test]
    fn p_p_still_two_lines_with_closing_breaks() {
        assert_eq!(
            shape(&parse_html("<p>a</p><p>b</p>")),
            vec!["a", "\n", "b"]
        );
    }

    // Close-pattern precision: searching for "</b" (no closing bracket)
    // matched inside "</blockquote>", truncating bold content early and
    // leaking the remainder as plain text.
    #[test]
    fn bold_close_does_not_match_blockquote_close() {
        let segs = parse_html("<b>a<blockquote>q</blockquote>b</b>");
        match &segs[0] {
            Seg::Bold(t) => assert_eq!(t, "a q b"),
            other => panic!("expected Bold, got {other:?}"),
        }
        assert_eq!(segs.len(), 1, "nothing may leak past the bold close");
    }

    // XHTML self-closing INLINE tags are empty — "<b/>rest" must not
    // swallow the remainder as one styled run (take_until has no closer).
    #[test]
    fn self_closing_inline_tag_flows_text() {
        assert_eq!(shape(&parse_html("<i/>plain")), vec!["plain"]);
        assert_eq!(shape(&parse_html("a<b/>c")), vec!["ac"]);
        assert_eq!(shape(&parse_html("<b/>x<br/>y")), vec!["x", "\n", "y"]);
    }
}

#[cfg(test)]
mod strip_tags_tests {
    use super::strip_tags;

    #[test]
    fn self_closing_br_gets_block_space() {
        assert_eq!(strip_tags("a<br/>b"), "a b");
        assert_eq!(strip_tags("a<br />b"), "a b");
    }

    #[test]
    fn closing_block_space_but_no_trailing() {
        assert_eq!(strip_tags("<strong>A<p>B</p></strong>"), "A B");
        assert_eq!(strip_tags("x<div>y</div>"), "x y");
    }
}

#[cfg(test)]
mod pipe_table_tests {
    use super::{parse_html, render_pipe_table, split_pipe_tables, TextChunk};

    /// The real k8s-capacity TextBlock shape: Jira `||header||` row, pipe
    /// data rows, and a row truncated mid-cell by the connector (the
    /// source caps comment length — "ast..." ends the payload).
    #[test]
    fn jira_shape_parses_with_truncated_tail() {
        let text = concat!(
            " Resource request findings\n",
            "\n",
            "||Workload||Container||CPU req → suggested||Mem req → suggested||Action||\n",
            "|qa-default|api|0.500 → 0.20|512Mi → 384Mi|Reduce|\n",
            "|qa-default|management-api|0.200 → 0.05|500Mi → 384Mi|Reduce|\n",
            "|qa-default|ast..."
        );
        let chunks = split_pipe_tables(text);
        assert_eq!(chunks.len(), 2, "prose + one table");
        assert!(matches!(&chunks[0], TextChunk::Prose(p) if p.contains("Resource request")));
        let TextChunk::Table(t) = &chunks[1] else {
            panic!("second chunk must be the table");
        };
        assert_eq!(
            t.header,
            vec![
                "Workload".to_string(),
                "Container".to_string(),
                "CPU req → suggested".to_string(),
                "Mem req → suggested".to_string(),
                "Action".to_string(),
            ]
        );
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.rows[0][0], "qa-default");
        assert_eq!(t.rows[0][4], "Reduce");
        // truncated row keeps its cells, padded to the header width
        assert_eq!(t.rows[2][0], "qa-default");
        assert_eq!(t.rows[2][1], "ast...");
        assert_eq!(t.rows[2].len(), 5);
    }

    #[test]
    fn markdown_shape_separator_makes_header() {
        let chunks = split_pipe_tables("|H1|H2|\n|---|---|\n|a|b|");
        let TextChunk::Table(t) = &chunks[0] else {
            panic!("must be a table");
        };
        assert_eq!(t.header, vec!["H1".to_string(), "H2".to_string()]);
        assert_eq!(t.rows, vec![vec!["a".to_string(), "b".to_string()]]);
    }

    /// Lone pipe lines (SQL pastes, Wireshark `||` filters, `a | b`
    /// prose) never trigger a table: two CONSECUTIVE pipe-starting lines
    /// are required.
    #[test]
    fn single_pipe_lines_stay_prose() {
        let text = "| d143a72db18de646 | 1 | 0 |";
        assert!(matches!(split_pipe_tables(text).as_slice(), [TextChunk::Prose(p)] if p == text));
        let wireshark = "{noformat}sip.Call-ID == \"x\" || sip.Call-ID == \"y\"{noformat}";
        assert!(matches!(split_pipe_tables(wireshark).as_slice(), [TextChunk::Prose(p)] if p == wireshark));
    }

    #[test]
    fn escaped_pipe_is_literal() {
        let chunks = split_pipe_tables("|a\\|b|c|\n|d|e|f|");
        let TextChunk::Table(t) = &chunks[0] else {
            panic!("must be a table");
        };
        assert_eq!(t.rows[0][0], "a|b");
        assert_eq!(t.rows[0][1], "c");
    }

    #[test]
    fn wide_and_long_runs_cap() {
        let mut wide = String::from("||h0||h1||h2");
        for i in 3..18 {
            wide.push_str(&format!("||h{i}"));
        }
        wide.push_str("||\n");
        wide.push_str(&"|c0|c1|c2|c3|c4|c5|c6|c7|c8|c9|c10|c11|c12|c13|c14");
        let chunks = split_pipe_tables(&wide);
        let TextChunk::Table(t) = &chunks[0] else {
            panic!("must be a table");
        };
        assert_eq!(t.header.len(), super::TABLE_MAX_COLS);
        assert!(t.rows[0].len() == super::TABLE_MAX_COLS);
        assert!(t.rows[0].last().unwrap().contains("c11"));
        assert!(t.rows[0].last().unwrap().contains("c14"), "overflow merges into the last cell");

        let mut long = String::from("|a|\n|b|");
        for i in 0..70 {
            long.push_str(&format!("\n|r{i}|"));
        }
        let chunks = split_pipe_tables(&long);
        let TextChunk::Table(t) = &chunks[0] else {
            panic!("must be a table");
        };
        assert_eq!(t.rows.len(), super::TABLE_MAX_ROWS + 1);
        assert!(t.rows.last().unwrap()[0].contains("+12 more rows"));
    }

    /// `<br>`-separated chat rows produce exactly the Text/LineBreak/Text
    /// segment shape the render_segments lookahead groups.
    #[test]
    fn chat_rows_arrive_as_linebreak_split_segs() {
        let segs = parse_html("|a|b|<br>|c|d|<br>|e|f|");
        let shape: Vec<&str> = segs
            .iter()
            .map(|s| match s {
                super::Seg::Text(t) if t.trim_start().starts_with('|') => "T",
                super::Seg::LineBreak => "L",
                _ => "?",
            })
            .collect();
        assert_eq!(shape, vec!["T", "L", "T", "L", "T"]);
    }

    /// Render smoke test: tables render without panicking and two grids
    /// in one frame get distinct ids (the "Second use of Grid ID" trap).
    #[test]
    fn render_two_tables_smoke() {
        crate::emoji::setup();
        let ctx = crate::theme::test_ctx();
        let pal = crate::theme::Palette::dark();
        let t1 = super::parse_pipe_run(&["||A||B||", "|1|2|", "|3|4|"]);
        let t2 = super::parse_pipe_run(&["||C||D||", "|x|y|"]);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                super::render_pipe_table(ui, &pal, &t1, egui::Id::new("tbl-a"), 13.0);
                super::render_pipe_table(ui, &pal, &t2, egui::Id::new("tbl-b"), 13.0);
                let segs = parse_html("|a|b|<br>|c|d|");
                super::render_segments(ui, &segs, &pal, |_, _| false, |_, _, _| {}, |_, _, _| {});
            });
            out.textures_delta.clear();
        }
    }
}



#[cfg(test)]
mod block_break_tests {
    // `<p>a</p><p>b</p>` used to concatenate ("ab"): the block-open arm
    // pushed LineBreak without flushing pending text, so both paragraphs
    // stayed in one buffer and the break landed before both. This pins
    // the flush.
    use super::{parse_html, Seg};

    fn shape(html: &str) -> Vec<&'static str> {
        parse_html(html)
            .iter()
            .map(|s| match s {
                Seg::Text(t) => match t.as_str() {
                    "a" => "a",
                    "b" => "b",
                    _ => "t",
                },
                Seg::LineBreak => "L",
                _ => "?",
            })
            .collect()
    }

    #[test]
    fn p_boundaries_break() {
        assert_eq!(shape("<p>a</p><p>b</p>"), vec!["a", "L", "b"]);
        assert_eq!(shape("a<div>b</div><div>c</div>"), vec!["a", "L", "b", "L", "t"]);
    }
}



#[cfg(test)]
mod quote_plain_tests {
    use super::{parse_html, segs_to_plain};

    #[test]
    fn quote_then_text_gets_a_separator() {
        // The real reply-quote payload: <strong> author then <p> preview
        // inside the blockquote (from the live SSRE/DevOps chat).
        let segs = parse_html(
            "<blockquote itemscope itemtype=\"http://schema.skype.com/Reply\" itemid=\"1\">\n<strong itemprop=\"mri\" itemid=\"8:orgid:x\">Cvitan Grguricin</strong><span itemprop=\"time\"></span><p itemprop=\"preview\">And we are still not done</p>\n</blockquote>\n<p>Does it mean that we are developing on staging?</p>",
        );
        let plain = segs_to_plain(&segs);
        assert!(
            plain.contains("Grguricin And"),
            "author and preview must not fuse: {plain:?}"
        );
    }
}

/// Real-payload fuzz harness (run-4). The archive holds thousands of real
/// Teams HTML payloads; this drives them — pristine and mutated (truncation
/// at every char boundary, unicode injection, tag swaps, pipe floods) —
/// through every pure parser on the message path. Panics report the file +
/// mutation so the exact input can be replayed.
///
/// Env-gated: without TEAMSFAST_FUZZ_DIR the test is a no-op (the corpus
/// is machine-local and must never be committed — fictional fixtures only).
/// Run: TEAMSFAST_FUZZ_DIR=/tmp/fuzz_corpus/html cargo test --release fuzz_corpus
#[cfg(test)]
mod fuzz_corpus {
    use super::{parse_html, segs_to_plain, split_markdown_links, split_pipe_tables};

    fn exercise(label: &str, input: &str) {
        let r = std::panic::catch_unwind(|| {
            let segs = parse_html(input);
            let plain = segs_to_plain(&segs);
            let _ = split_markdown_links(&plain);
            let _ = split_markdown_links(input);
            let _ = split_pipe_tables(&plain);
            let _ = crate::ui::cards::extract_card(input);
            let _ = crate::backend::live_parse::strip_html(&plain);
        });
        if r.is_err() {
            panic!("FUZZ HIT at {label}: {input:?}");
        }
    }

    fn char_indices_sampled(s: &str, full_below: usize, stride: usize) -> Vec<usize> {
        let mut v: Vec<usize> = s
            .char_indices()
            .map(|(i, _)| i)
            .filter(|&i| s.is_char_boundary(i))
            .collect();
        if s.chars().count() > full_below {
            v.retain(|&i| i % stride == 0);
        }
        v.push(s.len());
        v
    }

    #[test]
    fn real_payloads_and_mutations_never_panic() {
        let Ok(dir) = std::env::var("TEAMSFAST_FUZZ_DIR") else {
            return; // no local corpus — nothing to fuzz
        };
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("TEAMSFAST_FUZZ_DIR must be a readable dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "html"))
            .collect();
        files.sort();
        if files.is_empty() {
            return; // dir present but no payloads — nothing to fuzz
        }
        let mut n = 0;
        for f in &files {
            let Ok(raw) = std::fs::read_to_string(f) else {
                continue;
            };
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            // Pristine.
            exercise(&name, &raw);
            // Truncation at every char boundary (sampled for big payloads).
            for i in char_indices_sampled(&raw, 2048, 101) {
                exercise(&format!("{name}#trunc@{i}"), &raw[..i]);
            }
            // Unicode injection at sampled boundaries.
            for i in char_indices_sampled(&raw, 512, 211) {
                let mut m = String::with_capacity(raw.len() + 8);
                m.push_str(&raw[..i]);
                m.push_str("🎉éע❤️‍");
                m.push_str(&raw[i..]);
                exercise(&format!("{name}#inj@{i}"), &m);
            }
            // Tag chaos.
            let ops: [fn(&str) -> String; 5] = [
                |s: &str| s.replace('<', "<br/>"),
                |s: &str| s.replace("</", "/>"),
                |s: &str| s.to_uppercase(),
                |s: &str| s.replace("<p>", "<br>").replace("<br>", "<p>"),
                |s: &str| format!("{s}<)<<a</</"),
            ];
            for (k, op) in ops.iter().enumerate() {
                exercise(&format!("{name}#tagop{k}"), &op(&raw));
            }
            // Pipe flood through the table splitter.
            let plain = segs_to_plain(&parse_html(&raw));
            let flood = format!("{plain}{}|a|b|\n|c|d|", "|".repeat(64));
            exercise(&format!("{name}#pipes"), &flood);
            n += 1;
        }
        eprintln!("fuzz: exercised {n} payloads from {dir}");
    }
}


#[cfg(test)]
mod split_cells_tests {
    use super::split_cells;

    // Both dialects strip the outer pipes; Jira headers use the same
    // splitting with leading/trailing || removed.
    #[test]
    fn jira_vs_markdown_first_cell() {
        assert_eq!(
            split_cells("||A||B||", true),
            vec!["A".to_string(), "B".to_string()]
        );
        assert_eq!(
            split_cells("|a|b|", false),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn escapes_and_empties() {
        // escaped pipe becomes a LITERAL pipe inside one cell (the
        // backslash is consumed)
        assert_eq!(
            split_cells(r"|a\|b|c|", false),
            vec!["a|b".to_string(), "c".to_string()]
        );
        // interior empties preserved (column alignment)
        assert_eq!(
            split_cells("||x|", false),
            vec!["".to_string(), "x".to_string()]
        );
    }
    /// Pipe-table cell budget: natural 36-char look when the table fits
    /// (cheap estimate AND measured borderline band), shrunk with a char
    /// floor when a wide paste clearly overflows — an escaped table
    /// poisons every following row's layout math.
    #[test]
    fn pipe_cell_cap_fits_vs_wide() {
        use super::{pipe_cell_cap, PipeTable};
        crate::emoji::setup();
        let tbl = PipeTable {
            header: vec!["a".into(), "b".into()],
            rows: vec![vec!["short".into(), "value".into()]],
        };
        let wide = PipeTable {
            header: (0..5).map(|_| "h".repeat(40)).collect(),
            rows: vec![(0..5).map(|_| "c".repeat(60)).collect()],
        };
        let mut cap_out = [0usize; 2];
        let ctx = crate::theme::test_ctx();
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 200.0),
                )),
                ..Default::default()
            },
            |ui| {
                cap_out[0] = pipe_cell_cap(ui, &tbl, 2, 13.0);
                cap_out[1] = pipe_cell_cap(ui, &wide, 5, 13.0);
            },
        );
        let mut out = out;
        out.textures_delta.clear();
        // Tiny natural width vs a 600px budget: unchanged look (the
        // estimate alone returns without measuring).
        assert_eq!(cap_out[0], 36);
        // Five 60-char columns in a 600px region: clearly over — shrunk,
        // floored, sane (measured path).
        assert!(cap_out[1] >= 4 && cap_out[1] < 36, "cap={}", cap_out[1]);

        // Degenerate 1px region: never a 0-char budget (floor 4).
        let mut tiny = 0;
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1.0, 1.0),
                )),
                ..Default::default()
            },
            |ui| tiny = pipe_cell_cap(ui, &wide, 5, 13.0),
        );
        let mut out = out;
        out.textures_delta.clear();
        assert!(tiny >= 4 && tiny < 36, "tiny={tiny}");
    }
}

