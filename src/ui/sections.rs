//! Section panels: Calendar, Files, To Do, Activity. Pure views — data
//! arrives via App; actions flow back through the shared Action enum.

use crate::model::{format_day_label, format_message_time};
use crate::theme::Palette;
use crate::ui::conversation::Action;
use egui::{RichText, ScrollArea, Ui};
use ost::api::{MeetingInfo, SharedFile, TodoListInfo, TodoTaskInfo};
use std::collections::HashMap;

// ----------------------------------------------------------------- calendar

/// Upcoming meetings, soonest first. Day-grouped list; each row shows the
/// join link actions when the event has one.
/// Centered body note for loading/empty states. Drawn with the painter
/// (no layout): ScrollAreas and nested layouts collapse available rects,
/// which pinned every layout-based attempt to the top.
pub fn body_note(ui: &mut Ui, spinner: bool, pal: &Palette, text: &str) {
    let _ = spinner;
    let rect = ui.max_rect();
    let center = egui::pos2(
        rect.center().x,
        rect.top() + (rect.height() * 0.45).max(90.0),
    );
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        pal.secondary,
    );
}

/// Calendar view mode.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CalView {
    List,
    Week,
    Month,
}

const DAY_HEADERS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEK_HOURS: (i32, i32) = (7, 21); // 07:00 – 21:00 grid
/// Teams-default hour height: 36px crushed 30-min meetings into 17px
/// strips; 48px keeps them readable and fills the viewport.
const HOUR_PX: f32 = 48.0;

/// Wall-clock civil date + minute-of-day + duration of a meeting. Graph
/// calendar dateTimes are NAIVE (the meeting's own zone) — parse the wall
/// components directly so grid blocks sit where the printed times say, not
/// shifted by the local offset.
fn meeting_span(m: &MeetingInfo) -> Option<(jiff::civil::Date, i32, i32)> {
    let num = |t: &str, a: usize, n: usize| t.get(a..a + n)?.parse::<i64>().ok();
    let t = m.start.as_deref()?;
    let date = jiff::civil::date(
        i16::try_from(num(t, 0, 4)?).ok()?,
        i8::try_from(num(t, 5, 2)?).ok()?,
        i8::try_from(num(t, 8, 2)?).ok()?,
    );
    let start_min = (num(t, 11, 2)? * 60 + num(t, 14, 2)?) as i32;
    let dur = m
        .end
        .as_deref()
        .and_then(|e| {
            let edate = jiff::civil::date(
                i16::try_from(num(e, 0, 4)?).ok()?,
                i8::try_from(num(e, 5, 2)?).ok()?,
                i8::try_from(num(e, 8, 2)?).ok()?,
            );
            let emin = (num(e, 11, 2)? * 60 + num(e, 14, 2)?) as i32;
            let days = (edate - date).get_days();
            Some((emin + days * 1440 - start_min).clamp(15, 14 * 60))
        })
        .unwrap_or(30);
    Some((date, start_min, dur))
}

fn today_date() -> jiff::civil::Date {
    jiff::Zoned::now().date()
}

/// Monday of the current week shifted by `offset` weeks.
fn monday_of(offset: i32) -> jiff::civil::Date {
    let today = today_date();
    today
        .checked_add(jiff::ToSpan::days(
            -i64::from(today.weekday().to_monday_zero_offset()) + 7 * i64::from(offset),
        ))
        .unwrap_or(today)
}

pub fn calendar_panel(
    ui: &mut Ui,
    meetings: &[MeetingInfo],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
    view: &mut CalView,
    week_offset: &mut i32,
    month_offset: &mut i32,
    fetched: Option<(u64, u64)>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Calendar").strong().size(17.0));
        // Range label per view.
        let label = match view {
            CalView::List => "Next 7 days".to_string(),
            CalView::Week => {
                let monday = today_date()
                    .checked_add(jiff::ToSpan::days(
                        -i64::from(today_date().weekday().to_monday_zero_offset())
                            + 7 * i64::from(*week_offset),
                    ))
                    .unwrap_or_else(|_| today_date());
                let sunday = monday.checked_add(jiff::ToSpan::days(6)).unwrap_or(monday);
                format!(
                    "{} {} – {} {}",
                    monday.day(),
                    MONTH_ABBR(monday.month()),
                    sunday.day(),
                    MONTH_ABBR(sunday.month()),
                )
            }
            CalView::Month => {
                let first = today_date()
                    .first_of_month()
                    .checked_add(jiff::ToSpan::months(i64::from(*month_offset)))
                    .unwrap_or_else(|_| today_date().first_of_month());
                format!("{} {}", MONTH_ABBR(first.month()), first.year())
            }
        };
        ui.label(RichText::new(label).small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::widgets::btn::ghost(ui, pal, "Refresh").clicked() {
                actions.push(Action::ReloadSection);
            }
            if crate::ui::widgets::btn::ghost(ui, pal, "Join link…")
                .on_hover_text("Paste a Teams meeting link or ID")
                .clicked()
            {
                actions.push(Action::ShowJoinDialog);
            }
            if crate::ui::widgets::btn::secondary(ui, pal, "Meet now")
                .on_hover_text("Create an instant meeting and join it")
                .clicked()
            {
                actions.push(Action::MeetNow);
            }
            ui.separator();
            // View toggle + period navigation.
            let mut toggle = None;
            for (v, name) in [
                (CalView::List, "List"),
                (CalView::Week, "Week"),
                (CalView::Month, "Month"),
            ] {
                if ui
                    .selectable_label(*view == v, RichText::new(name).small())
                    .clicked()
                {
                    toggle = Some(v);
                }
            }
            if let Some(v) = toggle {
                *view = v;
            }
            if *view != CalView::List {
                if ui.small_button("›").clicked() {
                    match view {
                        CalView::Week => *week_offset += 1,
                        CalView::Month => *month_offset += 1,
                        CalView::List => {}
                    }
                }
                if ui.small_button("‹").clicked() {
                    match view {
                        CalView::Week => *week_offset -= 1,
                        CalView::Month => *month_offset -= 1,
                        CalView::List => {}
                    }
                }
                if crate::ui::widgets::btn::ghost(ui, pal, "Today").clicked() {
                    *week_offset = 0;
                    *month_offset = 0;
                }
            }
        });
    });
    // Header rule always draws (empty sections kept it hidden, so the
    // header rhythm jumped between tabs).
    ui.separator();

    match view {
        CalView::List => {
            // Upcoming only: the fetched window may reach further back once
            // the month grid navigated.
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let week_end = now_ms + 7 * 86_400_000;
            let upcoming: Vec<&MeetingInfo> = meetings
                .iter()
                .filter(|m| {
                    m.start.as_deref().and_then(crate::model::to_epoch_ms)
                        .map(|ms| ms + 86_400_000 >= now_ms && ms <= week_end)
                        .unwrap_or(false)
                })
                .collect();
            if loading && upcoming.is_empty() {
                body_note(ui, true, pal, "Loading calendar…");
                return;
            }
            if upcoming.is_empty() {
                body_note(ui, false, pal, "No meetings in the coming week.");
                return;
            }
            ScrollArea::vertical()
                .id_salt("calendar_scroll")
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut last_day = String::new();
                    for m in upcoming {
                        let day = m
                            .start
                            .as_deref()
                            .map(format_day_label)
                            .unwrap_or_default();
                        if !day.is_empty() && day != last_day {
                            last_day = day.clone();
                            ui.add_space(6.0);
                            crate::ui::widgets::day_separator(ui, pal, &day);
                            ui.add_space(6.0);
                        }
                        meeting_row(ui, m, pal, actions);
                    }
                });
        }
        CalView::Week => {
            week_grid(ui, meetings, *week_offset, pal, actions);
            ensure_fetched(ui, actions, fetched, week_window(*week_offset), loading);
        }
        CalView::Month => {
            month_grid(ui, meetings, *month_offset, pal, actions);
            ensure_fetched(ui, actions, fetched, month_window(*month_offset), loading);
        }
    }
}

