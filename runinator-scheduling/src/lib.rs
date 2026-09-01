//! Pure calendar evaluation shared by durable trigger and freeze-window schedulers.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;
use rrule::{RRule, RRuleSet, Unvalidated};
use runinator_models::schedules::{ScheduleRecurrence, ScheduleSpec, ScheduleWeekday};
use thiserror::Error;

pub mod ical;

const SEARCH_DAYS: i64 = 14;

#[derive(Debug, Error)]
pub enum ScheduleError {
    #[error("unknown IANA timezone '{0}'")]
    Timezone(String),
    #[error("invalid cron expression: {0}")]
    Cron(String),
    #[error("invalid RRULE: {0}")]
    Rrule(String),
    #[error("schedule has no future occurrence")]
    Exhausted,
    #[error("invalid weekday wall-clock time")]
    WeekdayTime,
}

pub type Result<T> = std::result::Result<T, ScheduleError>;

/// Validate both the portable shape and the recurrence parser/timezone used at runtime.
pub fn validate(spec: &ScheduleSpec) -> Result<()> {
    let _ = timezone(spec)?;
    match &spec.recurrence {
        ScheduleRecurrence::Once { .. } => Ok(()),
        ScheduleRecurrence::Cron { expression } => expression
            .parse::<Cron>()
            .map(|_| ())
            .map_err(|error| ScheduleError::Cron(error.to_string())),
        ScheduleRecurrence::Weekdays {
            days,
            hour,
            minute,
            second,
        } => {
            if days.is_empty()
                || NaiveTime::from_hms_opt(*hour as u32, *minute as u32, *second as u32).is_none()
            {
                return Err(ScheduleError::WeekdayTime);
            }
            Ok(())
        }
        ScheduleRecurrence::Rrule { rule, dtstart } => {
            let _ = rrule_set(rule, *dtstart, spec)?;
            Ok(())
        }
    }
}

/// First occurrence strictly after `after`.
pub fn next_after(spec: &ScheduleSpec, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    validate(spec)?;
    match &spec.recurrence {
        ScheduleRecurrence::Once { at } => {
            (*at > after).then_some(*at).ok_or(ScheduleError::Exhausted)
        }
        ScheduleRecurrence::Cron { expression } => {
            let zone = timezone(spec)?;
            let local_after = after.with_timezone(&zone);
            expression
                .parse::<Cron>()
                .map_err(|error| ScheduleError::Cron(error.to_string()))?
                .find_next_occurrence(&local_after, false)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| ScheduleError::Cron(error.to_string()))
        }
        ScheduleRecurrence::Weekdays {
            days,
            hour,
            minute,
            second,
        } => next_weekday(timezone(spec)?, days, *hour, *minute, *second, after),
        ScheduleRecurrence::Rrule { rule, dtstart } => {
            let set = rrule_set(rule, *dtstart, spec)?;
            let zone: rrule::Tz = timezone(spec)?.into();
            set.after(after.with_timezone(&zone))
                .all(1)
                .dates
                .into_iter()
                .next()
                .map(|value| value.with_timezone(&Utc))
                .ok_or(ScheduleError::Exhausted)
        }
    }
}

/// Occurrences strictly after `after` and at or before `until`, capped at `max`.
pub fn between(
    spec: &ScheduleSpec,
    after: DateTime<Utc>,
    until: DateTime<Utc>,
    max: i64,
) -> Result<(Vec<DateTime<Utc>>, bool)> {
    let mut dates = Vec::new();
    let mut cursor = after;
    loop {
        let next = match next_after(spec, cursor) {
            Ok(next) => next,
            Err(ScheduleError::Exhausted) => return Ok((dates, false)),
            Err(error) => return Err(error),
        };
        if next > until {
            return Ok((dates, false));
        }
        if dates.len() as i64 >= max.max(0) {
            return Ok((dates, true));
        }
        dates.push(next);
        cursor = next;
    }
}

/// The concrete active occurrence, or the next interval when no occurrence is active.
pub fn current_or_next_window(
    spec: &ScheduleSpec,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    if spec.duration_seconds <= 0 {
        return Err(ScheduleError::WeekdayTime);
    }
    let duration = Duration::seconds(spec.duration_seconds);
    // Looking from one duration before now is sufficient: an occurrence older than that cannot
    // still own a half-open interval containing now.
    let threshold = now - duration - Duration::seconds(1);
    let candidate = next_after(spec, threshold)?;
    let start = if candidate + duration > now {
        candidate
    } else {
        next_after(spec, now)?
    };
    Ok((start, start + duration))
}

