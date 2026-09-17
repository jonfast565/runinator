#[allow(unused_imports)]
use super::*;

pub(super) struct DesktopObserver {
    pub(super) shared: SharedHandle,
    /// the sandbox folder, which the shared status has no notion of.
    pub(super) root: String,
}

impl AgentObserver for DesktopObserver {
    fn on_log(&self, line: &str) {
        log_line(&self.shared, line);
    }

    fn on_status(&self, status: &runinator_worker::AgentStatus) {
        // decide the notification under the lock (so the latch is race-free), but fire it after
        // releasing — `notify` only spawns a thread, yet keeping platform calls off a held lock is
        // the habit worth keeping.
        let toast = {
            let Ok(mut guard) = self.shared.lock() else {
                return;
            };
            guard.connection = status.connection.clone();
            guard.metrics = status.metrics.clone();
            set_activity(
                &mut guard.agent_activity,
                connection_activity(&status.connection),
            );
            guard.status = AgentStatus {
                running: status.running,
                replica_id: status.replica_id,
                root: Some(self.root.clone()),
                broker_connection: status.broker_connection.clone(),
            };
            match &status.connection {
                ConnectionState::Reconnecting {
                    attempt,
                    max_attempts,
                    ..
                } if !guard.degraded_notified => {
                    guard.degraded_notified = true;
                    Some(Toast::Degraded {
                        attempt: *attempt,
                        max_attempts: *max_attempts,
                    })
                }
                ConnectionState::ReenrollmentRequired { .. } if !guard.degraded_notified => {
                    guard.degraded_notified = true;
                    Some(Toast::Credential)
                }
                ConnectionState::Disconnected { attempts, .. } if !guard.disconnected_notified => {
                    guard.disconnected_notified = true;
                    Some(Toast::Disconnected {
                        attempts: *attempts,
                    })
                }
                ConnectionState::Connected if guard.degraded_notified => {
                    guard.degraded_notified = false;
                    Some(Toast::Recovered)
                }
                _ => None,
            }
        };
        match toast {
            Some(Toast::Degraded {
                attempt,
                max_attempts,
            }) => crate::notify::notify_degraded(&match max_attempts {
                Some(max) => format!(
                    "The broker is unreachable (attempt {attempt} of {max}); the agent stops if it \
                     runs out."
                ),
                None => "The broker is unreachable.".to_string(),
            }),
            Some(Toast::Recovered) => crate::notify::notify_recovered(),
            Some(Toast::Disconnected { attempts }) => crate::notify::notify_disconnected(attempts),
            Some(Toast::Credential) => crate::notify::notify_degraded(
                "The agent credential was rejected; re-enrollment is required.",
            ),
            None => {}
        }
    }

    fn on_worker_event(&self, event: &WorkerEvent) {
        if matches!(event, WorkerEvent::EffectOutputChunk { .. }) {
            log_line(&self.shared, describe_worker_event(event));
            return;
        }
        let activity = match event {
            WorkerEvent::EffectStarted {
                provider, function, ..
            } => {
                crate::notify::notify_action_started(provider, function);
                format!("executing {provider}.{function}")
            }
            WorkerEvent::EffectFinished { .. } => "waiting for desktop work".to_string(),
            WorkerEvent::EffectSkippedDuplicate { .. } => "skipped duplicate delivery".to_string(),
            WorkerEvent::ControlReceived { kind, .. } => {
                format!("handling {} control", control_name(kind))
            }
            WorkerEvent::EffectOutputChunk { .. } => unreachable!(),
        };
        if let Ok(mut guard) = self.shared.lock() {
            set_activity(&mut guard.worker_activity, activity);
        }
        log_line(&self.shared, describe_worker_event(event));
    }
}
