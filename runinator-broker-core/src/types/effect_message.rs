#[allow(unused_imports)]
use super::*;

/// A VM effect command queued for a provider worker or an infrastructure effect host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectMessage {
    pub command: EffectCommand,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
    /// Absolute point after which this command must not be handed to an executor.
    ///
    /// `None` preserves wire compatibility. Receivers derive a bounded fallback for older provider
    /// actions; infrastructure effects continue to use the lifetime governed by their request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl EffectMessage {
    pub fn dedupe_key_or_hash(&self) -> String {
        self.dedupe_key
            .clone()
            .unwrap_or_else(|| self.command.effect_id.to_string())
    }

    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.effective_expires_at()
            .is_some_and(|expires_at| expires_at <= now)
    }

    /// Resolve the explicit wire expiry, or derive one for a provider action published by an
    /// older engine. This makes an upgrade drain an existing stale backlog safely instead of only
    /// protecting commands created after the upgrade.
    pub fn effective_expires_at(&self) -> Option<DateTime<Utc>> {
        effect_expires_at(&self.command, self.enqueued_at, self.expires_at)
    }
}
