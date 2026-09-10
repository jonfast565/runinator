//! Scoped file materialization with injected bytes.
use super::*;

struct Files {
    file: Uuid,
    run: Uuid,
}
#[async_trait::async_trait]
impl RunFileSource for Files {
    async fn download_workflow_file_for_run(
        &self,
        file: Uuid,
        run: Uuid,
    ) -> runinator_api::Result<Vec<u8>> {
        assert_eq!((file, run), (self.file, self.run));
        Ok(b"input".to_vec())
    }
}

#[tokio::test]
async fn materialization_checks_digest_before_writing_injected_content() {
    let root = tempfile::tempdir().unwrap();
    let files = Files {
        file: Uuid::new_v4(),
        run: Uuid::new_v4(),
    };
    let mut descriptor = FileDescriptor {
        id: files.file,
        name: "input.txt".into(),
        path: "nested/input.txt".into(),
        mime_type: "text/plain".into(),
        size_bytes: 5,
        sha256: "bad".into(),
    };
    assert!(
        materialize_file(&files, root.path(), files.run, &descriptor)
            .await
            .is_err()
    );
    assert!(!root.path().join(&descriptor.path).exists());
    descriptor.sha256 = format!("{:x}", Sha256::digest(b"input"));
    materialize_file(&files, root.path(), files.run, &descriptor)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(root.path().join(&descriptor.path))
            .await
            .unwrap(),
        b"input"
    );
}
