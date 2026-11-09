//! Dates and periods as people say them.
//!
//! Henrri list filters take `fromDate` and `toDate` as `yyyy-MM-dd`, both
//! **inclusive**, and write bodies take datetimes. People say "last month".
//! This module is the translation: `today`, `yesterday`, `2026-09-15`,
//! `15/09/2026` for a day, and `this month`, `last month`, `2026-09`, `2026`,
//! `last 30 days` for a period.

use chrono::{Datelike, Duration, Local, NaiveDate};

use crate::error::{Error, Result};

/// Today, in the timezone of the machine running the server.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Format a date the way the API's query strings want it.
pub fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Read one calendar day.
pub fn date(raw: &str) -> Result<NaiveDate> {
    let lower = raw.trim().to_lowercase();
    let today = today();
    match lower.as_str() {
        "today" | "aujourd'hui" | "aujourdhui" => return Ok(today),
        "yesterday" | "hier" => return Ok(today - Duration::days(1)),
        "tomorrow" | "demain" => return Ok(today + Duration::days(1)),
        _ => {}
    }
    if let Some(days) = lower
        .strip_suffix(" days ago")
        .or_else(|| lower.strip_suffix(" day ago"))
        .and_then(|n| n.trim().parse::<i64>().ok())
    {
        return Ok(today - Duration::days(days));
    }
    if let Some(days) = lower
        .strip_prefix("in ")
        .and_then(|r| r.strip_suffix(" days"))
        .and_then(|n| n.trim().parse::<i64>().ok())
    {
        return Ok(today + Duration::days(days));
    }
    // A datetime names a day too.
    let day_part = lower
        .get(..10)
        .filter(|_| lower.len() > 10)
        .unwrap_or(&lower);
    for format in ["%Y-%m-%d", "%d/%m/%Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(day_part, format) {
            return Ok(date);
        }
    }
    Err(Error::invalid(format!(
        "{raw:?} is not a date. Use YYYY-MM-DD, DD/MM/YYYY, today, yesterday, \"in 30 days\" or \"N days ago\"."
    )))
}

/// Read a datetime for a request body: a full datetime is kept as is, a day
/// becomes its midnight, local time.
pub fn datetime(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.len() > 10
        && (chrono::DateTime::parse_from_rfc3339(trimmed).is_ok()
            || chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S").is_ok())
    {
        return Ok(trimmed.to_owned());
    }
    Ok(format!("{}T00:00:00", iso(date(trimmed)?)))
}

/// Read a period into `[from, to]`, both inclusive, as Henrri filters on.
pub fn period(raw: &str) -> Result<(NaiveDate, NaiveDate)> {
    let lower = raw.trim().to_lowercase();
    let today = today();
    let month_start = |d: NaiveDate| d.with_day(1).unwrap_or(d);
    let month_end = |d: NaiveDate| {
        let (y, m) = if d.month() == 12 {
            (d.year() + 1, 1)
        } else {
            (d.year(), d.month() + 1)
        };
        NaiveDate::from_ymd_opt(y, m, 1).map_or(d, |next| next - Duration::days(1))
    };
    let year = |y: i32| {
        (
            NaiveDate::from_ymd_opt(y, 1, 1).unwrap_or(today),
            NaiveDate::from_ymd_opt(y, 12, 31).unwrap_or(today),
        )
    };
    let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));

    let range = match lower.as_str() {
        "today" | "aujourd'hui" => (today, today),
        "yesterday" | "hier" => (today - Duration::days(1), today - Duration::days(1)),
        "this week" | "cette semaine" => (monday, monday + Duration::days(6)),
        "last week" | "la semaine dernière" | "semaine dernière" => {
            (monday - Duration::weeks(1), monday - Duration::days(1))
        }
        "this month" | "ce mois" | "ce mois-ci" => (month_start(today), month_end(today)),
        "last month" | "le mois dernier" | "mois dernier" => {
            let last = month_start(today) - Duration::days(1);
            (month_start(last), last)
        }
        "this year" | "cette année" => year(today.year()),
        "last year" | "l'année dernière" | "année dernière" => year(today.year() - 1),
        other => {
            if let Some(n) = other
                .strip_prefix("last ")
                .or_else(|| other.strip_prefix("past "))
                .and_then(|r| r.strip_suffix(" days"))
                .and_then(|n| n.trim().parse::<i64>().ok())
                .filter(|n| *n > 0)
            {
                (today - Duration::days(n - 1), today)
            } else if let Some(y) = other
                .parse::<i32>()
                .ok()
                .filter(|y| (2000..=2100).contains(y))
            {
                year(y)
            } else if let Ok(start) = NaiveDate::parse_from_str(&format!("{other}-01"), "%Y-%m-%d")
            {
                (start, month_end(start))
            } else if let Ok(day) = date(other) {
                (day, day)
            } else {
                return Err(Error::invalid(format!(
                    "{raw:?} is not a period. Use today, yesterday, this week, last week, this month, last month, this year, last year, \"last 30 days\", 2026, 2026-09 or 2026-09-15."
                )));
            }
        }
    };
    Ok(range)
}