fn MONTH_ABBR(month: i8) -> &'static str {
    const M: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    M[(i64::from(month).clamp(1, 12) - 1) as usize]
}

/// (monday_of_week, label) for a week offset from today.
fn week_window(offset: i32) -> (u64, u64) {
    let today = today_date();
    let monday = today
        .checked_add(jiff::ToSpan::days(
            -i64::from(today.weekday().to_monday_zero_offset()) + 7 * i64::from(offset),
        ))
        .unwrap_or(today);
    let to_unix = |d: jiff::civil::Date| {
        d.at(0, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::system())
            .ok()
            .map(|z| z.timestamp().as_second() as u64)
            .unwrap_or(0)
    };
    (to_unix(monday), to_unix(monday) + 7 * 86_400)
}

/// (first_of_month, label) for a month offset from today.
fn month_window(offset: i32) -> (u64, u64) {
    let first = today_date()
        .first_of_month()
        .checked_add(jiff::ToSpan::months(i64::from(offset)))
        .unwrap_or_else(|_| today_date().first_of_month());
    let next = first
        .checked_add(jiff::ToSpan::months(1))
        .unwrap_or(first);
    let to_unix = |d: jiff::civil::Date| {
        d.at(0, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::system())
            .ok()
            .map(|z| z.timestamp().as_second() as u64)
            .unwrap_or(0)
    };
    (to_unix(first), to_unix(next))
}

/// Fire a range fetch when the visible window isn't covered. Edge-triggered
/// by `fetched` being updated in Action::apply.
///
/// `loading` gates re-fires while a fetch is already in flight: without it,
/// every frame until the reply lands queues another identical command (a
/// Graph round-trip is ~1s ≈ 60 frames of duplicates).
fn ensure_fetched(
    ui: &mut Ui,
    actions: &mut Vec<Action>,
    fetched: Option<(u64, u64)>,
    window: (u64, u64),
    loading: bool,
) {
    let covered = fetched
        .map(|(a, b)| window.0 >= a && window.1 <= b)
        .unwrap_or(false);
    // Ceil the span: a month crossing the EU DST switch is 31.04 days, and
    // a floored 31-day fetch falls an hour short — the coverage check then
    // fails forever and the same window re-fetches every round-trip.
    let days = (window.1.saturating_sub(window.0) + 86_399) / 86_400;
    if !covered && !loading {
        actions.push(Action::LoadCalendarRange {
            start_unix: window.0,
            days,
        });
    }
}

/// Cut `s` to fit `max_px` at `size` px, appending "…" — painter text has
/// no built-in truncation, and clipping alone shears glyphs mid-stroke.
fn ellipsize(ui: &Ui, s: &str, size: f32, max_px: f32) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    let fits = |t: &str| -> bool {
        ui.painter()
            .layout_no_wrap(t.to_string(), egui::FontId::proportional(size), egui::Color32::WHITE)
            .size()
            .x
            <= max_px
    };
    if fits(s) {
        return s.to_string();
    }
    // Char-boundary-safe shrinking cut (floor to the previous boundary).
    let mut cut = s.len();
    while cut > 0 {
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 {
            break;
        }
        if fits(&format!("{}…", &s[..cut])) {
            return format!("{}…", &s[..cut]);
        }
        cut = cut.saturating_sub(std::cmp::max(1, cut / 4));
    }
    "…".to_string()
}

/// Pure wall-clock "HH:MM" for a naive-or-zoned Graph stamp (no date —
/// the calendar column/cell already carries it).
fn hm_only(ts: &str) -> String {
    if let Ok(zdt) = ts
        .trim()
        .parse::<jiff::Timestamp>()
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
    {
        format!("{:02}:{:02}", zdt.hour(), zdt.minute())
    } else {
        // Naive wall-clock fallback (Graph dateTimes are naive).
        ts.get(11..16).unwrap_or_default().to_string()
    }
}

/// One meeting pill; returns true when clicked (join). Painter text is
/// truncated with an ellipsis — clipping alone shears glyphs mid-stroke.
/// Click routing shared by every meeting surface (list rows, week blocks,
/// month pills): join when there's a link, otherwise say WHY nothing joined
/// — a silent click read as a dead UI.
fn meeting_click(m: &MeetingInfo, actions: &mut Vec<Action>) {
    if let Some(url) = m.join_url.clone() {
        actions.push(Action::JoinMeeting {
            source: url,
            label: Some(if m.subject.is_empty() {
                "Meeting".into()
            } else {
                m.subject.clone()
            }),
        });
    } else if m.canceled {
        actions.push(Action::Status(
            "Cancelled meeting — nothing to join.".into(),
        ));
    } else {
        actions.push(Action::Status(
            "In-person meeting — no Teams join link.".into(),
        ));
    }
}

fn meeting_pill(
    ui: &mut Ui,
    rect: egui::Rect,
    m: &MeetingInfo,
    pal: &Palette,
    compact: bool,
    dim: bool,
) -> bool {
    let online = m.join_url.is_some();
    let dead = m.canceled;
    let fill = if dead {
        pal.surface
    } else if online {
        egui::Color32::from_rgba_unmultiplied(
            pal.accent.r(),
            pal.accent.g(),
            pal.accent.b(),
            0x38,
        )
    } else {
        pal.surface
    };
    let p = ui.painter().with_clip_rect(rect.shrink(1.0));
    p.rect_filled(rect, 6, fill);
    p.rect_stroke(
        rect,
        6,
        egui::Stroke::new(1.0, if dead { pal.outline.gamma_multiply(0.6) } else { pal.outline }),
        egui::StrokeKind::Inside,
    );
    let time = m.start.as_deref().map(hm_only).unwrap_or_default();
    // Teams blocks show the TIME RANGE — the date is the column/cell.
    let range = m
        .end
        .as_deref()
        .map(hm_only)
        .map(|e| format!("{time} – {e}"))
        .unwrap_or_else(|| time.clone());
    let title_col = if dead || dim {
        pal.secondary
    } else {
        pal.text
    };
    // No "Canceled:" prefix — the dim + strikethrough carry the state
    // (native Teams shows the plain subject).
    let subject = m.subject.clone();
    let text_w = rect.width() - 10.0;
    if compact || rect.height() < 30.0 {
        let subj_e = ellipsize(ui, &subject, 10.0, (text_w - 32.0).max(8.0));
        // Lane too narrow for "time title" — a title fragment still says
        // more than a bare time; never render "14:00 …" (time + nothing).
        let line = if subj_e.chars().count() <= 2 {
            ellipsize(ui, &subject, 10.0, text_w.max(8.0))
        } else {
            format!("{time} {subj_e}")
        };
        let r = p.text(
            rect.left_center() + egui::vec2(5.0, 0.0),
            egui::Align2::LEFT_CENTER,
            line,
            egui::FontId::proportional(10.0),
            title_col,
        );
        if dead {
            p.line_segment([r.left_center(), r.right_center()], egui::Stroke::new(1.0, pal.secondary));
        }
    } else {
        let r = p.text(
            rect.left_top() + egui::vec2(5.0, 3.0),
            egui::Align2::LEFT_TOP,
            ellipsize(ui, &subject, 10.5, text_w),
            egui::FontId::proportional(10.5),
            title_col,
        );
        if dead {
            p.line_segment([r.left_center(), r.right_center()], egui::Stroke::new(1.0, pal.secondary));
        }
        p.text(
            rect.left_bottom() + egui::vec2(5.0, -3.0),
            egui::Align2::LEFT_BOTTOM,
            ellipsize(ui, &range, 9.5, text_w),
            egui::FontId::proportional(9.5),
            pal.secondary,
        );
    }
    let r = ui.allocate_rect(rect, egui::Sense::click());
    r.on_hover_text(if m.subject.is_empty() {
        "Meeting".to_string()
    } else {
        format!(
            "{} — {}{}",
            m.subject,
            range,
            if online && !dead { " (click to join)" } else { "" }
        )
    })
    .clicked()
}