/// Whether `instant` belongs to any half-open scheduled interval.
pub fn is_excluded(spec: &ScheduleSpec, instant: DateTime<Utc>) -> Result<bool> {
    if spec.duration_seconds <= 0 {
        return Ok(false);
    }
    let duration = Duration::seconds(spec.duration_seconds);
    let candidate = match next_after(spec, instant - duration - Duration::seconds(1)) {
        Ok(candidate) => candidate,
        Err(ScheduleError::Exhausted) => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(candidate <= instant && instant < candidate + duration)
}

fn timezone(spec: &ScheduleSpec) -> Result<Tz> {
    spec.timezone
        .parse()
        .map_err(|_| ScheduleError::Timezone(spec.timezone.clone()))
}

fn next_weekday(
    zone: Tz,
    days: &[ScheduleWeekday],
    hour: u8,
    minute: u8,
    second: u8,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let time = NaiveTime::from_hms_opt(hour as u32, minute as u32, second as u32)
        .ok_or(ScheduleError::WeekdayTime)?;
    let local_after = after.with_timezone(&zone);
    for offset in 0..SEARCH_DAYS {
        let date = local_after.date_naive() + Duration::days(offset);
        if !days.contains(&weekday(date)) {
            continue;
        }
        let naive = date.and_time(time);
        let local = match zone.from_local_datetime(&naive) {
            LocalResult::Single(value) => value,
            // On a fall-back transition, choose the first occurrence consistently.
            LocalResult::Ambiguous(first, _) => first,
            // A nonexistent spring-forward wall time has no occurrence that day.
            LocalResult::None => continue,
        };
        let utc = local.with_timezone(&Utc);
        if utc > after {
            return Ok(utc);
        }
    }
    Err(ScheduleError::Exhausted)
}

fn weekday(date: NaiveDate) -> ScheduleWeekday {
    match date.weekday() {
        chrono::Weekday::Mon => ScheduleWeekday::Monday,
        chrono::Weekday::Tue => ScheduleWeekday::Tuesday,
        chrono::Weekday::Wed => ScheduleWeekday::Wednesday,
        chrono::Weekday::Thu => ScheduleWeekday::Thursday,
        chrono::Weekday::Fri => ScheduleWeekday::Friday,
        chrono::Weekday::Sat => ScheduleWeekday::Saturday,
        chrono::Weekday::Sun => ScheduleWeekday::Sunday,
    }
}

fn rrule_set(rule: &str, dtstart: DateTime<Utc>, spec: &ScheduleSpec) -> Result<RRuleSet> {
    let zone: rrule::Tz = timezone(spec)?.into();
    let raw = rule.trim().strip_prefix("RRULE:").unwrap_or(rule.trim());
    let parsed: RRule<Unvalidated> = raw
        .parse()
        .map_err(|error: rrule::RRuleError| ScheduleError::Rrule(error.to_string()))?;
    parsed
        .build(dtstart.with_timezone(&zone))
        .map_err(|error| ScheduleError::Rrule(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
    }

    #[test]
    fn weekdays_keep_local_wall_time_across_dst() {
        let spec = ScheduleSpec {
            recurrence: ScheduleRecurrence::Weekdays {
                days: vec![ScheduleWeekday::Tuesday, ScheduleWeekday::Wednesday],
                hour: 3,
                minute: 0,
                second: 0,
            },
            timezone: "America/New_York".into(),
            duration_seconds: 7_200,
        };
        assert_eq!(
            next_after(&spec, at(2026, 3, 8, 8)).unwrap(),
            at(2026, 3, 10, 7)
        );
        assert!(is_excluded(&spec, at(2026, 3, 10, 8)).unwrap());
        assert!(!is_excluded(&spec, at(2026, 3, 10, 9)).unwrap());
    }

    #[test]
    fn rrule_supports_byday() {
        let spec = ScheduleSpec {
            recurrence: ScheduleRecurrence::Rrule {
                rule: "FREQ=WEEKLY;BYDAY=TU,WE".into(),
                dtstart: at(2026, 9, 1, 7),
            },
            timezone: "America/New_York".into(),
            duration_seconds: 7_200,
        };
        assert_eq!(
            next_after(&spec, at(2026, 9, 1, 8)).unwrap(),
            at(2026, 9, 2, 7)
        );
    }
}
