//! Dates as Excel counts them: serial numbers from 1 January 1900, which is
//! 1, with the 29 February 1900 that Lotus 1-2-3 had and Excel kept (serial
//! 60), so a date reads the same in Excel. A day's time is the fraction.

/// Days since 1970-01-01 for a date in the proleptic Gregorian calendar.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day counted from 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// DATE(y, m, d): months and days past their end carried over, as Excel
/// does — DATE(2024, 13, 1) is 1 January 2025. `None` before 1900 or past
/// 9999.
pub fn serial(y: i64, m: i64, d: i64) -> Option<f64> {
    // Excel reads years 0–1899 as 1900 onwards.
    let y = if (0..1900).contains(&y) { y + 1900 } else { y };
    let y = y + (m - 1).div_euclid(12);
    let m = (m - 1).rem_euclid(12) + 1;
    // The month's first day as a serial, then the days counted on from it
    // in Excel's own count, so 29 February 1900 is 60 however it is reached.
    let first = days_from_civil(y, m, 1);
    let march = days_from_civil(1900, 3, 1);
    let first = if first >= march { first - days_from_civil(1899, 12, 30) } else { first - days_from_civil(1899, 12, 31) };
    let n = first + d - 1;
    (1..=2_958_465).contains(&n).then_some(n as f64)
}

/// The year, month and day of a serial; its 60 is 29 February 1900.
pub fn ymd(serial: f64) -> Option<(i64, u32, u32)> {
    if !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let n = serial.floor() as i64;
    Some(match n {
        60 => (1900, 2, 29),
        n if n < 60 => civil_from_days(days_from_civil(1899, 12, 31) + n),
        n => civil_from_days(days_from_civil(1899, 12, 30) + n),
    })
}

/// The hour, minute and second of a serial's fraction, rounded to the second.
pub fn hms(serial: f64) -> (u32, u32, u32) {
    let secs = ((serial - serial.floor()) * 86_400.0).round() as u32 % 86_400;
    (secs / 3600, secs / 60 % 60, secs % 60)
}

/// The day of the week, Sunday 0, as WEEKDAY's type 1 counts from it.
pub fn weekday(serial: f64) -> u32 {
    // Serial 1, 1 January 1900, was a Sunday by Excel's count.
    ((serial.floor() as i64 - 1).rem_euclid(7)) as u32
}

/// Days in a month.
pub fn month_days(y: i64, m: u32) -> u32 {
    let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m as i64 + 1, 1) };
    (next - days_from_civil(y, m as i64, 1)) as u32
}

/// Now, as a serial, in the Mac's own time zone.
pub fn local_now() -> f64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let offset = local_offset(since as i64) as f64;
    let local = since + offset;
    let days = (local / 86_400.0).floor();
    let base = days_from_civil(1899, 12, 30);
    (days - base as f64) + (local - days * 86_400.0) / 86_400.0
}

#[cfg(unix)]
fn local_offset(t: i64) -> i64 {
    // SAFETY: `localtime_r` writes into the `tm` given, from a `time_t`
    // given; nothing else is touched.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let at: libc::time_t = t as libc::time_t;
        if libc::localtime_r(&at, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
}

#[cfg(not(unix))]
fn local_offset(_: i64) -> i64 {
    0
}

/// Text read as a date as a cell reads typing: `2024-03-15`, `15/03/2024`
/// is not read (its order is ambiguous), `3/15/2024`, `15 Mar 2024`,
/// `Mar 15, 2024`. A time after it is read too: `2024-03-15 14:30`.
pub fn parse_date(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, time) = match s.split_once(' ') {
        Some((d, t)) if t.contains(':') => (d, Some(t)),
        _ => (s, None),
    };
    let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let month_of = |w: &str| months.iter().position(|m| w.to_lowercase().starts_with(m)).map(|i| i as i64 + 1);
    let num = |w: &str| w.parse::<i64>().ok();
    let day = if let Some((y, rest)) = date.split_once('-') {
        let (m, d) = rest.split_once('-')?;
        if y.len() != 4 {
            return None;
        }
        serial(num(y)?, num(m)?, num(d)?)?
    } else if date.contains('/') {
        let parts: Vec<&str> = date.split('/').collect();
        if parts.len() != 3 || parts[2].len() != 4 {
            return None;
        }
        let (m, d) = (num(parts[0])?, num(parts[1])?);
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        serial(num(parts[2])?, m, d)?
    } else {
        let words: Vec<&str> = s.split([' ', ',']).filter(|w| !w.is_empty()).collect();
        match words.as_slice() {
            [d, m, y] if num(d).is_some() => serial(num(y)?, month_of(m)?, num(d)?)?,
            [m, d, y] => serial(num(y)?, month_of(m)?, num(d)?)?,
            _ => return None,
        }
    };
    let frac = match time {
        Some(t) => parse_time(t)?,
        None => 0.0,
    };
    Some(day + frac)
}

/// `14:30`, `14:30:15`, `2:30 PM`, as a fraction of a day.
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let (clock, pm) = match s.to_ascii_uppercase() {
        u if u.ends_with("PM") => (s[..s.len() - 2].trim(), Some(true)),
        u if u.ends_with("AM") => (s[..s.len() - 2].trim(), Some(false)),
        _ => (s, None),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let sec: f64 = parts.get(2).map_or(Some(0.0), |p| p.parse().ok())?;
    if let Some(pm) = pm {
        if !(1..=12).contains(&h) {
            return None;
        }
        h = h % 12 + if pm { 12 } else { 0 };
    }
    if h > 23 || m > 59 || sec >= 60.0 {
        return None;
    }
    Some((h as f64 * 3600.0 + m as f64 * 60.0 + sec) / 86_400.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serials_are_excels_lotus_leap_day_and_all() {
        assert_eq!(serial(1900, 1, 1), Some(1.0));
        assert_eq!(serial(1900, 2, 28), Some(59.0));
        assert_eq!(serial(1900, 3, 1), Some(61.0));
        assert_eq!(ymd(60.0), Some((1900, 2, 29)), "the day that was not");
        assert_eq!(serial(2024, 3, 15), Some(45366.0));
        assert_eq!(serial(2024, 13, 1), serial(2025, 1, 1), "months carry over");
        assert_eq!(serial(2024, 3, 0), serial(2024, 2, 29), "so do days, back");
        assert_eq!(ymd(45366.75), Some((2024, 3, 15)));
        assert_eq!(hms(45366.75), (18, 0, 0));
        assert_eq!(weekday(45366.0), 5, "15 March 2024 was a Friday");
        assert_eq!(month_days(2024, 2), 29);
        assert_eq!(serial(1900, 2, 29), Some(60.0));
        assert_eq!(serial(1899, 12, 31), serial(3799, 12, 31), "years before 1900 count from it, as DATE's do");
        assert_eq!(serial(10_000, 1, 1), None);
    }

    #[test]
    fn typed_dates_and_times_are_read() {
        assert_eq!(parse_date("2024-03-15"), Some(45366.0));
        assert_eq!(parse_date("3/15/2024"), Some(45366.0));
        assert_eq!(parse_date("15 Mar 2024"), Some(45366.0));
        assert_eq!(parse_date("March 15, 2024"), Some(45366.0));
        assert_eq!(parse_date("2024-03-15 18:00"), Some(45366.75));
        assert_eq!(parse_time("2:30 PM"), Some(14.5 / 24.0));
        assert_eq!(parse_date("15/03/2024"), None);
        assert_eq!(parse_date("not a date"), None);
    }
}