fn week_grid(
    ui: &mut Ui,
    meetings: &[MeetingInfo],
    offset: i32,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    let avail_w = ui.available_width() - 8.0;
    let gutter = 42.0;
    // Fit the 7 columns to the available width (a "just barely scrolls"
    // grid clipped Sunday at common widths — worst of both worlds). The
    // 96px floor is where horizontal scroll takes over on tiny panes;
    // narrow overlap lanes degrade gracefully via compact pills instead
    // of forcing the whole grid wider.
    let col_w = ((avail_w - gutter) / 7.0).max(96.0);
    let (h0, h1) = WEEK_HOURS;
    let rows = (h1 - h0) as f32;
    let header_h = 22.0;
    let body_h = rows * HOUR_PX;
    let total_w = gutter + col_w * 7.0;

    let today = today_date();
    let monday = monday_of(offset);

    ScrollArea::both()
        .id_salt("calendar_week")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.set_min_width(total_w);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(total_w, header_h + body_h), egui::Sense::hover());
            let top = rect.top();

            // Header row + column separators. Today = accent date disc
            // (same treatment as the month grid, so "today" reads the same
            // in both views); weekday name stays quiet above it.
            for (i, name) in DAY_HEADERS.iter().enumerate() {
                let day = monday.checked_add(jiff::ToSpan::days(i as i64)).unwrap_or(monday);
                let x = rect.left() + gutter + col_w * i as f32;
                let is_today = day == today;
                let cx = x + col_w / 2.0;
                ui.painter().text(
                    egui::pos2(cx, top + 8.0),
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::proportional(10.0),
                    pal.secondary,
                );
                if is_today {
                    let drect = egui::Rect::from_center_size(
                        egui::pos2(cx, top + 22.0),
                        egui::vec2(19.0, 19.0),
                    );
                    ui.painter()
                        .circle_filled(drect.center(), 9.5, pal.accent);
                    ui.painter().text(
                        drect.center(),
                        egui::Align2::CENTER_CENTER,
                        day.day().to_string(),
                        egui::FontId::proportional(10.5),
                        pal.on_accent,
                    );
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(x + 1.0, top + header_h),
                            egui::vec2(col_w - 2.0, body_h),
                        ),
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(
                            pal.accent.r(),
                            pal.accent.g(),
                            pal.accent.b(),
                            16,
                        ),
                    );
                } else {
                    ui.painter().text(
                        egui::pos2(cx, top + 22.0),
                        egui::Align2::CENTER_CENTER,
                        day.day().to_string(),
                        egui::FontId::proportional(11.5),
                        pal.text,
                    );
                }
                ui.painter().line_segment(
                    [egui::pos2(x, top), egui::pos2(x, top + header_h + body_h)],
                    egui::Stroke::new(1.0, pal.outline),
                );
            }

            // Hour gutter lines + labels. Hour lines are the QUIETER rule
            // (day columns carry the stronger one); labels sit centered ON
            // their line with a background knockout so the line reads as
            // interrupted, not as an underline.
            for h in h0..=h1 {
                let y = top + header_h + (h - h0) as f32 * HOUR_PX;
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left() + gutter, y),
                        egui::pos2(rect.left() + gutter + col_w * 7.0, y),
                    ],
                    egui::Stroke::new(1.0, pal.outline.gamma_multiply(0.4)),
                );
                if h < h1 {
                    let label = format!("{h:02}:00");
                    let gal = ui.painter().layout_no_wrap(
                        label.clone(),
                        egui::FontId::proportional(9.5),
                        pal.dim,
                    );
                    let lx = rect.left() + 6.0;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(lx - 2.0, y - gal.size().y / 2.0 - 1.0),
                            gal.size() + egui::vec2(4.0, 2.0),
                        ),
                        2.0,
                        pal.panel,
                    );
                    ui.painter().galley(egui::pos2(lx, y - gal.size().y / 2.0), gal, pal.dim);
                }
            }

            // Now line.
            let now = jiff::Zoned::now();
            if today >= monday
                && today
                    < monday
                        .checked_add(jiff::ToSpan::days(7))
                        .unwrap_or(monday)
            {
                let day_idx = (today - monday).get_days() as f32;
                let mins = i32::from(now.hour()) * 60 + i32::from(now.minute());
                if mins >= h0 * 60 && mins <= h1 * 60 {
                    let y = top + header_h + (mins - h0 * 60) as f32 / 60.0 * HOUR_PX;
                    let x0 = rect.left() + gutter + col_w * day_idx;
                    // The now-line spans the WHOLE grid (shared time
                    // reference, not a per-day decoration) with a leading
                    // dot at the current column.
                    let x_end = rect.left() + gutter + col_w * 7.0;
                    ui.painter().line_segment(
                        [egui::pos2(x0, y), egui::pos2(x_end, y)],
                        egui::Stroke::new(1.5, pal.danger),
                    );
                    ui.painter().circle_filled(egui::pos2(x0, y), 3.0, pal.danger);
                }
            }

            // Meeting blocks: concurrent events lane side-by-side within
            // their day column instead of overlapping.
            let mut by_day: std::collections::HashMap<i32, Vec<&MeetingInfo>> =
                std::collections::HashMap::new();
            for m in meetings {
                if let Some((date, _, _)) = meeting_span(m) {
                    let idx = (date - monday).get_days();
                    if (0..7).contains(&idx) {
                        by_day.entry(idx).or_default().push(m);
                    }
                }
            }
            for (idx, list) in by_day {
                // Sort by start; greedy lanes: a block takes the first lane
                // whose last block ends at or before its start.
                let mut spans: Vec<(&&MeetingInfo, i32, i32)> = list
                    .iter()
                    .filter_map(|m| meeting_span(m).map(|(_, s, d)| (m, s, d)))
                    .collect();
                spans.sort_by_key(|(_, s, _)| *s);
                let mut lanes: Vec<i32> = Vec::new(); // lane -> end minute
                let mut lane_of: Vec<usize> = Vec::new();
                for (_, s, d) in &spans {
                    let mut lane = lanes.iter().position(|end| *end <= *s);
                    match lane {
                        Some(i) => lanes[i] = s + d,
                        None => {
                            lanes.push(s + d);
                            lane = Some(lanes.len() - 1);
                        }
                    }
                    lane_of.push(lane.unwrap());
                }
                let total_lanes = lanes.len().max(1);
                for (i, (m, s, d)) in spans.iter().enumerate() {
                    let lane = lane_of[i];
                    let lane_w = col_w / total_lanes as f32;
                    let x = rect.left() + gutter + col_w * idx as f32 + lane_w * lane as f32 + 1.0;
                    // Blocks starting before the visible window clamp their
                    // TOP; the height must run to the REAL end, not the full
                    // duration (a 06:00-10:10 meeting drew until 11:10).
                    let vis_start = (*s).max(h0 * 60);
                    let y =
                        top + header_h + (vis_start - h0 * 60) as f32 / 60.0 * HOUR_PX;
                    let h = ((s + d - vis_start) as f32 / 60.0 * HOUR_PX)
                        .clamp(16.0, (top + header_h + body_h - y).max(16.0));
                    let block = egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(lane_w - 2.0, h),
                    );
                    // Narrow overlap lanes drop to the compact single-line
                    // pill (time + ellipsized title) instead of two crushed
                    // rows of clipped text.
                    if meeting_pill(ui, block, m, pal, lane_w < 52.0, false) {
                        meeting_click(m, actions);
                    }
                }
            }
        });
}

