//! Shared app-side helpers: time formatting, avatar colors.

use egui::Color32;

/// Message timestamp: `"14:32"` today, `"14:32 · 3 Oct"` this year, else with year.
/// Accepts RFC3339-ish ISO strings (Teams server timestamps).
pub fn format_message_time(ts: &str) -> String {
    let Some(secs) = parse_iso_secs(ts) else {
        return String::new();
    };
    let now = now_secs();
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let (ny, nm, nd) = civil_from_days(now.div_euclid(86_400));
    let (hh, mm, _) = hms_of(secs);
    if y == ny && m == nm && d == nd {
        format!("{hh:02}:{mm:02}")
    } else if y == ny {
        format!("{:02}:{:02}, {} {}", hh, mm, d, MONTHS[(m - 1) as usize % 12])
    } else {
        format!(
            "{:02}:{:02}, {} {} {}",
            hh,
            mm,
            d,
            MONTHS[(m - 1) as usize % 12],
            y
        )
    }
}

/// Day-separator label: `"Today"`, `"Yesterday"`, `"3 Oct"`, `"3 Oct 2025"`.
pub fn format_day_label(ts: &str) -> String {
    let Some(secs) = parse_iso_secs(ts) else {
        return String::new();
    };
    let now = now_secs();
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let (ny, nm, nd) = civil_from_days(now.div_euclid(86_400));
    let today = y == ny && m == nm && d == nd;
    let yesterday = {
        let (yy, ym, yd) = civil_from_days(now.div_euclid(86_400) - 1);
        y == yy && m == ym && d == yd
    };
    if today {
        "Today".into()
    } else if yesterday {
        "Yesterday".into()
    } else if y == ny {
        format!("{} {}", d, MONTHS[(m - 1) as usize % 12])
    } else {
        format!("{} {} {}", d, MONTHS[(m - 1) as usize % 12], y)
    }
}

/// `"14:32"` for a chat-list timestamp (epoch-milliseconds string or ISO);
/// empty when unparsable.
pub fn format_chat_time(ms_opt: &Option<String>) -> String {
    let raw = ms_opt.as_deref().unwrap_or("").trim();
    if raw.is_empty() {
        return String::new();
    }
    let secs = match raw.parse::<u64>() {
        Ok(ms) => (ms / 1000) as i64,
        Err(_) => match parse_iso_secs(raw) {
            Some(secs) => secs,
            None => return String::new(),
        },
    };
    let now = now_secs();
    let (ly, lm, ld) = civil_from_days(secs.div_euclid(86_400));
    let (ny, nm, nd) = civil_from_days(now.div_euclid(86_400));
    if ly == ny && lm == nm && ld == nd {
        let (hh, mm, _) = hms_of(secs);
        format!("{hh:02}:{mm:02}")
    } else if ly == ny {
        format!("{} {}", ld, MONTHS[(lm - 1) as usize % 12])
    } else {
        format!("{} {} {}", ld, MONTHS[(lm - 1) as usize % 12], ly)
    }
}

/// Remove connector markdown (`++underline++`, `**bold**`, `` `code` ``)
/// that leaks into previews.
pub fn clean_preview(s: &str) -> String {
    s.replace("++", "")
        .replace("**", "")
        .replace("~~", "")
        .replace('`', "")
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hms_of(secs: i64) -> (i64, i64, i64) {
    let sod = secs.rem_euclid(86_400);
    (sod / 3600, (sod % 3600) / 60, sod % 60)
}

/// Parse an ISO-8601 timestamp (`2026-10-06T08:56:22.402Z`, with offset or not)
/// to Unix seconds. Manual but tolerant — no chrono on the UI crate.
pub fn parse_iso_secs(ts: &str) -> Option<i64> {
    let t = ts.trim();
    let b = t.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |a: usize, n: usize| -> Option<i64> { t.get(a..a + n)?.parse().ok() };
    let y = num(0, 4)?;
    let month = num(5, 2)?;
    let day = num(8, 2)?;
    let hh = num(11, 2)?;
    let mi = num(14, 2)?;
    let ss = num(17, 2)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let days = days_from_civil(y, month, day);
    let mut secs = days * 86_400 + hh * 3600 + mi * 60 + ss;
    // Offset suffix: 'Z', or ±HH:MM.
    if let Some(pos) = t[19..].find(['+', '-']) {
        let off = &t[19 + pos..];
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let oh: i64 = off.get(1..3)?.parse().ok()?;
        let om: i64 = off.get(4..6).unwrap_or("00").parse().unwrap_or(0);
        secs -= sign * (oh * 3600 + om * 60);
    }
    Some(secs)
}

/// Howard Hinnant's `days_from_civil`.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant's `civil_from_days`: days since epoch to (y, m, d).
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Deterministic pleasant avatar color for a name.
pub fn avatar_color(name: &str) -> Color32 {
    const PALETTE: [Color32; 10] = [
        Color32::from_rgb(0x0f, 0x6b, 0xd5),
        Color32::from_rgb(0x8b, 0x2f, 0xc7),
        Color32::from_rgb(0xc2, 0x37, 0x66),
        Color32::from_rgb(0xd1, 0x70, 0x1f),
        Color32::from_rgb(0x0e, 0x83, 0x4e),
        Color32::from_rgb(0x00, 0x7e, 0x8a),
        Color32::from_rgb(0x5b, 0x5f, 0xd6),
        Color32::from_rgb(0xb4, 0x59, 0x2c),
        Color32::from_rgb(0x56, 0x69, 0x0a),
        Color32::from_rgb(0x94, 0x10, 0x70),
    ];
    let mut h: u64 = 5381;
    for b in name.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    PALETTE[(h % PALETTE.len() as u64) as usize]
}

/// 1–2 letter initials for an avatar.
pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    match parts.as_slice() {
        [first, last, ..] => {
            format!("{}{}", first.chars().next().unwrap_or('?'), last.chars().next().unwrap_or('?'))
                .to_uppercase()
        }
        [one] => one.chars().take(2).collect::<String>().to_uppercase(),
        _ => "?".into(),
    }
}
