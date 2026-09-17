#[allow(unused_imports)]
use super::*;

/// a timer ticket for a workflow VM effect that completes at a known instant.
///
/// the infrastructure effect host publishes one of these instead of sleeping in-process, carrying
/// the terminal [`EffectResult`] it would have returned; the waker is the sole consumer and relays
/// a [`WsIngressCommand::SettleEffect`] once due. the result is carried rather than rebuilt so the
/// waker needs no database and the settle path stays the ordinary effect-result path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCommand {
    /// the instant this wake becomes due and its result should be handed back.
    pub due_at: DateTime<Utc>,
    /// the effect result to publish once due. its `timestamp` is already stamped at `due_at`, so a
    /// late relay never backdates or forward-dates the settlement.
    pub result: EffectResult,
    /// correlation id minted when this wake is published, carried through the waker into the
    /// resulting [`WsIngressCommand::SettleEffect`] so a stuck or delayed wake can be traced end to
    /// end. defaults for backward-compatible deserialization of older messages.
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    /// A workflow-level periodic timer interrupt. When present, the waker relays an ingress
    /// command instead of settling `result`; `result` remains populated for wire compatibility
    /// with effect wakes and is intentionally ignored by the timer-interrupt path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_interrupt: Option<TimerInterruptWake>,
    /// A provider-neutral nudge for a durable coalescing window. The pending-intent row remains
    /// authoritative; the waker merely tells an engine replica that its deadline has arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_intent: Option<OrchestrationIntentWake>,
    /// Current server-managed waker limits. Older wakes omit this and leave the receiving waker's
    /// process settings unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waker_settings: Option<WakerSettings>,
}

impl WakeCommand {
    pub fn new(due_at: DateTime<Utc>, result: EffectResult, trace_id: Uuid) -> Self {
        Self {
            due_at,
            result,
            trace_id,
            timer_interrupt: None,
            orchestration_intent: None,
            waker_settings: None,
        }
    }

    /// Build a wake for a workflow-owned periodic timer. `result` is a compatibility marker only:
    /// the waker detects `timer_interrupt` and never sends it to the effect-settlement path.
    pub fn timer_interrupt(
        due_at: DateTime<Utc>,
        workflow_run_id: Uuid,
        timer_id: impl Into<String>,
        interval_seconds: i64,
        trace_id: Uuid,
    ) -> Self {
        Self {
            due_at,
            result: EffectResult {
                workspace_commit: None,
                version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
                event_id: Uuid::now_v7(),
                effect_id: Uuid::now_v7(),
                workflow_run_id,
                continuation_id: Uuid::nil(),
                attempt: 0,
                ai_usage: None,
                kind: EffectResultKind::Status {
                    status: WorkflowEffectStatus::Succeeded,
                    output: None,
                    message: None,
                },
                timestamp: due_at,
                trace_id,
                notification_delivery_id: None,
            },
            trace_id,
            timer_interrupt: Some(TimerInterruptWake {
                workflow_run_id,
                timer_id: timer_id.into(),
                interval_seconds,
            }),
            orchestration_intent: None,
            waker_settings: None,
        }
    }

    /// Build an opaque coalescing-deadline wake. The compatibility result is never settled: the
    /// waker recognizes `orchestration_intent` and relays the typed ingress nudge instead.
    pub fn orchestration_intent(
        due_at: DateTime<Utc>,
        binding_id: Uuid,
        intent: impl Into<String>,
        trace_id: Uuid,
    ) -> Self {
        Self {
            due_at,
            result: EffectResult {
                workspace_commit: None,
                version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
                event_id: Uuid::now_v7(),
                effect_id: Uuid::nil(),
                workflow_run_id: Uuid::nil(),
                continuation_id: Uuid::nil(),
                attempt: 0,
                ai_usage: None,
                kind: EffectResultKind::Status {
                    status: WorkflowEffectStatus::Succeeded,
                    output: None,
                    message: None,
                },
                timestamp: due_at,
                trace_id,
                notification_delivery_id: None,
            },
            trace_id,
            timer_interrupt: None,
            orchestration_intent: Some(OrchestrationIntentWake {
                binding_id,
                intent: intent.into(),
            }),
            waker_settings: None,
        }
    }

    /// Attach the operating policy the receiving broker-only waker should adopt.
    pub fn with_waker_settings(mut self, settings: WakerSettings) -> Self {
        self.waker_settings = Some(settings);
        self
    }

    /// the effect this wake settles.
    pub fn effect_id(&self) -> Uuid {
        self.result.effect_id
    }

    /// the run this wake settles an effect for.
    pub fn workflow_run_id(&self) -> Uuid {
        self.result.workflow_run_id
    }

    /// stable identity for broker deduplication while a wake is in flight. keyed on the attempt so
    /// a retried effect arms a new timer rather than colliding with the one it replaced.
    pub fn dedupe_key(&self) -> String {
        if let Some(intent) = &self.orchestration_intent {
            return format!(
                "orchestration-intent:{}:{}:{}",
                intent.binding_id,
                intent.intent,
                self.due_at.timestamp_millis()
            );
        }
        if let Some(timer) = &self.timer_interrupt {
            return format!(
                "timer-interrupt:{}:{}:{}",
                timer.workflow_run_id,
                timer.timer_id,
                self.due_at.timestamp()
            );
        }
        format!("{}:{}", self.result.effect_id, self.result.attempt)
    }
}