fn month_grid(
    ui: &mut Ui,
    meetings: &[MeetingInfo],
    offset: i32,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    let first = today_date()
        .first_of_month()
        .checked_add(jiff::ToSpan::months(i64::from(offset)))
        .unwrap_or_else(|_| today_date().first_of_month());
    let days = first.days_in_month();
    let lead = i64::from(first.weekday().to_monday_zero_offset());
    let today = today_date();
    let avail_w = ui.available_width() - 8.0;
    let cell_w = (avail_w / 7.0).max(96.0);
    // Stretch rows to the viewport (Teams behavior): ~90px rows fit four
    // pills + "+N"; the old fixed 64px left a third of the window empty.
    let rows = (((lead + days as i64 + 6) / 7) as f32).max(1.0);
    let cell_h = ((ui.available_height() - 26.0) / rows).clamp(64.0, 130.0);

    // Meeting index by civil date.
    let mut by_date: std::collections::HashMap<jiff::civil::Date, Vec<&MeetingInfo>> =
        std::collections::HashMap::new();
    for m in meetings {
        if let Some((date, _, _)) = meeting_span(m) {
            by_date.entry(date).or_default().push(m);
        }
    }

    ScrollArea::vertical()
        .id_salt("calendar_month")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.set_min_width(cell_w * 7.0);
            // Weekday header (painted at computed column centers — a
            // per-cell painter centered on a zero-content cell stacked all
            // seven labels at the far left).
            {
                let (row_rect, _) = ui
                    .allocate_exact_size(egui::vec2(cell_w * 7.0, 18.0), egui::Sense::hover());
                for (i, name) in DAY_HEADERS.iter().enumerate() {
                    ui.painter().text(
                        egui::pos2(
                            row_rect.left() + cell_w * i as f32 + cell_w / 2.0,
                            row_rect.center().y,
                        ),
                        egui::Align2::CENTER_CENTER,
                        name,
                        egui::FontId::proportional(11.0),
                        pal.secondary,
                    );
                }
            }
            let cells = ((lead + days as i64 + 6) / 7) * 7;
            for row in 0..(cells / 7) {
                ui.horizontal(|ui| {
                    for col in 0..7 {
                        let idx = row * 7 + col;
                        let day = first
                            .checked_add(jiff::ToSpan::days(idx - lead))
                            .unwrap_or(first);
                        let in_month = day.month() == first.month();
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(cell_w, cell_h),
                            egui::Sense::hover(),
                        );
                        if day == today {
                            // Faint cell wash only — the marker is the
                            // accent DISC behind the numeral (a full-cell
                            // accent outline read as a selection).
                            ui.painter().rect_filled(
                                rect,
                                0,
                                egui::Color32::from_rgba_unmultiplied(
                                    pal.accent.r(),
                                    pal.accent.g(),
                                    pal.accent.b(),
                                    0x14,
                                ),
                            );
                        }
                        ui.painter().rect_stroke(
                            rect,
                            0,
                            egui::Stroke::new(1.0, pal.outline.gamma_multiply(0.55)),
                            egui::StrokeKind::Inside,
                        );
                        let day_alpha = if in_month { 255 } else { 90 };
                        let num_pos = rect.left_top() + egui::vec2(13.0, 12.0);
                        if day == today {
                            ui.painter().circle_filled(num_pos, 9.0, pal.accent);
                        }
                        ui.painter().text(
                            num_pos,
                            egui::Align2::CENTER_CENTER,
                            day.day().to_string(),
                            egui::FontId::proportional(11.0),
                            if day == today {
                                pal.on_accent
                            } else {
                                egui::Color32::from_rgba_unmultiplied(
                                    pal.text.r(),
                                    pal.text.g(),
                                    pal.text.b(),
                                    day_alpha,
                                )
                            },
                        );
                        if let Some(list) = by_date.get(&day) {
                            // Out-of-month pills dim like their numerals.
                            let dim = !in_month;
                            for (i, m) in list.iter().take(
                                ((cell_h as usize - 24) / 18).clamp(1, 4),
                            )
                            .enumerate()
                            {
                                let pill = egui::Rect::from_min_size(
                                    rect.left_top()
                                        + egui::vec2(4.0, 20.0 + i as f32 * 18.0),
                                    egui::vec2(cell_w - 8.0, 16.0),
                                );
                                if meeting_pill(ui, pill, m, pal, true, dim) {
                                    meeting_click(m, actions);
                                }
                            }
                            if list.len() > 2 {
                                ui.painter().text(
                                    rect.left_bottom() + egui::vec2(6.0, -3.0),
                                    egui::Align2::LEFT_BOTTOM,
                                    format!("+{}", list.len() - 2),
                                    egui::FontId::proportional(9.5),
                                    pal.secondary,
                                );
                            }
                        }
                    }
                });
            }
        });
}

