//! Execution profile sources preserve the consuming run or adapter dispatch.
use super::*;
use runinator_models::execution_profiles::ExecutionProfile;
use uuid::Uuid;

struct Profiles {
    id: Uuid,
    consumer: Uuid,
    adapter: bool,
}
impl Profiles {
    fn disabled(&self) -> ExecutionProfile {
        serde_json::from_value(serde_json::json!({
            "id": self.id, "name": "profile", "collection": {"sources": []}, "exposure": {},
            "config_version": 1, "config_digest": "digest", "enabled": false,
            "health": "disabled", "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"
        })).unwrap()
    }
}
#[async_trait::async_trait]
impl ExecutionProfileSource for Profiles {
    async fn resolve_execution_profile_for_run(
        &self,
        name: &str,
        run: Uuid,
    ) -> runinator_api::Result<ExecutionProfile> {
        assert!(!self.adapter);
        assert_eq!(name, "profile");
        assert_eq!(run, self.consumer);
        Ok(self.disabled())
    }
    async fn fetch_execution_profile_for_run(
        &self,
        id: Uuid,
        run: Uuid,
    ) -> runinator_api::Result<ExecutionProfile> {
        assert!(!self.adapter);
        assert_eq!((id, run), (self.id, self.consumer));
        Ok(self.disabled())
    }
    async fn fetch_execution_profile_for_adapter_dispatch(
        &self,
        id: Uuid,
        dispatch: Uuid,
    ) -> runinator_api::Result<ExecutionProfile> {
        assert!(self.adapter);
        assert_eq!((id, dispatch), (self.id, self.consumer));
        Ok(self.disabled())
    }
    async fn download_execution_profile_for_run(
        &self,
        _: Uuid,
        _: i64,
        _: Uuid,
    ) -> runinator_api::Result<Vec<u8>> {
        panic!("disabled profile must not be downloaded")
    }
    async fn download_execution_profile_for_adapter_dispatch(
        &self,
        _: Uuid,
        _: i64,
        _: Uuid,
    ) -> runinator_api::Result<Vec<u8>> {
        panic!("disabled profile must not be downloaded")
    }
}

#[tokio::test]
async fn injected_profile_reads_keep_consumer_scope_and_reject_disabled_content() {
    for adapter in [false, true] {
        let source = Profiles {
            id: Uuid::new_v4(),
            consumer: Uuid::new_v4(),
            adapter,
        };
        let binding = ExecutionProfileBinding::resolved(source.id, "profile");
        let result = if adapter {
            materialize_for_adapter(&source, source.consumer, &binding).await
        } else {
            materialize(&source, Uuid::new_v4(), source.consumer, &binding).await
        };
        assert!(result.err().unwrap().to_string().contains("disabled"));
        if !adapter {
            let result = materialize(
                &source,
                Uuid::new_v4(),
                source.consumer,
                &ExecutionProfileBinding::unresolved("profile"),
            )
            .await;
            assert!(result.err().unwrap().to_string().contains("disabled"));
        }
    }
}
