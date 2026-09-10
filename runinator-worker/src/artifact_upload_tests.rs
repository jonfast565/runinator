//! Artifact relocation through an injected content uploader.
use super::*;
use async_trait::async_trait;
use runinator_api::{ApiError, ArtifactContentResponse};
use std::sync::Mutex;
use uuid::Uuid;

struct Upload {
    run: Uuid,
    fail: bool,
    bytes: Arc<Mutex<Vec<u8>>>,
}

#[async_trait]
impl ArtifactContentUploader for Upload {
    async fn upload_artifact_content(
        &self,
        run: Uuid,
        name: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> runinator_api::Result<ArtifactContentResponse> {
        assert_eq!(run, self.run);
        assert_eq!(name, "report");
        assert_eq!(mime, "text/plain");
        *self.bytes.lock().unwrap() = bytes;
        if self.fail {
            return Err(ApiError::UnexpectedResponse("upload unavailable".into()));
        }
        Ok(ArtifactContentResponse {
            uri: "blob://report".into(),
            size_bytes: 5,
            sha256: "digest".into(),
        })
    }
}

#[tokio::test]
async fn relocation_updates_only_after_a_successful_scoped_upload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report");
    tokio::fs::write(&path, b"hello").await.unwrap();
    let run = Uuid::new_v4();
    for fail in [true, false] {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let uploader = ArtifactUploader::new(Upload {
            run,
            fail,
            bytes: bytes.clone(),
        });
        let mut artifact = NewRunArtifact {
            name: "report".into(),
            mime_type: "text/plain".into(),
            size_bytes: 999,
            uri: path.to_string_lossy().into_owned(),
            metadata: Default::default(),
        };
        uploader
            .relocate_for_execution(run, Uuid::new_v4(), &mut artifact)
            .await;
        assert_eq!(*bytes.lock().unwrap(), b"hello");
        if fail {
            assert_eq!(artifact.uri, path.to_string_lossy());
            assert_eq!(artifact.size_bytes, 999);
        } else {
            assert_eq!(artifact.uri, "blob://report");
            assert_eq!(artifact.size_bytes, 5);
        }
    }
}
