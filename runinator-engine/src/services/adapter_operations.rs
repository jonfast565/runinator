//! Application service for orchestration-adapter persistence and normalized ingress routing.
//!
//! Provider-host HTTP remains at the transport edge. Durable adapter revisions, secret lookup,
//! correlation aliases, and workflow/pipeline admission lookup live here so every transport sees
//! one storage contract.

use std::{collections::BTreeMap, sync::Arc};

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        AdapterDefinition, AdapterRevision, IngressAction, IngressAdmissionStatus,
        IngressLifecycle, IngressPolicy, IngressTargetKind, NormalizedAdapterEvent,
        OrchestrationPolicy,
    },
    settings::SettingKind,
};
use runinator_secrets::{secret_cipher::SecretCipher, stored_secret::StoredSecret};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, IngressStore, NewAdapterDefinition, NewAdapterRevision, OrchestrationStore,
    },
};
use uuid::Uuid;

use super::choose_intent;

#[derive(Clone)]
pub struct AdapterOperations<T> {
    store: Arc<T>,
}

impl<T> AdapterOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: OrchestrationStore> AdapterOperations<T> {
    pub async fn list(&self, org_id: Uuid) -> Result<Vec<AdapterDefinition>, SendableError> {
        self.store.fetch_orchestration_adapters(org_id).await
    }

    pub async fn fetch(&self, id: Uuid) -> Result<Option<AdapterDefinition>, SendableError> {
        self.store.fetch_orchestration_adapter(id).await
    }

    pub async fn fetch_by_endpoint(
        &self,
        endpoint: String,
    ) -> Result<Option<AdapterDefinition>, SendableError> {
        self.store
            .fetch_orchestration_adapter_by_endpoint(endpoint)
            .await
    }

    pub async fn revisions(&self, id: Uuid) -> Result<Vec<AdapterRevision>, SendableError> {
        self.store.fetch_orchestration_adapter_revisions(id).await
    }

    pub async fn current_revision(
        &self,
        adapter: &AdapterDefinition,
    ) -> Result<Option<AdapterRevision>, SendableError> {
        self.store
            .fetch_orchestration_adapter_revision(adapter.id, adapter.current_revision)
            .await
    }

    pub async fn create(
        &self,
        adapter: NewAdapterDefinition,
        now: DateTime<Utc>,
    ) -> Result<(AdapterDefinition, AdapterRevision), SendableError> {
        self.store.create_orchestration_adapter(adapter, now).await
    }

    pub async fn create_revision(
        &self,
        revision: NewAdapterRevision,
        now: DateTime<Utc>,
    ) -> Result<Option<(AdapterDefinition, AdapterRevision)>, SendableError> {
        self.store
            .create_orchestration_adapter_revision(revision, now)
            .await
    }