fn meeting_row(ui: &mut Ui, m: &MeetingInfo, pal: &Palette, actions: &mut Vec<Action>) {
    let time = m
        .start
        .as_deref()
        .map(|s| hm_only(s))
        .unwrap_or_default();
    // Duration on the card: "13:30 – 14:00" (start-only left durations
    // unknowable).
    let time = m
        .end
        .as_deref()
        .map(hm_only)
        .map(|e| format!("{time} – {e}"))
        .unwrap_or(time);
    // Canceled events keep their slot (native Teams behavior) but dim,
    // strike the title, and lose the Join action.
    let dead = m.canceled;
    egui::Frame::default()
        .fill(if dead {
            crate::theme::tint_over(pal.surface, pal.panel, 0.5)
        } else {
            pal.surface
        })
        .stroke(egui::Stroke::new(1.0, pal.outline))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let time_txt = RichText::new(time).small().strong();
                ui.label(if dead {
                    time_txt.weak()
                } else {
                    time_txt.color(pal.secondary)
                });
                let title = RichText::new(if m.subject.is_empty() {
                    "(no title)"
                } else {
                    &m.subject
                })
                .strong();
                ui.label(if dead { title.weak().strikethrough() } else { title });
                if dead {
                    // Bordered chip — the plain small label read like body
                    // text and cancelled rows still scanned as joinable.
                    egui::Frame::default()
                        .stroke(egui::Stroke::new(1.0, pal.outline))
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(5, 1))
                        .show(ui, |ui| {
                            ui.set_min_height(15.0);
                            ui.label(
                                RichText::new("Cancelled").weak().small().color(pal.secondary),
                            );
                        });
                }
            });
            ui.horizontal(|ui| {
                // Reserve the action-button height so cards without a join
                // URL stay the same height as carded ones.
                ui.set_min_height(20.0);
                if let Some(who) = &m.organizer {
                    ui.label(RichText::new(format!("by {who}")).small().weak());
                }
                if m.is_organizer {
                    ui.label(RichText::new("organizer").small().weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(url) = &m.join_url {
                        if !dead && ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Join").small().color(pal.on_accent),
                                )
                                .fill(pal.accent)
                                .min_size(egui::vec2(54.0, 20.0)),
                            )
                            .on_hover_text("Join this meeting in TeamsFast")
                            .clicked()
                        {                            actions.push(Action::JoinMeeting {
                                source: url.clone(),
                                label: Some(if m.subject.is_empty() {
                                    "Meeting".to_string()
                                } else {
                                    m.subject.clone()
                                }),
                            });
                        }
                        if crate::ui::widgets::btn::ghost(ui, pal, "Copy link")
                            .clicked()
                        {
                            actions.push(Action::CopyText(url.clone()));
                        }
                    }
                });
            });
        });
    ui.add_space(4.0);
}

// ------------------------------------------------------------------- files

pub fn files_panel(
    ui: &mut Ui,
    files: &[SharedFile],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Files").strong().size(17.0));
        ui.label(RichText::new("Recent in OneDrive").small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::widgets::btn::ghost(ui, pal, "Refresh").clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
    ui.separator();
    if loading && files.is_empty() {
        body_note(ui, true, pal, "Loading files…");
        return;
    }
    if files.is_empty() {
        body_note(ui, false, pal, "No recent files.");
        return;
    }

    ScrollArea::vertical().id_salt("files_scroll").auto_shrink(false).show(ui, |ui| {

        for f in files {
            file_row(ui, f, pal, actions);
        }
    });
}

/// Type tint for the file glyph — palette-sourced (adapts per theme):
/// color-coding by type is a native-Teams scanning cue without needing
/// per-type icon assets.
fn file_type_tint(pal: &Palette, name: &str) -> egui::Color32 {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => pal.danger,
        "doc" | "docx" | "rtf" | "odt" | "txt" | "md" => pal.link,
        "xls" | "xlsx" | "csv" | "ods" => pal.ok,
        "ppt" | "pptx" | "odp" => pal.accent,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => pal.accent_hover,
        "zip" | "tar" | "gz" | "7z" | "rar" => pal.secondary,
        _ => pal.secondary,
    }
}

fn file_row(ui: &mut Ui, f: &SharedFile, pal: &Palette, actions: &mut Vec<Action>) {
    let size = if f.size >= 1_048_576 {
        format!("{:.1} MB", f.size as f64 / 1_048_576.0)
    } else if f.size >= 1024 {
        format!("{:.0} KB", f.size as f64 / 1024.0)
    } else if f.size > 0 {
        format!("{} B", f.size)
    } else {
        "—".to_string()
    };
    let modified = f
        .modified
        .as_deref()
        .map(format_message_time)
        .unwrap_or_default();
    let is_folder = f.is_folder;
    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width() - 4.0, 40.0),
        egui::Sense::click(),
    );
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 6, pal.surface_hover);
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(8.0, 2.0))));
    {
        let ui = &mut child;
        ui.horizontal(|ui| {
            let (icon, tint) = if is_folder {
                (crate::theme::Icon::Archive, pal.secondary)
            } else {
                (crate::theme::Icon::FileText, file_type_tint(pal, &f.name))
            };
            ui.add(
                egui::Image::from_bytes(icon.uri(), icon.bytes())
                    .tint(tint)
                    .fit_to_exact_size(egui::Vec2::splat(16.0)),
            );
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 150.0);
                // A lone "—" placeholder line on rows without metadata read
                // as broken — draw the name alone, nudged toward the row's
                // optical center, and keep the row height (rhythm intact).
                ui.add(
                    egui::Label::new(RichText::new(&f.name).strong())
                        .truncate()
                        .selectable(false),
                );
                // Known metadata, else a muted origin line — every row
                // keeps the same two-line anatomy (an empty second line
                // read as alternating row heights; a "—" read as broken).
                let meta = match (size == "—", modified.is_empty()) {
                    (true, false) => modified.clone(),
                    (false, true) => size.clone(),
                    (false, false) => format!("{size} · {modified}"),
                    (true, true) => "In OneDrive".to_string(),
                };
                ui.label(RichText::new(meta).small().weak());
            });
            // Row actions reveal on hover (Fluent): resting rows carry
            // icon + text only — two permanent outlined buttons on every
            // row was the loudest chrome in the app. Ghost icons: the
            // default quiet stroke still drew two boxed buttons per row.
            if hovered {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ghost_icon = |ui: &mut egui::Ui,
                                      icon: crate::theme::Icon,
                                      tip: &str,
                                      hit: &mut bool| {
                        if crate::ui::widgets::icon_button(ui, icon, tip).clicked() {
                            *hit = true;
                        }
                    };
                    if !is_folder {
                        let mut do_dl = false;
                        let mut do_open = false;
                        if let (Some(drive), Some(item)) = (&f.drive_id, Some(&f.id)) {
                            ghost_icon(
                                ui,
                                crate::theme::Icon::Download,
                                "Download and open",
                                &mut do_dl,
                            );
                            if do_dl {
                                actions.push(Action::DownloadDriveFile {
                                    drive_id: drive.clone(),
                                    item_id: item.clone(),
                                    name: f.name.clone(),
                                });
                            }
                        }
                        if let Some(web) = &f.web_url {
                            ghost_icon(
                                ui,
                                crate::theme::Icon::ExternalLink,
                                "Open online",
                                &mut do_open,
                            );
                            if do_open {
                                actions.push(Action::OpenLink(web.clone()));
                            }
                        }
                    }
                });
            }
        });
    }
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        // Click a folder/file: open online when possible.
        if let Some(web) = &f.web_url {
            actions.push(Action::OpenLink(web.clone()));
        }
    }
    // Separators yield to the hover surface (a rule drawn through the
    // highlighted row slices it).
    if !hovered {
        ui.separator();
    }
}

/// Files shared within one conversation (shown above the drive recents).
pub fn chat_files_panel(
    ui: &mut Ui,
    _chat_id: &str,
    chat_name: &str,
    files: &[SharedFile],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        let chat_label = chat_name.trim_matches(|c| c == '[' || c == ']');
        let chat_label = if chat_label == "Direct message" {
            "Direct message"
        } else {
            chat_name
        };
        ui.heading(
            RichText::new(format!("Files · {chat_label}"))
                .strong()
                .size(15.0),
        );
        if loading {
            ui.spinner();
        }
    });
    ui.separator();
    ScrollArea::vertical()
        .id_salt(("chat_files", _chat_id))
        .max_height(260.0)
        .show(ui, |ui| {
            if files.is_empty() && !loading {
                ui.label(RichText::new("No files shared in this conversation.").weak());
            }
            for f in files {
                file_row(ui, f, pal, actions);
            }
        });
}

