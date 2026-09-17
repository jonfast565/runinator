#[allow(unused_imports)]
use super::*;

/// A portable calendar schedule. Consumers decide whether an occurrence fires work (duration zero)
/// or opens a window (positive duration); recurrence only answers *when*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSpec {
    pub recurrence: ScheduleRecurrence,
    #[serde(default = "default_schedule_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub duration_seconds: i64,
}

impl ScheduleSpec {
    pub fn once(starts_at: DateTime<Utc>, ends_at: DateTime<Utc>) -> Self {
        Self {
            recurrence: ScheduleRecurrence::Once { at: starts_at },
            timezone: default_schedule_timezone(),
            duration_seconds: (ends_at - starts_at).num_seconds(),
        }
    }
}

impl Validate for ScheduleSpec {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("timezone", &self.timezone, SHORT_TEXT_MAX)?;
        if self.duration_seconds < 0 {
            return Err(ValidationError::new(
                "duration_seconds",
                "must not be negative",
            ));
        }
        match &self.recurrence {
            ScheduleRecurrence::Cron { expression } => {
                required_text("recurrence.expression", expression, SHORT_TEXT_MAX)?;
            }
            ScheduleRecurrence::Weekdays {
                days,
                hour,
                minute,
                second,
            } => {
                if days.is_empty() {
                    return Err(ValidationError::new(
                        "recurrence.days",
                        "select at least one weekday",
                    ));
                }
                if *hour > 23 || *minute > 59 || *second > 59 {
                    return Err(ValidationError::new(
                        "recurrence.time",
                        "must be a valid wall-clock time",
                    ));
                }
            }
            ScheduleRecurrence::Rrule { rule, .. } => {
                required_text("recurrence.rule", rule, LONG_TEXT_MAX)?;
            }
            ScheduleRecurrence::Once { .. } => {}
        }
        Ok(())
    }
}
