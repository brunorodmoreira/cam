//! Calendar dates without a date crate: today's UTC date and day arithmetic,
//! using Howard Hinnant's `days_from_civil` / `civil_from_days` algorithms.

use std::time::{SystemTime, UNIX_EPOCH};

use toml::value::Date;

/// Today's date in UTC.
pub fn today() -> Date {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    from_days(i64::try_from(secs / 86_400).unwrap_or(0))
}

/// Days since 1970-01-01.
pub fn to_days(date: Date) -> i64 {
    let month = i64::from(date.month);
    let year = i64::from(date.year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + i64::from(date.day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date `days` after 1970-01-01.
pub fn from_days(days: i64) -> Date {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    Date {
        year: year as u16,
        month: month as u8,
        day: day as u8,
    }
}

/// How long ago `date` was, as of `today`: "today", "1 day ago", "12 days ago".
pub fn age(date: Date, today: Date) -> String {
    match to_days(today) - to_days(date) {
        days if days < 0 => "in the future".to_owned(),
        0 => "today".to_owned(),
        1 => "1 day ago".to_owned(),
        days => format!("{days} days ago"),
    }
}

/// How far ahead `date` is, as of `today`: "today", "in 1 day", "in 12 days".
pub fn until(date: Date, today: Date) -> String {
    match to_days(date) - to_days(today) {
        days if days < 0 => age(date, today),
        0 => "today".to_owned(),
        1 => "in 1 day".to_owned(),
        days => format!("in {days} days"),
    }
}

/// The day of an ISO 8601 timestamp, such as `2026-11-16T15:59:25.959Z`.
/// Only the date part is read; it must be a real calendar day.
pub fn parse_day(timestamp: &str) -> Option<Date> {
    let day = timestamp.get(..10)?;
    let rest = &timestamp[10..];
    if !(rest.is_empty() || rest.starts_with('T')) {
        return None;
    }
    let mut parts = day.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let date = Date {
        year: year.parse().ok()?,
        month: month.parse().ok()?,
        day: day.parse().ok()?,
    };
    (from_days(to_days(date)) == date).then_some(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: u16, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    #[test]
    fn known_days() {
        assert_eq!(to_days(date(1970, 1, 1)), 0);
        assert_eq!(to_days(date(1970, 1, 2)), 1);
        assert_eq!(to_days(date(1969, 12, 31)), -1);
        assert_eq!(to_days(date(2000, 3, 1)), 11_017);
        assert_eq!(to_days(date(2026, 11, 11)), 20_768);
    }

    #[test]
    fn leap_years() {
        assert_eq!(to_days(date(2024, 3, 1)) - to_days(date(2024, 2, 28)), 2);
        assert_eq!(to_days(date(2100, 3, 1)) - to_days(date(2100, 2, 28)), 1);
        assert_eq!(to_days(date(2000, 3, 1)) - to_days(date(2000, 2, 28)), 2);
    }

    #[test]
    fn round_trips() {
        for days in -700_000..800_000 {
            assert_eq!(to_days(from_days(days)), days, "{days}");
        }
        assert_eq!(from_days(20_768), date(2026, 11, 11));
    }

    #[test]
    fn today_is_plausible() {
        let today = today();
        assert!(today.year >= 2026 && (1..=12).contains(&today.month));
    }

    #[test]
    fn ages() {
        let today = date(2026, 11, 11);
        assert_eq!(age(date(2026, 11, 11), today), "today");
        assert_eq!(age(date(2026, 11, 10), today), "1 day ago");
        assert_eq!(age(date(2025, 11, 11), today), "365 days ago");
        assert_eq!(age(date(2026, 11, 12), today), "in the future");
    }

    #[test]
    fn until_counts_ahead() {
        let today = date(2026, 11, 11);
        assert_eq!(until(date(2026, 11, 11), today), "today");
        assert_eq!(until(date(2026, 11, 12), today), "in 1 day");
        assert_eq!(until(date(2026, 12, 9), today), "in 28 days");
        assert_eq!(until(date(2026, 11, 10), today), "1 day ago");
    }

    #[test]
    fn parses_the_day_of_a_timestamp() {
        assert_eq!(
            parse_day("2026-11-16T15:59:25.959Z"),
            Some(date(2026, 11, 16))
        );
        assert_eq!(parse_day("2026-11-16"), Some(date(2026, 11, 16)));
        for bad in [
            "",
            "2026-11",
            "2026-13-01T00:00Z",
            "2026-02-30",
            "26-11-16",
            "2026-11-16x",
            "2026/11/16",
        ] {
            assert_eq!(parse_day(bad), None, "{bad}");
        }
    }
}