// ------------------------------------------------------------------- to-do

pub struct TodoPanelState {
    pub selected_list: Option<String>,
    pub quick_add: String,
    pub show_completed: bool,
}

impl Default for TodoPanelState {
    fn default() -> Self {
        Self {
            selected_list: None,
            quick_add: String::new(),
            show_completed: false,
        }
    }
}

/// Two-pane: lists on the left, tasks of the selected list on the right.
pub fn todo_panel(
    ui: &mut Ui,
    lists: &[TodoListInfo],
    tasks: &HashMap<String, Vec<TodoTaskInfo>>,
    st: &mut TodoPanelState,
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("To Do").strong().size(17.0));
        ui.label(RichText::new("Lists & tasks").small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Matched ghost buttons — a small_button chip next to a
            // standard ghost sat visibly misaligned on the header line.
            if crate::ui::widgets::btn::ghost(
                ui,
                pal,
                if st.show_completed {
                    "Hide completed"
                } else {
                    "Show completed"
                },
            )
            .clicked()
            {
                st.show_completed = !st.show_completed;
            }
            if crate::ui::widgets::btn::ghost(ui, pal, "Refresh").clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
    ui.separator();
    if lists.is_empty() && !loading {
        body_note(ui, false, pal, "No To Do lists found.");
        return;
    }
    let body_h = ui.available_height();
    ui.horizontal(|ui| {
        // Lists column (same width token as the OneNote nav column).
        ui.allocate_ui(egui::vec2(240.0, body_h), |ui| {
            ScrollArea::vertical().id_salt("todo_lists_scroll").auto_shrink(false).show(ui, |ui| {
                // Parent is a horizontal row — force the column layout.
                ui.vertical(|ui| {
                for l in lists {
                    let sel = st.selected_list.as_deref() == Some(l.id.as_str());
                    if ui
                        .selectable_label(sel, RichText::new(&l.name).strong())
                        .clicked()
                    {
                        st.selected_list = Some(l.id.clone());
                        actions.push(Action::OpenTodoList(l.id.clone()));
                    }
                }
                })
            });
        });
        // Full-height divider between the columns (the stock vertical
        // Separator only spans the row's content height).
        {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(1.0, body_h),
                egui::Sense::hover(),
            );
            ui.painter().vline(
                rect.center().x,
                rect.top()..=rect.bottom(),
                egui::Stroke::new(1.0, pal.outline),
            );
        }
        // Tasks column.
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let Some(list) = st.selected_list.clone().or_else(|| lists.first().map(|l| l.id.clone())) else {
                ui.label(RichText::new("Pick a list.").weak());
                return;
            };
            if st.selected_list.is_none() {
                st.selected_list = Some(list.clone());
                actions.push(Action::OpenTodoList(list.clone()));
            }
            let empty: Vec<TodoTaskInfo> = Vec::new();
            let items = tasks.get(&list).unwrap_or(&empty);
            // Quick add.
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut st.quick_add)
                        .hint_text("Add a task…")
                        .desired_width(ui.available_width() - 64.0),
                );
                // Add rests disabled until there's text (a clickable no-op
                // invites a dead click).
                let armed = !st.quick_add.trim().is_empty();
                let add = ui.add_enabled(armed, egui::Button::new("Add")).clicked()
                    || (armed
                        && field.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if add {
                    let title = st.quick_add.trim().to_string();
                    if !title.is_empty() {
                        actions.push(Action::AddTodoTask {
                            list_id: list.clone(),
                            title,
                        });
                        st.quick_add.clear();
                    }
                }
            });
            ui.add_space(4.0);
            ScrollArea::vertical().id_salt("todo_tasks_scroll").auto_shrink(false).show(ui, |ui| {
                let mut shown = 0;
                for t in items {
                    let done = t.status == "completed";
                    if done && !st.show_completed {
                        continue;
                    }
                    shown += 1;
                    ui.horizontal(|ui| {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(18.0, 18.0),
                            egui::Sense::click(),
                        );
                        let stroke_col = if done { pal.ok } else { pal.outline };
                        ui.painter().rect_stroke(
                            rect,
                            4,
                            egui::Stroke::new(1.4, stroke_col),
                            egui::StrokeKind::Inside,
                        );
                        if done {
                            ui.painter().rect_filled(
                                rect.shrink(4.0),
                                3,
                                pal.ok,
                            );
                        }
                        if resp.clicked() {
                            actions.push(Action::SetTodoDone {
                                list_id: list.clone(),
                                task_id: t.id.clone(),
                                done: !done,
                            });
                        }
                        ui.add(
                            egui::Label::new(RichText::new(&t.title).color(if done {
                                pal.dim
                            } else {
                                pal.text
                            }))
                            .truncate()
                            .selectable(false),
                        )
                        .on_hover_text(format!(
                            "status: {}  importance: {}",
                            t.status, t.importance
                        ));
                        if let Some(due) = &t.due {
                            ui.label(
                                RichText::new(format_message_time(due))
                                    .small()
                                    .weak(),
                            );
                        }
                    });
                }
                if shown == 0 {
                    body_note(ui, false, pal, "Nothing here — add a task above.");
                }
            });
        });
    });
}

// ----------------------------------------------------------------- activity

/// One activity entry (mentions / reactions / replies) mined from live
/// events or notifications. The type lives in `model` so the backend can
/// seed the feed from cached history; re-exported here for the panel.
pub use crate::model::ActivityEntry;

pub fn activity_panel(
    ui: &mut Ui,
    entries: &[ActivityEntry],
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Activity").strong().size(17.0));
        ui.label(RichText::new("Mentions, reactions, replies").small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !entries.is_empty(),
                    egui::Button::new(RichText::new("Clear").small()),
                )
                .on_hover_text("Clear the activity feed")
                .clicked()
            {
                actions.push(Action::ClearActivity);
            }
        });
    });
    ui.separator();
    if entries.is_empty() {
        body_note(ui, false, pal, "Nothing yet — mentions and reactions land here.");
        return;
    }
    ScrollArea::vertical().id_salt("activity_scroll").auto_shrink(false).show(ui, |ui| {
        for e in entries {
            let (icon, tint) = match e.kind {
                "mention" => (crate::theme::Icon::AtSign, pal.warning),
                "reaction" => (crate::theme::Icon::Smile, pal.ok),
                "reply" => (crate::theme::Icon::Reply, pal.link),
                _ => (crate::theme::Icon::MessageSquare, pal.secondary),
            };
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width() - 4.0, 44.0),
                egui::Sense::click(),
            );
            if response.hovered() {
                ui.painter().rect_filled(rect, 6, pal.surface_hover);
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(8.0, 3.0))),
            );
            {
                let ui = &mut child;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::from_bytes(icon.uri(), icon.bytes())
                            .tint(tint)
                            .fit_to_exact_size(egui::Vec2::splat(15.0)),
                    );
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 10.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&e.chat_name)
                                    .strong()
                                    .color(pal.text),
                            );
                            ui.label(
                                RichText::new(format!("· {}", e.kind))
                                    .small()
                                    .weak(),
                            );
                        });
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}: {}", e.who, e.preview))
                                    .small()
                                    .weak(),
                            )
                            .truncate()
                            .selectable(false),
                        );
                    });
                });
            }
            if response.clicked() {
                actions.push(Action::OpenChat(e.chat_id.clone()));
            }
            ui.separator();
        }
    });
}

// ----------------------------------------------------------------- planner

