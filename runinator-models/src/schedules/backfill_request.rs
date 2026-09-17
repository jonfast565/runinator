#[allow(unused_imports)]
use super::*;

/// the time range a manual backfill replays. inclusive of `to`, exclusive of `from`, matching the
/// cron iterator's own half-open stepping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillRequest {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// cap on the number of slots replayed. defaults to [`DEFAULT_BACKFILL_LIMIT`].
    #[serde(default)]
    pub limit: Option<i64>,
    /// when true, report the slots that would fire without creating any runs.
    #[serde(default)]
    pub dry_run: bool,
}

impl Validate for BackfillRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.to <= self.from {
            return Err(ValidationError::new("to", "must be later than from"));
        }
        positive_limit("limit", self.limit, MAX_BACKFILL_LIMIT)
    }
}
