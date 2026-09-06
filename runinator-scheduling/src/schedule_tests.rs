//! recurrence cursors and half-open schedule windows.

use super::*;
use chrono::TimeZone;

fn hourly_rrule() -> ScheduleSpec {
    ScheduleSpec {
        recurrence: ScheduleRecurrence::Rrule {
            rule: "FREQ=HOURLY;COUNT=4".into(),
            dtstart: at(2026, 9, 1, 7),
        },
        timezone: "UTC".into(),
        duration_seconds: 0,
    }
}

#[test]
fn rrule_cursor_is_strict_at_occurrences_and_preserves_subsecond_bounds() {
    let spec = hourly_rrule();
    let start = at(2026, 9, 1, 7);
    assert_eq!(
        next_after(&spec, start).unwrap(),
        start + Duration::hours(1)
    );
    assert_eq!(
        next_after(&spec, start - Duration::nanoseconds(1)).unwrap(),
        start
    );
    assert_eq!(
        next_after(&spec, start + Duration::milliseconds(500)).unwrap(),
        start + Duration::hours(1)
    );
    assert!(matches!(
        next_after(&spec, start + Duration::hours(3)),
        Err(ScheduleError::Exhausted)
    ));
}

#[test]
fn rrule_ranges_advance_without_duplicate_slots_and_report_truncation() {
    let spec = hourly_rrule();
    let start = at(2026, 9, 1, 7);
    let until = start + Duration::hours(3);
    let expected = vec![
        start + Duration::hours(1),
        start + Duration::hours(2),
        until,
    ];
    assert_eq!(
        between(&spec, start, until, 10).unwrap(),
        (expected.clone(), false)
    );
    assert_eq!(
        between(&spec, start, until, 2).unwrap(),
        (expected[..2].to_vec(), true)
    );
    assert_eq!(between(&spec, start, until, 3).unwrap(), (expected, false));
    assert_eq!(between(&spec, start, until, 0).unwrap(), (vec![], true));
}

#[test]
fn expired_occurrences_do_not_hide_overlapping_active_windows() {
    for recurrence in [
        ScheduleRecurrence::Cron {
            expression: "* * * * *".into(),
        },
        ScheduleRecurrence::Rrule {
            rule: "FREQ=MINUTELY".into(),
            dtstart: at(2026, 9, 1, 7),
        },
    ] {
        let spec = ScheduleSpec {
            recurrence,
            timezone: "UTC".into(),
            duration_seconds: 120,
        };
        let now = at(2026, 9, 1, 8) + Duration::minutes(2);
        for instant in [now, now + Duration::milliseconds(500)] {
            assert!(is_excluded(&spec, instant).unwrap());
            assert_eq!(
                current_or_next_window(&spec, instant).unwrap(),
                (now - Duration::minutes(1), now + Duration::minutes(1))
            );
        }
    }
}

#[test]
fn once_windows_include_the_start_and_exclude_the_end() {
    let start = at(2026, 9, 1, 7);
    let end = start + Duration::hours(1);
    let spec = ScheduleSpec::once(start, end);
    assert!(!is_excluded(&spec, start - Duration::nanoseconds(1)).unwrap());
    assert!(is_excluded(&spec, start).unwrap());
    assert!(is_excluded(&spec, end - Duration::nanoseconds(1)).unwrap());
    assert!(!is_excluded(&spec, end).unwrap());
    assert!(matches!(
        current_or_next_window(&spec, end),
        Err(ScheduleError::Exhausted)
    ));
    assert_eq!(
        current_or_next_window(&spec, start - Duration::hours(1)).unwrap(),
        (start, end)
    );
}

#[test]
fn unrepresentable_durations_and_windows_return_errors() {
    let mut spec = hourly_rrule();
    for seconds in [-1, i64::MAX] {
        spec.duration_seconds = seconds;
        assert!(matches!(validate(&spec), Err(ScheduleError::Duration)));
        assert!(matches!(
            current_or_next_window(&spec, at(2026, 9, 1, 7)),
            Err(ScheduleError::Duration)
        ));
    }
    spec = ScheduleSpec::once(
        DateTime::<Utc>::MAX_UTC - Duration::seconds(1),
        DateTime::<Utc>::MAX_UTC,
    );
    spec.duration_seconds = 60;
    assert!(matches!(
        current_or_next_window(&spec, DateTime::<Utc>::MAX_UTC),
        Err(ScheduleError::DateRange)
    ));
    assert!(matches!(
        current_or_next_window(&spec, DateTime::<Utc>::MIN_UTC),
        Err(ScheduleError::DateRange)
    ));
}

#[test]
fn schedule_error_messages_match_the_dictionary() {
    for (error, descriptor) in [
        (ScheduleError::Timezone("invalid".into()), errors::TIMEZONE),
        (ScheduleError::Cron("invalid".into()), errors::CRON),
        (ScheduleError::Rrule("invalid".into()), errors::RRULE),
        (ScheduleError::Exhausted, errors::EXHAUSTED),
        (ScheduleError::WeekdayTime, errors::WEEKDAY_TIME),
        (ScheduleError::Duration, errors::DURATION),
        (ScheduleError::DateRange, errors::DATE_RANGE),
    ] {
        assert!(
            error
                .to_string()
                .starts_with(&format!("{} - {}", descriptor.code, descriptor.summary))
        );
    }
}

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
