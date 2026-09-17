#[allow(unused_imports)]
use super::*;

/// the platform-set price list. a missing (backend, kind) pair is treated as free (0 cents).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateCard {
    pub entries: Vec<RateEntry>,
    #[serde(default)]
    pub ai_entries: Vec<AiRateEntry>,
}

impl RateCard {
    /// the hourly cents for a (backend, kind), or 0 when unpriced.
    pub fn hourly_cents(&self, backend: ProvisionBackend, kind: ReplicaKind) -> u32 {
        self.entries
            .iter()
            .find(|entry| entry.backend == backend && entry.kind == kind)
            .map(|entry| entry.hourly_cents)
            .unwrap_or(0)
    }

    pub fn ai_rate(&self, provider: &str, model: &str) -> Option<&AiRateEntry> {
        self.ai_entries
            .iter()
            .find(|entry| entry.provider == provider && entry.model == model)
            .or_else(|| {
                self.ai_entries
                    .iter()
                    .find(|entry| entry.provider == provider && entry.model == "*")
            })
    }

    pub fn price_ai_usage(
        &self,
        provider: &str,
        model: &str,
        tokens: &AiTokenUsage,
    ) -> Option<u64> {
        self.ai_rate(provider, model)
            .map(|entry| entry.price(tokens))
    }

    /// a conservative default price list so costs are non-zero out of the box.
    pub fn default_card() -> Self {
        let kinds = [
            ReplicaKind::Worker,
            ReplicaKind::Waker,
            ReplicaKind::Webservice,
            ReplicaKind::Postgres,
        ];
        let mut entries = Vec::new();
        for backend in [ProvisionBackend::Supervisor, ProvisionBackend::Kubernetes] {
            for kind in kinds {
                // workers are the costly compute; waker/webservice are lighter control-plane nodes.
                let hourly_cents = match kind {
                    ReplicaKind::Worker => 25,
                    ReplicaKind::Waker => 5,
                    ReplicaKind::Webservice => 10,
                    ReplicaKind::Postgres => 20,
                    // the archiver and engine worker are not provisioned/billed nodes; they only
                    // register for visibility.
                    ReplicaKind::Archiver | ReplicaKind::Background => 0,
                };
                entries.push(RateEntry {
                    backend,
                    kind,
                    hourly_cents,
                });
            }
        }
        Self {
            entries,
            ai_entries: Vec::new(),
        }
    }
}