    pub async fn set_enabled(
        &self,
        id: Uuid,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<Option<AdapterDefinition>, SendableError> {
        self.store
            .set_orchestration_adapter_enabled(id, enabled, now)
            .await
    }

    pub async fn delete(&self, id: Uuid) -> Result<bool, SendableError> {
        self.store.delete_orchestration_adapter(id).await
    }
}

impl<T: RuntimeStore> AdapterOperations<T> {
    pub async fn resolve_secrets(
        &self,
        org_id: Uuid,
        bindings: &BTreeMap<String, Uuid>,
    ) -> Result<serde_json::Value, String> {
        let cipher = SecretCipher::from_env();
        let mut values = serde_json::Map::new();
        for (name, id) in bindings {
            let record = self
                .store
                .fetch_setting_by_id(*id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("secret binding '{name}' does not exist"))?;
            if record.kind != SettingKind::Secret {
                return Err(format!("binding '{name}' does not reference a Secret"));
            }
            if record.scope != format!("org:{org_id}") {
                return Err(format!(
                    "secret binding '{name}' is outside the adapter organization"
                ));
            }
            let opened = cipher
                .try_decrypt(&record.value)
                .ok_or_else(|| format!("secret binding '{name}' could not be decrypted"))?;
            let secret = StoredSecret::decode(&opened)?;
            if secret
                .expires_at
                .is_some_and(|expires| expires <= Utc::now())
            {
                return Err(format!("secret binding '{name}' is expired"));
            }
            values.insert(name.clone(), serde_json::Value::String(secret.value));
        }
        Ok(serde_json::Value::Object(values))
    }
}

impl<T: DefinitionStore + IngressStore> AdapterOperations<T> {
    pub async fn pipeline_for_event(
        &self,
        adapter: &AdapterDefinition,
        event: &NormalizedAdapterEvent,
    ) -> Result<Uuid, String> {
        if let Some(admission) = self
            .store
            .fetch_ingress_admission(
                Some(adapter.org_id),
                event.scope.clone(),
                event.correlation_key.clone(),
            )
            .await
            .map_err(|error| error.to_string())?
        {
            return match admission.target.kind {
                IngressTargetKind::Pipeline => Ok(admission.target.id),
                IngressTargetKind::Workflow => {
                    Err("correlation key is owned by a workflow ingress target".into())
                }
            };
        }
        let mut candidates = Vec::new();
        for pipeline in self
            .store
            .fetch_pipelines()
            .await
            .map_err(|error| error.to_string())?
        {
            if pipeline.org_id != Some(adapter.org_id) {
                continue;
            }
            let Some(raw_policy) = pipeline.metadata.get("ingress") else {
                continue;
            };
            let policy: IngressPolicy =
                serde_json::from_value(raw_policy.clone().into()).map_err(|error| {
                    format!(
                        "pipeline '{}' has invalid ingress policy: {error}",
                        pipeline.name
                    )
                })?;
            if policy.scope == event.scope
                && policy.action_for_payload(
                    &event.event_type,
                    IngressLifecycle::Unbound,
                    &event.payload,
                ) == Some(IngressAction::Start)
                && let Some(id) = pipeline.id
            {
                candidates.push(id);
            }
        }
        match candidates.as_slice() {
            [pipeline_id] => Ok(*pipeline_id),
            [] => Err(format!(
                "no pipeline admission route matched scope '{}' and event '{}'",
                event.scope, event.event_type
            )),
            _ => Err(format!(
                "multiple pipeline admission routes matched scope '{}' and event '{}'; make admission routes unambiguous",
                event.scope, event.event_type
            )),
        }
    }
}

impl<T: IngressStore + OrchestrationStore> AdapterOperations<T> {
    /// Direct admission identity always wins. Otherwise, route an alias learned from an earlier
    /// phase result to its binding generation's canonical ingress key while retaining provenance.
    pub async fn resolve_correlation_alias(
        &self,
        org_id: Uuid,
        event: &mut NormalizedAdapterEvent,
    ) -> Result<(), String> {
        if self
            .store
            .fetch_ingress_admission(
                Some(org_id),
                event.scope.clone(),
                event.correlation_key.clone(),
            )
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        let Some(alias) = self
            .store
            .fetch_orchestration_correlation_alias(
                Some(org_id),
                event.source.clone(),
                event.scope.clone(),
                event.correlation_key.clone(),
            )
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };
        let Some(binding) = self
            .store
            .fetch_orchestration_binding(alias.binding_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("correlation alias refers to a missing binding".into());
        };
        if binding.generation != alias.generation {
            return Err("correlation alias generation no longer matches its binding".into());
        }
        let received = serde_json::json!({
            "source": event.source,
            "scope": event.scope,
            "correlation_key": event.correlation_key,
            "alias_id": alias.id,
        });
        match &mut event.provenance {
            runinator_models::value::Value::Object(values) => {
                values.insert("received_correlation".into(), received.into());
            }
            prior => {
                *prior = serde_json::json!({
                    "received_correlation": received,
                    "adapter_provenance": prior.clone(),
                })
                .into();
            }
        }
        event.scope = binding.scope;
        event.correlation_key = binding.correlation_key;
        Ok(())
    }
}