/// One planner board: a plan's buckets and tasks.
#[derive(Clone, Debug)]
pub struct PlannerBoard {
    pub team: String,
    pub plan: String,
    pub buckets: Vec<(String, String)>, // (id, name)
    pub tasks: Vec<ost::api::PlannerTaskInfo>,
}

/// Planner boards grouped by bucket columns.
pub fn planner_panel(
    ui: &mut Ui,
    boards: &[PlannerBoard],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Planner").strong().size(17.0));
        ui.label(RichText::new("Boards from your teams").small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::widgets::btn::ghost(ui, pal, "Refresh").clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
    ui.separator();
    if loading && boards.is_empty() {
        body_note(ui, true, pal, "Loading planner…");
        return;
    }
    if boards.is_empty() {
        body_note(ui, false, pal, "No planner tasks found in your teams.");
        return;
    }

    ScrollArea::vertical()
        .id_salt("planner_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            for board in boards {
                ui.horizontal(|ui| {
                    ui.strong(&board.plan);
                    ui.label(
                        RichText::new(format!("· {}", board.team))
                            .small()
                            .weak(),
                    );
                });
                ui.add_space(2.0);
                let bucket_name = |id: &str| -> String {
                    board
                        .buckets
                        .iter()
                        .find(|(bid, _)| bid == id)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| "No bucket".into())
                };
                egui::Grid::new(("planner-grid", &board.plan, &board.team))
                    .num_columns(2)
                    .spacing([10.0, 2.0])
                    .show(ui, |ui| {
                        for t in &board.tasks {
                            ui.horizontal(|ui| {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(16.0, 16.0),
                                    egui::Sense::click(),
                                );
                                let col = if t.completed { pal.ok } else { pal.outline };
                                ui.painter().rect_stroke(
                                    rect,
                                    4,
                                    egui::Stroke::new(1.3, col),
                                    egui::StrokeKind::Inside,
                                );
                                if t.completed {
                                    ui.painter().rect_filled(rect.shrink(4.0), 3, pal.ok);
                                }
                                if resp.clicked() {
                                    actions.push(Action::SetPlannerDone {
                                        task_id: t.id.clone(),
                                        etag: t.etag.clone(),
                                        done: !t.completed,
                                    });
                                }
                            });
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&t.title).color(if t.completed {
                                            pal.dim
                                        } else {
                                            pal.text
                                        }),
                                    );
                                    if t.percent_complete > 0 && !t.completed {
                                        ui.label(
                                            RichText::new(format!(
                                                "{}%",
                                                t.percent_complete
                                            ))
                                            .small()
                                            .weak(),
                                        );
                                    }
                                    if let Some(due) = &t.due {
                                        ui.label(
                                            RichText::new(format_message_time(due))
                                                .small()
                                                .weak(),
                                        );
                                    }
                                });
                                ui.label(
                                    RichText::new(bucket_name(&t.bucket_id))
                                        .small()
                                        .weak(),
                                );
                            });
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.separator();
            }
        });
}

// ----------------------------------------------------------------- shifts

/// This week's shifts, grouped per person.
pub fn shifts_panel(
    ui: &mut Ui,
    shifts: &[ost::api::ShiftInfo],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Shifts").strong().size(17.0));
        ui.label(RichText::new("This week").small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::widgets::btn::ghost(ui, pal, "Refresh").clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
    ui.separator();
    if loading && shifts.is_empty() {
        body_note(ui, true, pal, "Loading shifts…");
        return;
    }
    if shifts.is_empty() {
        body_note(ui, false, pal, "No shifts this week (no team schedule or none shared).");
        return;
    }

    ScrollArea::vertical()
        .id_salt("shifts_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            for sh in shifts {
                egui::Frame::default()
                    .fill(pal.surface)
                    .stroke(egui::Stroke::new(1.0, pal.outline))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&sh.display_name).strong());
                            if sh.is_draft {
                                ui.label(
                                    RichText::new("draft").small().weak(),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let start = sh
                                        .start
                                        .as_deref()
                                        .map(format_message_time)
                                        .unwrap_or_default();
                                    let end = sh
                                        .end
                                        .as_deref()
                                        .map(format_message_time)
                                        .unwrap_or_default();
                                    if !start.is_empty() {
                                        ui.label(
                                            RichText::new(format!("{start} – {end}"))
                                                .small()
                                                .color(pal.link),
                                        );
                                    }
                                },
                            );
                        });
                        if let Some(notes) = &sh.notes {
                            if !notes.trim().is_empty() {
                                ui.label(RichText::new(notes).small().weak());
                            }
                        }
                    });
                ui.add_space(4.0);
            }
        });
}

// ----------------------------------------------------------------- onenote

/// Notebook → sections → pages tree (app-side; core returns flat pieces).
#[derive(Clone, Debug)]
pub struct NotebookTree {
    pub name: String,
    pub sections: Vec<ost::api::SectionInfo>,
}

pub struct NotesPanelState {
    pub open_page: Option<(String, String)>, // (page_id, title)
    pub append_text: String,
}

impl Default for NotesPanelState {
    fn default() -> Self {
        Self {
            open_page: None,
            append_text: String::new(),
        }
    }
}

