//! injected execution preserves profile isolation and maps lifecycle failures.
use super::*;
#[derive(Clone)]
struct Runner;
impl ProcessRunner for Runner {
    fn run(
        &self,
        request: ProcessRequest<'_>,
    ) -> Result<runinator_provider_support::process_runner::ProcessResult, ProcessFailure> {
        assert_eq!(request.command.get_program(), "gh");
        assert!(request.sink.is_none());
        let env: BTreeMap<_, _> = request.command.get_envs().collect();
        assert_eq!(env[std::ffi::OsStr::new("GH_TOKEN")], None);
        assert_eq!(env[std::ffi::OsStr::new("GITHUB_TOKEN")], None);
        assert_eq!(
            env[std::ffi::OsStr::new("HOME")],
            Some(std::ffi::OsStr::new("/profile/home"))
        );
        assert_eq!(request.timeout, Duration::from_secs(3));
        Err(ProcessFailure::TimedOut)
    }
}
#[test]
fn injectable_runner_keeps_credentials_private() {
    let request = ProviderExecutionRequest {
        run_id: None, action_name: "github_cli".into(), action_function: "run".into(),
        parameters: json!({"args":["pr","list"]}), timeout_secs: 3, artifact_dir: String::new(), events_jsonl_path: String::new(), idempotency_key: None, workspace_path: None,
        execution_profile: Some(serde_json::from_value(serde_json::json!({"profile_id":"00000000-0000-0000-0000-000000000001","revision":1,"root":"/profile","home":"/profile/home"})).unwrap()),
    };
    let error = GitHubCliProvider::with_runner(Runner)
        .execute_service(request, None, CancellationToken::new())
        .unwrap_err();
    assert!(error.to_string().contains(errors::COMMAND_TIMEOUT.code));
}
