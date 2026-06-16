//! Canonical time helpers.
//!
//! Storage and wire format are **always UTC** (epoch integers in SQLite,
//! RFC 3339 `…Z` on the API). The only place a non-UTC zone enters the backend
//! is *calendar-day grouping* for the uptime breakdown, which uses the
//! configurable workspace timezone. The system never assumes KST or the
//! server-local timezone.

use std::str::FromStr;
use std::sync::OnceLock;

use chrono::TimeZone;
use chrono_tz::Tz;

/// The configured workspace timezone, used **only** for calendar semantics
/// (daily/weekly grouping, reports, "today/yesterday" labels). Raw storage and
/// wire timestamps stay UTC.
///
/// Read once from `WORKSPACE_TIMEZONE` — an IANA name such as `Asia/Seoul`,
/// `America/Los_Angeles`, or `UTC` — defaulting to `UTC`. An invalid name falls
/// back to UTC with a warning. Cached for the process lifetime, matching the
/// rest of the env-driven config (a change needs a restart).
pub fn workspace_timezone() -> Tz {
    static TZ: OnceLock<Tz> = OnceLock::new();
    *TZ.get_or_init(|| match std::env::var("WORKSPACE_TIMEZONE") {
        Ok(raw) if !raw.trim().is_empty() => {
            let name = raw.trim();
            match Tz::from_str(name) {
                Ok(tz) => {
                    tracing::info!(timezone = %name, "Workspace timezone configured");
                    tz
                }
                Err(_) => {
                    tracing::warn!(
                        value = %name,
                        "Invalid WORKSPACE_TIMEZONE (not an IANA name) — falling back to UTC"
                    );
                    Tz::UTC
                }
            }
        }
        _ => Tz::UTC,
    })
}

/// Start-of-calendar-day **in `tz`** for the instant `epoch_secs`, returned as a
/// UTC epoch second.
///
/// DST-correct: the absolute instant is mapped into `tz`, truncated to the
/// local date, and that local midnight is mapped back to a UTC instant. This is
/// the grouping key for daily aggregation, so day boundaries land on the
/// workspace timezone's calendar rather than UTC's. With `tz == UTC` it reduces
/// to flooring to the 86 400 s boundary, preserving the previous behavior.
pub fn day_start_utc(tz: Tz, epoch_secs: i64) -> i64 {
    // Converting an absolute instant into a timezone is always unambiguous.
    let local = match tz.timestamp_opt(epoch_secs, 0).single() {
        Some(dt) => dt,
        None => return epoch_secs - epoch_secs.rem_euclid(86_400),
    };
    let midnight = local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("00:00:00 is always a valid time");
    // Local → absolute can be a gap (spring-forward) or ambiguous (fall-back)
    // for zones whose transition lands at midnight; pick the earliest valid
    // instant, and on the rare gap-at-midnight fall back to a UTC-day floor.
    match tz.from_local_datetime(&midnight).earliest() {
        Some(dt) => dt.timestamp(),
        None => epoch_secs - epoch_secs.rem_euclid(86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_day_start_floors_to_86400() {
        let ts = 1_700_000_123_i64;
        assert_eq!(day_start_utc(Tz::UTC, ts), ts - ts.rem_euclid(86_400));
        // Exactly midnight UTC maps to itself.
        let midnight = Tz::UTC
            .with_ymd_and_hms(2026, 6, 16, 0, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(day_start_utc(Tz::UTC, midnight), midnight);
    }

    #[test]
    fn seoul_day_start_uses_kst_calendar() {
        let seoul = Tz::Asia__Seoul; // UTC+9, no DST
        let expected = seoul
            .with_ymd_and_hms(2026, 6, 16, 0, 0, 0)
            .unwrap()
            .timestamp();
        // 09:30 KST and 23:00 KST on the same calendar day share a day start.
        let morning = seoul
            .with_ymd_and_hms(2026, 6, 16, 9, 30, 0)
            .unwrap()
            .timestamp();
        let evening = seoul
            .with_ymd_and_hms(2026, 6, 16, 23, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(day_start_utc(seoul, morning), expected);
        assert_eq!(day_start_utc(seoul, evening), expected);
        // The KST day start is 15:00:00Z the previous UTC day — i.e. NOT the
        // UTC-day floor, proving the grouping is timezone-aware.
        assert_ne!(
            day_start_utc(seoul, morning),
            day_start_utc(Tz::UTC, morning)
        );
    }

    #[test]
    fn los_angeles_day_start_handles_dst_spring_forward() {
        let la = Tz::America__Los_Angeles;
        // 2026-03-08: US spring-forward (02:00 PST → 03:00 PDT).
        let expected = la
            .with_ymd_and_hms(2026, 3, 8, 0, 0, 0)
            .unwrap()
            .timestamp();
        // An instant before the transition (01:00, still PST) and one after
        // (23:00, now PDT) belong to the same LA calendar day.
        let before = la
            .with_ymd_and_hms(2026, 3, 8, 1, 0, 0)
            .unwrap()
            .timestamp();
        let after = la
            .with_ymd_and_hms(2026, 3, 8, 23, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(day_start_utc(la, before), expected);
        assert_eq!(day_start_utc(la, after), expected);
    }

    #[test]
    fn los_angeles_day_start_handles_dst_fall_back() {
        let la = Tz::America__Los_Angeles;
        // 2026-11-01: US fall-back (02:00 PDT → 01:00 PST), the 01:00 hour is
        // ambiguous. Day start must still be a single PDT-midnight instant.
        let expected = la
            .with_ymd_and_hms(2026, 11, 1, 0, 0, 0)
            .unwrap()
            .timestamp();
        let noon = la
            .with_ymd_and_hms(2026, 11, 1, 12, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(day_start_utc(la, noon), expected);
    }
}
