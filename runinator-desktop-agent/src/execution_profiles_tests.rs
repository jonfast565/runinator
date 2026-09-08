//! execution-profile collection, approval visibility, and lifecycle coverage.

use super::*;
use runinator_models::execution_profiles::{
    ExecutionProfileCollectionSpec, ExecutionProfileExposureSpec, ExecutionProfileHealth,
};

#[test]
fn collection_maps_files_and_directories_into_one_deterministic_archive() {
    let root =
        std::env::temp_dir().join(format!("runinator-profile-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("cache/nested")).unwrap();
    fs::write(root.join("config"), b"profile config").unwrap();
    fs::write(root.join("cache/token.json"), b"token").unwrap();
    fs::write(root.join("cache/nested/ignored.txt"), b"ignored").unwrap();
    let profile = ExecutionProfile {
        id: uuid::Uuid::new_v4(),
        org_id: None,
        name: "fixture".into(),
        description: String::new(),
        credential_scopes: vec!["fixture".into()],
        collection: ExecutionProfileCollectionSpec {
            version: 1,
            probe: None,
            refresh: None,
            sources: vec![
                ExecutionProfileSource::File {
                    path: root.join("config").to_string_lossy().into_owned(),
                    target: ".tool/config".into(),
                },
                ExecutionProfileSource::Directory {
                    path: root.join("cache").to_string_lossy().into_owned(),
                    glob: "*.json".into(),
                    target: ".tool/cache".into(),
                },
            ],
        },
        exposure: ExecutionProfileExposureSpec::default(),
        config_version: 1,
        config_digest: "config-digest".into(),
        enabled: true,
        current_revision: None,
        current_digest: None,
        current_publisher_id: None,
        published_at: None,
        expires_at: None,
        refresh_requested_at: None,
        health: ExecutionProfileHealth::Unpublished,
        last_error: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let (_, first, first_digest) = collect(&profile, false, false).unwrap();
    let (_, second, second_digest) = collect(&profile, false, false).unwrap();
    assert_eq!(first, second);
    assert_eq!(first_digest, second_digest);
    let archive = zip::ZipArchive::new(Cursor::new(first)).unwrap();
    let names = archive.file_names().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            ".runinator-profile.json",
            ".tool/cache/token.json",
            ".tool/config"
        ]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dry_run_does_not_refresh_after_a_failed_probe() {
    let profile = ExecutionProfile {
        id: uuid::Uuid::new_v4(),
        org_id: None,
        name: "dry-run".into(),
        description: String::new(),
        credential_scopes: vec!["fixture".into()],
        collection: ExecutionProfileCollectionSpec {
            version: 1,
            probe: Some(ExecutionProfileCommand {
                argv: vec!["false".into()],
                interactive: false,
            }),
            refresh: Some(ExecutionProfileCommand {
                argv: vec!["true".into()],
                interactive: false,
            }),
            sources: vec![ExecutionProfileSource::File {
                path: "/dev/null".into(),
                target: ".tool/config".into(),
            }],
        },
        exposure: ExecutionProfileExposureSpec::default(),
        config_version: 1,
        config_digest: "config-digest".into(),
        enabled: true,
        current_revision: None,
        current_digest: None,
        current_publisher_id: None,
        published_at: None,
        expires_at: None,
        refresh_requested_at: None,
        health: ExecutionProfileHealth::Testing,
        last_error: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let error = collect(&profile, false, true).unwrap_err();
    assert!(error.to_string().contains("profile probe command 'false'"));
    assert!(error.to_string().contains("during dry run"));
}

#[test]
fn missing_file_source_names_the_source_and_target() {
    let missing = std::env::temp_dir().join(format!(
        "runinator-profile-missing-{}",
        uuid::Uuid::new_v4()
    ));
    let profile = ExecutionProfile {
        id: uuid::Uuid::new_v4(),
        org_id: None,
        name: "missing-file".into(),
        description: String::new(),
        credential_scopes: vec!["fixture".into()],
        collection: ExecutionProfileCollectionSpec {
            version: 1,
            probe: None,
            refresh: None,
            sources: vec![ExecutionProfileSource::File {
                path: missing.to_string_lossy().into_owned(),
                target: ".tool/config".into(),
            }],
        },
        exposure: ExecutionProfileExposureSpec::default(),
        config_version: 1,
        config_digest: "config-digest".into(),
        enabled: true,
        current_revision: None,
        current_digest: None,
        current_publisher_id: None,
        published_at: None,
        expires_at: None,
        refresh_requested_at: None,
        health: ExecutionProfileHealth::Testing,
        last_error: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let error = collect(&profile, false, true).unwrap_err().to_string();
    assert!(error.contains("profile file source"));
    assert!(error.contains(missing.to_string_lossy().as_ref()));
    assert!(error.contains("target '.tool/config'"));
}

#[test]
fn missing_directory_source_names_the_source_and_target() {
    let missing = std::env::temp_dir().join(format!(
        "runinator-profile-directory-missing-{}",
        uuid::Uuid::new_v4()
    ));
    let error = collect_directory(
        &mut BTreeMap::new(),
        &missing,
        ".tool/cache",
        &Pattern::new("*.json").unwrap(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("profile directory source"));
    assert!(error.contains(missing.to_string_lossy().as_ref()));
    assert!(error.contains("target '.tool/cache'"));
}

#[test]
fn status_error_is_bounded_for_the_profile_status_api() {
    let detail = status_error("desktop collection dry run failed", &"x".repeat(600));

    assert_eq!(detail.chars().count(), MAX_STATUS_ERROR_CHARS);
    assert!(detail.ends_with('…'));
}

#[test]
fn collection_waits_for_a_running_connected_agent() {
    let mut status = runinator_worker::AgentStatus {
        connection: ConnectionState::Connected,
        ..Default::default()
    };
    assert!(!collection_can_run(&status));

    status.running = true;
    assert!(collection_can_run(&status));

    status.connection = ConnectionState::Connecting;
    assert!(!collection_can_run(&status));
}

#[test]
fn bundled_keychain_export_uses_the_app_resources_directory() {
    let root = std::env::temp_dir().join(format!(
        "runinator-keychain-export-test-{}",
        uuid::Uuid::new_v4()
    ));
    let executable =
        root.join("Runinator Desktop Agent.app/Contents/MacOS/runinator-desktop-agent");
    let helper = root.join("Runinator Desktop Agent.app/Contents/Resources/keychain-export");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::create_dir_all(helper.parent().unwrap()).unwrap();
    fs::write(&executable, []).unwrap();
    fs::write(&helper, []).unwrap();

    assert_eq!(bundled_keychain_export(&executable), Some(helper));

    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn cargo_build_stages_keychain_export() {
    assert!(cargo_built_keychain_export().is_some());
}

fn collection_fixture(sources: Vec<ExecutionProfileSource>) -> ExecutionProfile {
    ExecutionProfile {
        id: uuid::Uuid::new_v4(),
        org_id: None,
        name: "fixture".into(),
        description: String::new(),
        credential_scopes: vec!["fixture".into()],
        collection: ExecutionProfileCollectionSpec {
            version: 1,
            probe: None,
            refresh: None,
            sources,
        },
        exposure: ExecutionProfileExposureSpec::default(),
        config_version: 1,
        config_digest: "config-digest".into(),
        enabled: true,
        current_revision: None,
        current_digest: None,
        current_publisher_id: None,
        published_at: None,
        expires_at: None,
        refresh_requested_at: None,
        health: ExecutionProfileHealth::Unpublished,
        last_error: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[test]
fn explicit_refresh_recollects_sources_without_a_refresh_command() {
    let path = std::env::temp_dir().join(format!("profile-refresh-{}", uuid::Uuid::new_v4()));
    fs::write(&path, "old credentials").unwrap();
    let profile = collection_fixture(vec![ExecutionProfileSource::File {
        path: path.to_string_lossy().into_owned(),
        target: "credentials".into(),
    }]);
    let (_, _, original_digest) = collect(&profile, false, false).unwrap();
    fs::write(&path, "new credentials").unwrap();
    let (_, bytes, refreshed_digest) = collect(&profile, true, false).unwrap();
    assert_ne!(original_digest, refreshed_digest);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut contents = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("credentials").unwrap(), &mut contents)
        .unwrap();
    assert_eq!(contents, "new credentials");
    fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn explicit_refresh_runs_command_sources_and_probes_without_a_refresh_command() {
    let mut profile = collection_fixture(vec![ExecutionProfileSource::Command {
        command: ExecutionProfileCommand {
            argv: vec!["printf".into(), "collected".into()],
            interactive: false,
        },
        target: "credentials".into(),
    }]);
    profile.collection.probe = Some(ExecutionProfileCommand {
        argv: vec!["true".into()],
        interactive: false,
    });
    let (_, bytes, _) = collect(&profile, true, false).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut contents = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("credentials").unwrap(), &mut contents)
        .unwrap();
    assert_eq!(contents, "collected");
    profile.collection.probe.as_mut().unwrap().argv = vec!["false".into()];
    assert!(
        collect(&profile, true, false)
            .unwrap_err()
            .to_string()
            .contains("profile probe command 'false'")
    );
}

#[cfg(unix)]
#[test]
fn configured_refresh_failure_is_preserved_but_dry_runs_never_execute_it() {
    let mut profile = collection_fixture(vec![]);
    profile.collection.refresh = Some(ExecutionProfileCommand {
        argv: vec!["false".into()],
        interactive: false,
    });
    assert!(
        collect(&profile, true, false)
            .unwrap_err()
            .to_string()
            .contains("profile refresh command 'false'")
    );
    assert!(collect(&profile, true, true).is_ok());
}

#[test]
fn approval_notices_latch_until_the_digest_or_approval_changes() {
    let status = LocalProfileStatus {
        id: uuid::Uuid::new_v4(),
        name: "fixture".into(),
        config_digest: "v1".into(),
        enabled: true,
        approved: false,
        message: String::new(),
    };
    assert_eq!(
        newly_required_approvals(&[], std::slice::from_ref(&status)).len(),
        1
    );
    assert!(
        newly_required_approvals(std::slice::from_ref(&status), std::slice::from_ref(&status))
            .is_empty()
    );
    let mut next = status.clone();
    next.config_digest = "v2".into();
    assert_eq!(
        newly_required_approvals(std::slice::from_ref(&status), std::slice::from_ref(&next)).len(),
        1
    );
    next.approved = true;
    assert!(newly_required_approvals(&[], std::slice::from_ref(&next)).is_empty());
    next.enabled = false;
    next.approved = false;
    assert!(newly_required_approvals(&[], std::slice::from_ref(&next)).is_empty());
}
