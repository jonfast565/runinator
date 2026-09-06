use runinator_models::errors::ErrorDescriptor;

pub const TIMEZONE: ErrorDescriptor =
    ErrorDescriptor::new("SCHEDULE001", "schedule.timezone", "Unknown IANA timezone");
pub const CRON: ErrorDescriptor =
    ErrorDescriptor::new("SCHEDULE002", "schedule.cron", "Invalid cron expression");
pub const RRULE: ErrorDescriptor =
    ErrorDescriptor::new("SCHEDULE003", "schedule.rrule", "Invalid RRULE");
pub const EXHAUSTED: ErrorDescriptor = ErrorDescriptor::new(
    "SCHEDULE004",
    "schedule.exhausted",
    "Schedule has no future occurrence",
);
pub const WEEKDAY_TIME: ErrorDescriptor = ErrorDescriptor::new(
    "SCHEDULE005",
    "schedule.weekday_time",
    "Invalid weekday wall-clock time",
);
pub const DURATION: ErrorDescriptor = ErrorDescriptor::new(
    "SCHEDULE006",
    "schedule.duration",
    "Invalid schedule duration",
);
pub const DATE_RANGE: ErrorDescriptor = ErrorDescriptor::new(
    "SCHEDULE007",
    "schedule.date_range",
    "Schedule window exceeds the supported date range",
);
