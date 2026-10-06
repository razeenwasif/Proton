//! Days as a person studies them: where today begins, here, and when the day
//! of an exam does.
//!
//! A day of study begins at four in the morning, as Anki's does: a card
//! reviewed at one in the morning was reviewed the day before, by the
//! reckoning of someone who has not yet slept.

const DAY: i64 = 86_400;
/// When a day of study begins, after midnight.
const ROLLOVER: i64 = 4 * 3600;

/// Seconds east of UTC, here, at a moment.
pub fn offset(at: i64) -> i64 {
    #[cfg(unix)]
    {
        let t = at as libc::time_t;
        // SAFETY: `localtime_r` writes only the `tm` it is given.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
    #[cfg(not(unix))]
    {
        let _ = at;
        0
    }
}

/// The day of study a moment falls in, here: `[start, end)`.
pub fn today(now: i64) -> (i64, i64) {
    day_of(now, offset(now))
}

/// The day of study a moment falls in, `offset` seconds east of UTC.
pub fn day_of(at: i64, offset: i64) -> (i64, i64) {
    let start = (at + offset - ROLLOVER).div_euclid(DAY) * DAY + ROLLOVER - offset;
    (start, start + DAY)
}

/// The moment the day of an exam begins, here: midnight of `YYYY-MM-DD`. A
/// card is never due after it, so the evening before is the last review.
pub fn exam_day(date: &str) -> Option<i64> {
    let midnight = midnight_utc(date)?;
    Some(midnight - offset(midnight))
}

/// Midnight UTC of `YYYY-MM-DD`.
pub fn midnight_utc(date: &str) -> Option<i64> {
    let mut parts = date.trim().splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_its_midnight() {
        assert_eq!(midnight_utc("1970-01-01"), Some(0));
        assert_eq!(midnight_utc("2022-11-29"), Some(1_669_680_000));
        assert_eq!(midnight_utc("2000-03-01"), Some(951_868_800), "after a leap day");
        assert_eq!(midnight_utc("2026-13-01"), None);
        assert_eq!(midnight_utc("soon"), None);
    }

    #[test]
    fn a_day_of_study_begins_at_four() {
        // Canberra in summer, eleven hours east.
        let east = 11 * 3600;
        let one_am = midnight_utc("2026-11-12").unwrap() - east + 3600;
        let (start, end) = day_of(one_am, east);
        assert_eq!(start, midnight_utc("2026-11-11").unwrap() - east + ROLLOVER, "one in the morning is still the day before");
        assert_eq!(end - start, DAY);
        let five_am = one_am + 4 * 3600;
        assert_eq!(day_of(five_am, east).0, end, "five is the next day");
    }
}