/// OneNote browser: notebooks → sections → pages; a selected page renders
/// its text with an append box.
pub fn notes_panel(
    ui: &mut Ui,
    notebooks: &[NotebookTree],
    page: Option<&ost::api::NotePage>,
    st: &mut NotesPanelState,
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("OneNote").strong().size(17.0));
        ui.label(RichText::new("notebooks & pages").small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::widgets::btn::ghost(ui, pal, "Refresh").clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
    ui.separator();
    let body_h = ui.available_height();
    ui.horizontal(|ui| {
        // Nav column.
        ui.allocate_ui(egui::vec2(240.0, body_h), |ui| {
            ScrollArea::vertical()
                .id_salt("notes_nav")
                .auto_shrink(false)
                .show(ui, |ui| {
                    // A ScrollArea inherits the parent layout — this one
                    // lives in a horizontal row, so force a column.
                    ui.vertical(|ui| {
                    if notebooks.is_empty() && !loading {
                        ui.label(RichText::new("No notebooks found.").weak());
                    }
                    for nb in notebooks {
                        ui.strong(&nb.name);
                        for sec in &nb.sections {
                            // Real indent, not string spaces — the two-space
                            // version read as same-level sibling rows.
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.label(RichText::new(&sec.name).small().weak());
                            });
                            for p in &sec.pages {
                                let sel = st
                                    .open_page
                                    .as_ref()
                                    .map(|(id, _)| id == &p.id)
                                    .unwrap_or(false);
                                if ui
                                    .selectable_label(sel, RichText::new(p.title.clone()).small())
                                    .clicked()
                                {
                                    st.open_page = Some((p.id.clone(), p.title.clone()));
                                    actions.push(Action::ReadNotePage(p.id.clone()));
                                }
                            }
                        }
                        ui.add_space(4.0);
                    }
                    });
                });
        });
        // Divider.
        {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(1.0, body_h),
                egui::Sense::hover(),
            );
            ui.painter().vline(
                rect.center().x,
                rect.top()..=rect.bottom(),
                egui::Stroke::new(1.0, pal.outline),
            );
        }
        // Page column.
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            match page {
                Some(p) => {
                    ui.strong(&p.title);
                    ui.separator();
                    ScrollArea::vertical()
                        .id_salt("notes_page")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            // Render the page HTML as plain segments.
                            let segs =
                                crate::ui::widgets::parse_html(&p.html);
                            crate::ui::widgets::render_segments(
                                ui,
                                &segs,
                                pal,
                                |_, _| false,
                                |_, _, _| {},
                            );
                        });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut st.append_text)
                                .hint_text("Append to this page…")
                                .desired_width(ui.available_width() - 66.0),
                        );
                        let send = ui.button("Append").clicked()
                            || (field.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if send && !st.append_text.trim().is_empty() {
                            if let Some((id, _)) = st.open_page.clone() {
                                actions.push(Action::AppendNote {
                                    page_id: id,
                                    text: st.append_text.trim().to_string(),
                                });
                                st.append_text.clear();
                            }
                        }
                    });
                }
                None => {
                    body_note(ui, false, pal, "Pick a page on the left.");
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting(date: jiff::civil::Date, join_url: Option<String>) -> MeetingInfo {
        let online = join_url.is_some();
        MeetingInfo {
            id: "evt".into(),
            subject: "Sync".into(),
            start: Some(format!("{date}T08:00:00.0000000")),
            end: Some(format!("{date}T09:00:00.0000000")),
            join_url,
            organizer: None,
            organizer_email: None,
            is_organizer: false,
            is_online: online,
            canceled: false,
        }
    }

    type Actions = std::rc::Rc<std::cell::RefCell<Vec<Action>>>;

    fn run(actions: &Actions, events: Vec<egui::Event>, mut body: impl FnMut(&mut Ui)) {
        let egui_ctx = egui::Context::default();
        let out = egui_ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            body,
        );
        let mut out = out;
        out.textures_delta.clear();
    }

    /// Click routing: an online meeting joins, an in-person one says why
    /// not, a cancelled one doesn't offer a join at all.
    #[test]
    fn meeting_click_routes_by_joinability() {
        let mut a = Vec::new();
        meeting_click(&meeting(today_date(), Some("https://join".into())), &mut a);
        assert!(matches!(a[0], Action::JoinMeeting { .. }), "online → join, got {a:?}");

        a.clear();
        meeting_click(&meeting(today_date(), None), &mut a);
        assert!(matches!(a[0], Action::Status(_)), "in-person → status note, got {a:?}");

        a.clear();
        let mut dead = meeting(today_date(), None);
        dead.canceled = true;
        meeting_click(&dead, &mut a);
        assert!(matches!(a[0], Action::Status(_)), "cancelled → status note, got {a:?}");
    }

    /// A month window spanning the EU DST switch (Oct 2026: +02:00 →
    /// +01:00) is 31.04 days. The pushed fetch rounds its day count UP, so
    /// the stored range covers the window — a floored 31 days fell an hour
    /// short and the same window re-fetched forever.
    #[test]
    fn ensure_fetched_accepts_dst_short_month() {
        // Oct 1 00:00 +02:00 → Nov 1 00:00 +01:00 = 31.04 days.
        let start: u64 = 1_790_805_600;
        let end: u64 = 1_793_487_600;
        // The ceil'd fetch Action::apply stores after the push (see
        // ensure_fetched_pushes_ceil_days).
        let days = (end - start + 86_399) / 86_400; // 32
        let fetched = Some((start, start + days * 86_400));

        let actions: Actions = Default::default();
        let sink = actions.clone();
        run(&actions, vec![], move |ui| {
            ensure_fetched(ui, &mut sink.borrow_mut(), fetched, (start, end), false);
        });
        assert!(
            actions.borrow().is_empty(),
            "covered DST month must not re-fetch, got {:?}",
            actions.borrow()
        );
    }

    /// An uncovered window pushes a fetch whose day count rounds UP, so the
    /// stored range (start + days·86400) covers the whole window.
    #[test]
    fn ensure_fetched_pushes_ceil_days() {
        let start: u64 = 1_790_805_600;
        let end: u64 = 1_793_487_600; // 31.04 days later
        let actions: Actions = Default::default();
        let sink = actions.clone();
        run(&actions, vec![], move |ui| {
            ensure_fetched(ui, &mut sink.borrow_mut(), None, (start, end), false);
        });
        let a = actions.borrow();
        assert_eq!(a.len(), 1, "uncovered window must fetch once");
        match &a[0] {
            Action::LoadCalendarRange { start_unix, days } => {
                assert_eq!(*start_unix, start);
                assert!(
                    start_unix + days * 86_400 >= end,
                    "ceil days must cover the window end ({days} days)"
                );
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    /// While a fetch is in flight, ensure_fetched must stay silent — every
    /// frame until the reply used to queue another identical command.
    #[test]
    fn ensure_fetched_quiet_while_loading() {
        let actions: Actions = Default::default();
        let sink = actions.clone();
        run(&actions, vec![], move |ui| {
            ensure_fetched(ui, &mut sink.borrow_mut(), None, (0, 86_400), true);
        });
        assert!(
            actions.borrow().is_empty(),
            "loading gate must suppress re-fires"
        );
    }

    /// Clicking a month-grid pill registers (the cell below is hover-only;
    /// the pill's later-allocated click sense must win) and joins when the
    /// meeting is online.
    ///
    /// egui's hit test reads the PREVIOUS frame's widget rects — both
    /// frames must share one Context, or the click lands in a cold registry
    /// (a fresh Context per frame silently eats every click).
    #[test]
    fn clicking_a_month_pill_joins() {
        // Same math as month_grid: first rendered cell = first_of_month -
        // lead days; pill 0 sits at cell.left_top() + (4, 20) below the
        // 18px weekday header.
        let first = today_date().first_of_month();
        let lead = i64::from(first.weekday().to_monday_zero_offset());
        let first_cell = first
            .checked_add(jiff::ToSpan::days(-lead))
            .unwrap_or(first);
        let m = meeting(first_cell, Some("https://teams.microsoft.com/join".into()));

        let cell_w = ((900.0_f32 - 8.0) / 7.0).max(96.0);
        let center = egui::pos2(4.0 + (cell_w - 8.0) / 2.0, 18.0 + 20.0 + 8.0);

        let egui_ctx = egui::Context::default();
        let screen = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };

        // Frame 1: warm layout — egui hit-tests against these rects.
        let warm: Actions = Default::default();
        let sink = warm.clone();
        let mm = m.clone();
        let mut out = egui_ctx.run_ui(screen.clone(), |ui| {
            month_grid(ui, std::slice::from_ref(&mm), 0, &Palette::dark(), &mut sink.borrow_mut());
        });
        out.textures_delta.clear();
        assert!(warm.borrow().is_empty(), "layout frame must not act");

        // Frame 2: press + release on the pill.
        let click_input = egui::RawInput {
            screen_rect: screen.screen_rect,
            events: vec![
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let clicked: Actions = Default::default();
        let sink = clicked.clone();
        let mm = m.clone();
        let mut out = egui_ctx.run_ui(click_input, |ui| {
            month_grid(ui, std::slice::from_ref(&mm), 0, &Palette::dark(), &mut sink.borrow_mut());
        });
        out.textures_delta.clear();
        let a = clicked.borrow();
        assert_eq!(
            a.len(),
            1,
            "pill click must route to meeting_click, got {a:?}"
        );
        match &a[0] {
            Action::JoinMeeting { source, .. } => {
                assert_eq!(source, "https://teams.microsoft.com/join")
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }
}