impl<T: DefinitionStore + IngressStore + OrchestrationStore + RuntimeStore> AdapterOperations<T> {
    pub async fn preview_event(
        &self,
        adapter: &AdapterDefinition,
        event: &NormalizedAdapterEvent,
    ) -> Result<serde_json::Value, String> {
        let admission = self
            .store
            .fetch_ingress_admission(
                Some(adapter.org_id),
                event.scope.clone(),
                event.correlation_key.clone(),
            )
            .await
            .map_err(|error| error.to_string())?;
        let lifecycle = admission
            .as_ref()
            .map(|admission| match admission.status {
                IngressAdmissionStatus::Active => IngressLifecycle::Active,
                IngressAdmissionStatus::Terminal => IngressLifecycle::Terminal,
            })
            .unwrap_or(IngressLifecycle::Unbound);
        let mut validation_errors = Vec::new();
        let mut pipelines = if let Some(admission) = &admission {
            match admission.target.kind {
                IngressTargetKind::Pipeline => self
                    .store
                    .fetch_pipeline(admission.target.id)
                    .await
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .collect::<Vec<_>>(),
                IngressTargetKind::Workflow => {
                    validation_errors
                        .push("correlation key is owned by a workflow ingress target".to_string());
                    Vec::new()
                }
            }
        } else {
            self.store
                .fetch_pipelines()
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|pipeline| pipeline.org_id == Some(adapter.org_id))
                .collect::<Vec<_>>()
        };
        pipelines.sort_by(|left, right| left.name.cmp(&right.name));

        let binding = if let Some(admission) = &admission {
            match admission.id {
                Some(id) => self
                    .store
                    .fetch_orchestration_binding_for_admission(id, admission.generation)
                    .await
                    .map_err(|error| error.to_string())?,
                None => None,
            }
        } else {
            None
        };
        let mut matches = Vec::new();
        let mut start_matches = 0usize;
        for pipeline in pipelines {
            let pipeline_id = match pipeline.id {
                Some(id) => id,
                None => continue,
            };
            let ingress = if admission
                .as_ref()
                .is_some_and(|admission| admission.target.id == pipeline_id)
            {
                admission.as_ref().and_then(|admission| {
                    serde_json::from_value(admission.policy.clone().into()).ok()
                })
            } else {
                pipeline.metadata.get("ingress").and_then(|value| {
                    serde_json::from_value::<IngressPolicy>(value.clone().into()).ok()
                })
            };
            let Some(ingress) = ingress else {
                continue;
            };
            if ingress.scope != event.scope {
                continue;
            }
            let routes = ingress.routes_for_payload(&event.event_type, lifecycle, &event.payload);
            if routes.is_empty() {
                continue;
            }
            if routes
                .iter()
                .any(|route| route.action == IngressAction::Start)
            {
                start_matches += 1;
            }
            let candidate_intents = routes
                .iter()
                .filter(|route| route.action == IngressAction::Dispatch)
                .filter_map(|route| route.intent.clone())
                .collect::<Vec<_>>();
            let orchestration = binding
                .as_ref()
                .filter(|binding| binding.pipeline_id == pipeline_id)
                .map(|binding| binding.policy.clone())
                .or_else(|| {
                    pipeline.metadata.get("orchestration").and_then(|value| {
                        serde_json::from_value::<OrchestrationPolicy>(value.clone().into()).ok()
                    })
                });
            let decision = orchestration
                .as_ref()
                .map(|policy| choose_intent(candidate_intents.iter().map(String::as_str), policy));
            matches.push(serde_json::json!({
                "pipeline_id": pipeline_id,
                "pipeline_name": pipeline.name,
                "lifecycle": lifecycle.as_str(),
                "routes": routes,
                "candidate_intents": candidate_intents,
                "winner": decision.as_ref().and_then(|decision| decision.winner.clone()),
                "suppressed_intents": decision.map(|decision| decision.suppressed).unwrap_or_default(),
                "managed": orchestration.is_some(),
            }));
        }
        if matches.is_empty() {
            validation_errors.push(format!(
                "no pipeline route matched scope '{}' and event '{}' for lifecycle '{}'",
                event.scope,
                event.event_type,
                lifecycle.as_str()
            ));
        } else if lifecycle == IngressLifecycle::Unbound && start_matches == 0 {
            validation_errors.push("matching routes do not admit a new pipeline generation".into());
        } else if lifecycle == IngressLifecycle::Unbound && start_matches > 1 {
            validation_errors.push(
                "multiple pipeline admission routes matched; admission would be rejected as ambiguous"
                    .into(),
            );
        }
        Ok(serde_json::json!({
            "delivery_id": event.delivery_id,
            "scope": event.scope,
            "correlation_key": event.correlation_key,
            "event_type": event.event_type,
            "lifecycle": lifecycle.as_str(),
            "existing_admission": admission,
            "pipeline_matches": matches,
            "validation_errors": validation_errors,
        }))
    }
}
