//! Relative timestamps for file cards ("5 minutes ago").

use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock milliseconds since the Unix epoch.
pub fn wall_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn plural(n: u64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// "just now", "5 minutes ago", "yesterday", "3 weeks ago" ... for a past timestamp.
pub fn relative(now_ms: u64, then_ms: u64) -> String {
    let secs = now_ms.saturating_sub(then_ms) / 1000;
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    match secs {
        0..=44 => "just now".into(),
        45..=89 => "1 minute ago".into(),
        _ if mins < 60 => plural(mins.max(2), "minute"),
        _ if hours < 24 => plural(hours, "hour"),
        _ if days == 1 => "yesterday".into(),
        _ if days < 7 => plural(days, "day"),
        _ if days < 30 => plural(days / 7, "week"),
        _ if days < 365 => plural(days / 30, "month"),
        _ => plural(days / 365, "year"),
    }
}

/// "Edited 5 minutes ago" / "Edited just now", in the UI language.
pub fn edited(now_ms: u64, then_ms: u64) -> String {
    crate::i18n::trf("Edited {}", &[&crate::i18n::tr_text(&relative(now_ms, then_ms))])
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;

    #[test]
    fn buckets() {
        let now = 1_000 * DAY;
        assert_eq!(relative(now, now), "just now");
        assert_eq!(relative(now, now + 5 * MIN), "just now", "future clamps");
        assert_eq!(relative(now, now - 50_000), "1 minute ago");
        assert_eq!(relative(now, now - 5 * MIN), "5 minutes ago");
        assert_eq!(relative(now, now - 2 * MIN), "2 minutes ago");
        assert_eq!(relative(now, now - HOUR), "1 hour ago");
        assert_eq!(relative(now, now - 7 * HOUR), "7 hours ago");
        assert_eq!(relative(now, now - DAY), "yesterday");
        assert_eq!(relative(now, now - 3 * DAY), "3 days ago");
        assert_eq!(relative(now, now - 14 * DAY), "2 weeks ago");
        assert_eq!(relative(now, now - 90 * DAY), "3 months ago");
        assert_eq!(relative(now, now - 800 * DAY), "2 years ago");
        assert_eq!(edited(now, now - 5 * MIN), "Edited 5 minutes ago");
    }
}