/// Resolve the `period` / `from_date` / `to_date` trio every list tool takes.
///
/// An explicit bound wins over the period's, so "last month, but only from the
/// 10th" is expressible.
pub fn range(
    period_arg: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(Option<String>, Option<String>)> {
    let (mut start, mut end) = match period_arg.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let (a, b) = period(p)?;
            (Some(iso(a)), Some(iso(b)))
        }
        None => (None, None),
    };
    if let Some(from) = from.map(str::trim).filter(|s| !s.is_empty()) {
        start = Some(iso(date(from)?));
    }
    if let Some(to) = to.map(str::trim).filter(|s| !s.is_empty()) {
        end = Some(iso(date(to)?));
    }
    if let (Some(a), Some(b)) = (&start, &end)
        && a > b
    {
        return Err(Error::invalid(format!(
            "the range starts on {a}, after it ends on {b}"
        )));
    }
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn days_read_in_both_spellings_and_from_datetimes() {
        assert_eq!(date("2026-09-15").unwrap(), d("2026-09-15"));
        assert_eq!(date("15/09/2026").unwrap(), d("2026-09-15"));
        assert_eq!(date("2026-09-15T10:00:00+02:00").unwrap(), d("2026-09-15"));
        assert!(date("someday").is_err());
    }

    #[test]
    fn a_month_is_inclusive_on_both_ends() {
        assert_eq!(
            period("2026-02").unwrap(),
            (d("2026-02-01"), d("2026-02-28"))
        );
        assert_eq!(
            period("2026-12").unwrap(),
            (d("2026-12-01"), d("2026-12-31"))
        );
        assert_eq!(period("2026").unwrap(), (d("2026-01-01"), d("2026-12-31")));
        assert_eq!(
            period("2026-09-15").unwrap(),
            (d("2026-09-15"), d("2026-09-15"))
        );
    }

    #[test]
    fn relative_periods_end_today() {
        let (from, to) = period("last 30 days").unwrap();
        assert_eq!(to, today());
        assert_eq!((to - from).num_days(), 29);
        let (from, to) = period("last month").unwrap();
        assert_eq!(from.day(), 1);
        assert!(to < today().with_day(1).unwrap());
    }

    #[test]
    fn an_explicit_bound_wins_over_the_period() {
        let (from, to) = range(Some("2026-09"), Some("2026-09-10"), None).unwrap();
        assert_eq!(from.as_deref(), Some("2026-09-10"));
        assert_eq!(to.as_deref(), Some("2026-09-30"));
        assert!(range(None, Some("2026-10-01"), Some("2026-09-01")).is_err());
    }

    #[test]
    fn a_day_becomes_a_midnight_datetime_and_a_datetime_is_kept() {
        assert_eq!(datetime("2026-09-15").unwrap(), "2026-09-15T00:00:00");
        assert_eq!(
            datetime("2026-09-15T10:30:00+02:00").unwrap(),
            "2026-09-15T10:30:00+02:00"
        );
    }
}
