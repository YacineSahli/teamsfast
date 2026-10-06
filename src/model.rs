//! Shared app-side helpers: time formatting, avatar colors.

use egui::Color32;

/// Message timestamp: `"14:32"` today, `"14:32, 3 Oct"` this year, else with year.
/// Server timestamps are UTC; rendered in the machine's local zone via jiff.
pub fn format_message_time(ts: &str) -> String {
    // Preferred path: jiff parses the instant and converts to the system
    // zone. (Zoned::from_str needs a [zone annotation]; Teams stamps have
    // only a trailing Z, so parse as Timestamp and convert.)
    if let Ok(zdt) = ts
        .trim()
        .parse::<jiff::Timestamp>()
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
    {
        let now = jiff::Zoned::now();
        let date = zdt.date();
        let today = now.date();
        let hm = format!("{:02}:{:02}", zdt.hour(), zdt.minute());
        return if date == today {
            hm
        } else if date.year() == today.year() {
            format!("{hm}, {} {}", date.day(), MONTHS[(date.month() - 1) as usize % 12])
        } else {
            format!("{hm}, {} {} {}", date.day(), MONTHS[(date.month() - 1) as usize % 12], date.year())
        };
    }
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
/// Local date, matching the local bubble timestamps.
pub fn format_day_label(ts: &str) -> String {
    let Ok(zdt) = ts
        .trim()
        .parse::<jiff::Timestamp>()
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
    else {
        return String::new();
    };
    let (y, m, d) = (zdt.year() as i64, zdt.month() as i64, zdt.day() as i64);
    let now = jiff::Zoned::now();
    let (ny, nm, nd) = (now.year() as i64, now.month() as i64, now.day() as i64);
    let today = y == ny && m == nm && d == nd;
    let yesterday = {
        let yz = now.clone() - jiff::Span::new().days(1);
        (y, m, d) == (yz.year() as i64, yz.month() as i64, yz.day() as i64)
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

/// Normalize a Teams timestamp (epoch-ms string or ISO-8601) to epoch
/// milliseconds — the key for read-horizon comparisons.
pub fn to_epoch_ms(raw: &str) -> Option<u64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(ms) = t.parse::<u64>() {
        return Some(ms);
    }
    t.parse::<jiff::Timestamp>()
        .ok()
        .map(|ts| ts.as_millisecond().max(0) as u64)
}

/// `"14:32"` for a chat-list timestamp (epoch-milliseconds string or ISO);
/// empty when unparsable.
pub fn format_chat_time(ms_opt: &Option<String>) -> String {    let raw = ms_opt.as_deref().unwrap_or("").trim();
    if raw.is_empty() {
        return String::new();
    }
    let zdt = if let Ok(ts) = raw.parse::<jiff::Timestamp>() {
        ts.to_zoned(jiff::tz::TimeZone::system())
    } else if let Ok(ms) = raw.parse::<u64>() {
        match jiff::Timestamp::from_second((ms / 1000) as i64) {
            Ok(ts) => ts.to_zoned(jiff::tz::TimeZone::system()),
            Err(_) => return String::new(),
        }
    } else {
        return String::new();
    };
    let hm = format!("{:02}:{:02}", zdt.hour(), zdt.minute());
    let today = jiff::Zoned::now().date();
    if zdt.date() == today {
        hm
    } else {
        format!("{} {}", zdt.day(), MONTHS[(zdt.month() - 1) as usize % 12])
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

/// Parse an ISO-8601 timestamp to Unix seconds. Tolerant fallback used when
/// jiff can't make sense of the string.
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

/// 1–2 letter initials for an avatar. Skips punctuation-only words so
/// "MI - ACME" yields "MD", not "M-".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphabetic()))
        .collect();
    let first_alpha = |w: &str| -> char {
        w.chars()
            .find(|c| c.is_alphabetic())
            .unwrap_or('?')
            .to_ascii_uppercase()
    };
    match words.as_slice() {
        [first, second, ..] => format!("{}{}", first_alpha(first), first_alpha(second)),
        [one] => one.chars().filter(|c| c.is_alphabetic()).take(2).collect(),
        _ => "?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set the test timezone before any jiff system-zone lookup.
    fn tz_brussels() {
        // SAFETY: single-threaded test binary, before first jiff use.
        unsafe { std::env::set_var("TZ", "Europe/Brussels") };
    }

    #[test]
    fn teams_shape_seven_digit_fraction_renders_local() {
        tz_brussels();
        // Teams wire shape: 7 fractional digits + Z (UTC). 10:12Z = 18:12 +8.
        let out = format_message_time("2026-10-06T10:12:37.6750000Z");
        // Today per wall clock in Taiwan is the same day as UTC+8 for this
        // stamp only if run on 2026-10-06/07 — assert the local hour shift
        // instead of the calendar bucket:
        assert!(
            out.contains("12:12"),
            "expected local 12:12 somewhere in {out:?}"
        );
    }

    #[test]
    fn chat_time_numeric_ms_is_local() {
        tz_brussels();
        // 2026-10-06T10:40:55Z in epoch ms = 10:40Z = 18:40 local.
        let ms = 1_760_000_000_000u64; // fixed: 2025-10-19T07:33:20Z — compute-free check below
        let _ = ms;
        // Instead build "now - 0" style: now ms as string → expect HH:MM now.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();
        let now_local = jiff::Zoned::now();
        let expected = format!("{:02}:{:02}", now_local.hour(), now_local.minute());
        assert_eq!(format_chat_time(&Some(now_ms)), expected);
    }

    #[test]
    fn chat_time_iso_with_offset() {
        tz_brussels();
        // Clock-independent: an ISO stamp from 30 minutes ago always falls
        // on the same local calendar day (except across a DST shift edge,
        // where the day bucket may differ — then only the digits matter).
        let now = jiff::Zoned::now();
        let past = now.clone() - jiff::Span::new().minutes(30);
        let iso = past.timestamp().to_string();
        let expected = format!("{:02}:{:02}", past.hour(), past.minute());
        let out = format_chat_time(&Some(iso.clone()));
        assert!(
            out.contains(&expected),
            "expected {expected:?} in {out:?} (iso={iso})"
        );
    }

    #[test]
    fn garbage_is_empty() {
        assert_eq!(format_message_time("nonsense"), "");
        assert_eq!(format_chat_time(&Some("nonsense".into())), "");
        assert_eq!(format_chat_time(&None), "");
    }
}
