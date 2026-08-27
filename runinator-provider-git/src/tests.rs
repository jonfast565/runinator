use super::*;

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git command starts");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn test_git_provider_unsupported_action() {
    let provider = GitProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "git".into(),
        action_function: "invalid".into(),
        parameters: json!({}),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
    };

    let result = provider.execute_service(
        request,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    );
    assert!(result.is_err());
}

#[test]
fn metadata_includes_push_action() {
    let provider = GitProvider;
    let metadata = provider.metadata();

    let push = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "push")
        .expect("push action is advertised");

    assert!(
        push.parameters
            .iter()
            .any(|parameter| parameter.name == "branch" && parameter.required)
    );
}

#[test]
fn push_requires_branch_before_execution() {
    let provider = GitProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "git".into(),
        action_function: "push".into(),
        parameters: json!({
            "workspace": "."
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
    };

    let result = provider.execute_service(
        request,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    );
    assert!(result.is_err());
}

#[test]
fn metadata_advertises_safe_orchestration_actions() {
    use runinator_models::orchestration::DeliverySemantics;

    let metadata = GitProvider.metadata();
    for (name, semantics) in [
        ("attempt_worktree", DeliverySemantics::Reconcilable),
        ("capture_revision", DeliverySemantics::Idempotent),
        ("archive_patch", DeliverySemantics::Idempotent),
        ("promote_revision", DeliverySemantics::Reconcilable),
        ("cleanup", DeliverySemantics::Reconcilable),
    ] {
        let action = metadata
            .actions
            .iter()
            .find(|action| action.function_name == name)
            .unwrap_or_else(|| panic!("{name} action is advertised"));
        assert_eq!(action.delivery_semantics, semantics);
    }
}

#[test]
fn artifact_names_cannot_escape_the_artifact_directory() {
    assert_eq!(
        super::provider::sanitize_artifact_name("../../candidate.patch"),
        ".._.._candidate.patch"
    );
    assert_eq!(
        super::provider::sanitize_artifact_name(""),
        "candidate.patch"
    );
}

#[test]
fn guarded_promotion_is_atomic_and_reconcilable() {
    let repo =
        std::env::temp_dir().join(format!("runinator-git-provider-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&repo).expect("temp repo directory");
    git(&repo, &["init", "-b", "main"]);
    git(
        &repo,
        &["config", "user.email", "runinator@example.invalid"],
    );
    git(&repo, &["config", "user.name", "Runinator Test"]);
    std::fs::write(repo.join("file.txt"), "initial\n").expect("initial file");
    git(&repo, &["add", "file.txt"]);
    git(&repo, &["commit", "-m", "initial"]);
    let initial = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["branch", "ticket", &initial]);
    git(&repo, &["checkout", "-b", "candidate"]);
    std::fs::write(repo.join("file.txt"), "candidate\n").expect("candidate file");
    git(&repo, &["commit", "-am", "candidate"]);
    let candidate = git(&repo, &["rev-parse", "HEAD"]);

    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "git".into(),
        action_function: "promote_revision".into(),
        parameters: json!({
            "candidate_sha": candidate,
            "target_ref": "refs/heads/ticket",
            "expected_target_sha": initial,
            "push": false
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: Some("promote-ticket".into()),
        workspace_path: Some(repo.to_string_lossy().into_owned()),
    };
    let provider = GitProvider;
    for _ in 0..2 {
        provider
            .execute_service(
                request.clone(),
                None,
                runinator_plugin::cancel::CancellationToken::new(),
            )
            .expect("promotion and its replay both reconcile");
    }
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/ticket"]), candidate);
    std::fs::remove_dir_all(repo).expect("remove temp repo");
}
