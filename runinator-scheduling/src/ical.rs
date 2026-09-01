//! RFC 5545 export for Outlook, Apple Calendar, and other subscription clients.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use runinator_models::schedules::{ScheduleRecurrence, ScheduleSpec, ScheduleWeekday};

use crate::{ScheduleError, between, is_excluded, next_after};

#[derive(Debug, Clone)]
pub struct CalendarEntry {
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub schedule: ScheduleSpec,
    pub exclusions: Vec<ScheduleSpec>,
}

/// Render a subscribable calendar. Cron has no lossless iCalendar equivalent, so its occurrences
/// are expanded over a rolling horizon; weekday and RRULE schedules remain native recurrences.
pub fn render(
    name: &str,
    entries: &[CalendarEntry],
    now: DateTime<Utc>,
    horizon_days: i64,
) -> Result<String, ScheduleError> {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//Runinator//Schedule Calendar//EN".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
        "METHOD:PUBLISH".to_string(),
        format!("X-WR-CALNAME:{}", escape(name)),
        "REFRESH-INTERVAL;VALUE=DURATION:PT15M".to_string(),
        "X-PUBLISHED-TTL:PT15M".to_string(),
    ];
    for entry in entries {
        match &entry.schedule.recurrence {
            ScheduleRecurrence::Cron { .. } => {
                let (dates, _) = between(
                    &entry.schedule,
                    now - Duration::minutes(1),
                    now + Duration::days(horizon_days.clamp(1, 366)),
                    2_000,
                )?;
                for date in dates {
                    if excluded(entry, date)? {
                        continue;
                    }
                    event(
                        &mut lines,
                        &format!("{}-{}", entry.uid, date.timestamp()),
                        entry,
                        date,
                        None,
                        &[],
                    );
                }
            }
            ScheduleRecurrence::Once { at } if !excluded(entry, *at)? => {
                event(&mut lines, &entry.uid, entry, *at, None, &[])
            }
            ScheduleRecurrence::Once { .. } => {}
            ScheduleRecurrence::Weekdays { days, .. } => {
                let start = next_after(&entry.schedule, now - Duration::minutes(1))?;
                let exdates = exclusion_dates(entry, now, horizon_days)?;
                event(
                    &mut lines,
                    &entry.uid,
                    entry,
                    start,
                    Some(format!(
                        "FREQ=WEEKLY;BYDAY={}",
                        days.iter().map(weekday).collect::<Vec<_>>().join(",")
                    )),
                    &exdates,
                );
            }
            ScheduleRecurrence::Rrule { rule, dtstart } => {
                let exdates = exclusion_dates(entry, now, horizon_days)?;
                event(
                    &mut lines,
                    &entry.uid,
                    entry,
                    *dtstart,
                    Some(
                        rule.trim()
                            .strip_prefix("RRULE:")
                            .unwrap_or(rule.trim())
                            .to_string(),
                    ),
                    &exdates,
                )
            }
        }
    }
    lines.push("END:VCALENDAR".to_string());
    Ok(lines
        .into_iter()
        .map(|line| fold(&line))
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n")
}

fn event(
    lines: &mut Vec<String>,
    uid: &str,
    entry: &CalendarEntry,
    start: DateTime<Utc>,
    rrule: Option<String>,
    exdates: &[DateTime<Utc>],
) {
    lines.push("BEGIN:VEVENT".into());
    lines.push(format!("UID:{}@runinator", escape(uid)));
    lines.push(format!("DTSTAMP:{}", utc(Utc::now())));
    lines.push(format!("SUMMARY:{}", escape(&entry.summary)));
    lines.push(format!("DESCRIPTION:{}", escape(&entry.description)));
    if rrule.is_some() && entry.schedule.timezone != "UTC" {
        if let Ok(zone) = entry.schedule.timezone.parse::<chrono_tz::Tz>() {
            lines.push(format!(
                "DTSTART;TZID={}:{}",
                entry.schedule.timezone,
                start.with_timezone(&zone).format("%Y%m%dT%H%M%S")
            ));
        } else {
            lines.push(format!("DTSTART:{}", utc(start)));
        }
    } else {
        lines.push(format!("DTSTART:{}", utc(start)));
    }
    if let Some(rrule) = rrule {
        lines.push(format!("RRULE:{rrule}"));
    }
    for date in exdates {
        if entry.schedule.timezone != "UTC"
            && let Ok(zone) = entry.schedule.timezone.parse::<chrono_tz::Tz>()
        {
            lines.push(format!(
                "EXDATE;TZID={}:{}",
                entry.schedule.timezone,
                date.with_timezone(&zone).format("%Y%m%dT%H%M%S")
            ));
        } else {
            lines.push(format!("EXDATE:{}", utc(*date)));
        }
    }
    lines.push(format!(
        "DURATION:{}",
        duration(entry.schedule.duration_seconds.max(60))
    ));
    lines.push("STATUS:CONFIRMED".into());
    lines.push("TRANSP:TRANSPARENT".into());
    lines.push("END:VEVENT".into());
}

fn excluded(entry: &CalendarEntry, date: DateTime<Utc>) -> Result<bool, ScheduleError> {
    for exclusion in &entry.exclusions {
        if is_excluded(exclusion, date)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn exclusion_dates(
    entry: &CalendarEntry,
    now: DateTime<Utc>,
    horizon_days: i64,
) -> Result<Vec<DateTime<Utc>>, ScheduleError> {
    let (dates, _) = between(
        &entry.schedule,
        now - Duration::minutes(1),
        now + Duration::days(horizon_days.clamp(1, 366)),
        2_000,
    )?;
    let mut excluded_dates = Vec::new();
    for date in dates {
        if excluded(entry, date)? {
            excluded_dates.push(date);
        }
    }
    Ok(excluded_dates)
}

fn utc(value: DateTime<Utc>) -> String {
    value
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(['-', ':'], "")
}

fn duration(seconds: i64) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("PT{hours}H{minutes}M{seconds}S")
}

fn weekday(day: &ScheduleWeekday) -> &'static str {
    match day {
        ScheduleWeekday::Monday => "MO",
        ScheduleWeekday::Tuesday => "TU",
        ScheduleWeekday::Wednesday => "WE",
        ScheduleWeekday::Thursday => "TH",
        ScheduleWeekday::Friday => "FR",
        ScheduleWeekday::Saturday => "SA",
        ScheduleWeekday::Sunday => "SU",
    }
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace(['\r', '\n'], "\\n")
}

fn fold(line: &str) -> String {
    if line.len() <= 73 {
        return line.to_string();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in line.chars() {
        let bytes = ch.len_utf8();
        if width + bytes > 73 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(ch);
        width += bytes;
    }
    out
}
