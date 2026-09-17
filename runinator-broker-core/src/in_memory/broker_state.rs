#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct BrokerState {
    pub(super) control_queue: VecDeque<ControlDelivery>,
    pub(super) control_inflight: HashMap<Uuid, Leased<ControlDelivery>>,
    pub(super) agent_queue: VecDeque<AgentDelivery>,
    pub(super) agent_inflight: HashMap<Uuid, Leased<AgentDelivery>>,
    pub(super) effect_queue: VecDeque<EffectDelivery>,
    pub(super) effect_inflight: HashMap<Uuid, Leased<EffectDelivery>>,
    pub(super) effect_dedupe: HashSet<String>,
    pub(super) effect_result_queue: VecDeque<EffectResultDelivery>,
    pub(super) effect_result_inflight: HashMap<Uuid, Leased<EffectResultDelivery>>,
    pub(super) effect_result_dedupe: HashSet<String>,
    pub(super) wake_queue: VecDeque<WakeDelivery>,
    pub(super) wake_inflight: HashMap<Uuid, Leased<WakeDelivery>>,
    pub(super) wake_dedupe: HashSet<String>,
    pub(super) ingress_queue: VecDeque<IngressDelivery>,
    pub(super) ingress_inflight: HashMap<Uuid, Leased<IngressDelivery>>,
    pub(super) ingress_dedupe: HashSet<String>,
}

impl BrokerState {
    pub(super) fn reclaim_expired_control(&mut self, now: Instant) {
        let expired = expired_ids(&self.control_inflight, now);
        for id in expired {
            if let Some(leased) = self.control_inflight.remove(&id) {
                self.control_queue
                    .push_front(redeliver_control(leased.delivery));
            }
        }
    }

    pub(super) fn reclaim_expired_agent(&mut self, now: Instant) {
        let expired = expired_ids(&self.agent_inflight, now);
        for id in expired {
            if let Some(leased) = self.agent_inflight.remove(&id) {
                self.agent_queue
                    .push_front(redeliver_agent(leased.delivery));
            }
        }
    }

    /// drop queued controls that have gone stale: a control targeted at a replica that never
    /// returns has no consumer that can ever match it, and controls are immediate signals, so
    /// retaining one past the ttl only grows the queue (this broker also backs the long-lived
    /// http/TCP servers).
    pub(super) fn drop_stale_control(&mut self, now: chrono::DateTime<chrono::Utc>) {
        self.control_queue.retain(|delivery| {
            (now - delivery.enqueued_at).num_seconds() < crate::STALE_CONTROL_TTL_SECONDS
        });
    }

    pub(super) fn reclaim_expired_effects(
        &mut self,
        now: Instant,
        wall_now: chrono::DateTime<chrono::Utc>,
    ) {
        for id in expired_ids(&self.effect_inflight, now) {
            let Some(leased) = self.effect_inflight.remove(&id) else {
                continue;
            };

            if leased.delivery.is_expired_at(wall_now) {
                self.effect_dedupe.remove(&leased.delivery.dedupe_key);
            } else {
                self.effect_queue
                    .push_front(redeliver_effect(leased.delivery));
            }
        }
    }

    pub(super) fn drop_expired_effects(&mut self, now: chrono::DateTime<chrono::Utc>) {
        let expired_dedupe: Vec<_> = self
            .effect_queue
            .iter()
            .filter(|delivery| delivery.is_expired_at(now))
            .map(|delivery| delivery.dedupe_key.clone())
            .collect();
        self.effect_queue
            .retain(|delivery| !delivery.is_expired_at(now));
        for dedupe_key in expired_dedupe {
            self.effect_dedupe.remove(&dedupe_key);
        }
    }

    pub(super) fn reclaim_expired_effect_results(&mut self, now: Instant) {
        for id in expired_ids(&self.effect_result_inflight, now) {
            if let Some(leased) = self.effect_result_inflight.remove(&id) {
                self.effect_result_queue
                    .push_front(redeliver_effect_result(leased.delivery));
            }
        }
    }

    pub(super) fn reclaim_expired_wakes(&mut self, now: Instant) {
        let expired = expired_ids(&self.wake_inflight, now);
        for id in expired {
            if let Some(leased) = self.wake_inflight.remove(&id) {
                self.wake_queue.push_front(redeliver_wake(leased.delivery));
            }
        }
    }

    pub(super) fn reclaim_expired_ingress(&mut self, now: Instant) {
        let expired = expired_ids(&self.ingress_inflight, now);
        for id in expired {
            if let Some(leased) = self.ingress_inflight.remove(&id) {
                self.ingress_queue
                    .push_front(redeliver_ingress(leased.delivery));
            }
        }
    }
}
