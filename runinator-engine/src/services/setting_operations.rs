//! Shared organization-aware lifecycle for config and secret settings.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use runinator_models::{
    bundles::{SettingBundleEntry, SettingsBundle},
    errors::SendableError,
    settings::{SettingBinding, SettingKind, SettingRecord},
    value::Value,
};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, SettingStore},
};
use uuid::Uuid;

pub struct SettingOperations<T> {
    store: Arc<T>,
}

/// Complete input for one config/secret lifecycle operation.
pub struct SettingConfiguration {
    pub org_id: Option<Uuid>,
    pub kind: SettingKind,
    pub scope: String,
    pub name: String,
    pub value: Value,
    pub schema: Option<Value>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl<T> SettingOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: SettingStore + RuntimeStore> SettingOperations<T> {
    pub async fn configure(
        &self,
        configuration: SettingConfiguration,
    ) -> Result<SettingRecord, SendableError> {
        let SettingConfiguration {
            org_id,
            kind,
            scope,
            name,
            value,
            schema,
            expires_at,
        } = configuration;
        let scope = scope.trim().to_string();
        let name = name.trim().to_string();
        let cipher = SecretCipher::from_env();
        let stored = self
            .store
            .fetch_setting(org_id, kind, scope.clone(), name.clone())
            .await?;
        let stored_schema = stored
            .as_ref()
            .and_then(|record| cipher.try_decrypt(&record.value))
            .and_then(|bytes| crate::settings::decode_config_schema(&bytes));
        let bytes = crate::settings::validate_and_encode_with_expiry(
            kind,
            &scope,
            &name,
            &value,
            schema.as_ref(),
            stored_schema.as_ref(),
            expires_at,
        )
        .map_err(|message| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                message,
            )) as SendableError
        })?;
        self.store
            .upsert_setting(
                org_id,
                kind,
                scope.clone(),
                name.clone(),
                cipher.encrypt(&bytes),
                Utc::now().timestamp(),
            )
            .await?;
        self.store
            .fetch_setting(org_id, kind, scope, name)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other("setting disappeared after save")) as SendableError
            })
    }

    pub async fn import(
        &self,
        org_id: Option<Uuid>,
        bundle: &SettingsBundle,
        overwrite: bool,
    ) -> Result<Vec<SettingBundleEntry>, crate::settings::SettingBundleImportError> {
        if bundle.version != 1 {
            return Err(crate::settings::SettingBundleImportError {
                bad_request: true,
                message: format!(
                    "unsupported settings bundle version {}; expected 1",
                    bundle.version
                ),
            });
        }
        crate::settings::import_setting_bundle_with(self.store.as_ref(), org_id, bundle, overwrite)
            .await
    }

    pub async fn move_setting(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<Option<SettingRecord>, SendableError> {
        self.store
            .move_setting(id, org_id, kind, scope.trim().into(), name.trim().into())
            .await
    }
}

impl<T: DefinitionStore + SettingStore + RuntimeStore> SettingOperations<T> {
    pub async fn delete(
        &self,
        org_id: Option<Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<Vec<String>, SendableError> {
        let Some(target) = self
            .store
            .fetch_setting(org_id, kind, scope.clone(), name.clone())
            .await?
        else {
            return Ok(Vec::new());
        };
        let inbound = self
            .store
            .fetch_workflows()
            .await?
            .into_iter()
            .filter(|workflow| workflow.org_id == org_id)
            .filter(|workflow| {
                let uuid_bound = workflow
                    .definition
                    .metadata
                    .pointer("/artifact_refs/settings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|value| {
                        serde_json::from_value::<SettingBinding>(value.clone().into())
                            .is_ok_and(|binding| binding.reference.id == target.id)
                    });
                if uuid_bound {
                    return true;
                }
                let mut paths = std::collections::BTreeSet::new();
                if let Ok(graph) = serde_json::to_value(&workflow.definition) {
                    crate::repository::collect_setting_paths(&graph, &mut paths);
                }
                paths.contains(&(kind, scope.clone(), name.clone()))
            })
            .map(|workflow| workflow.artifact_path().qualified())
            .collect::<Vec<_>>();
        if inbound.is_empty() {
            self.store.delete_setting(org_id, kind, scope, name).await?;
        }
        Ok(inbound)
    }
}
