use super::*;

#[test]
fn cli_resolves_runtime_counts_and_disables_optional_components() {
    let cli = Cli::try_parse_from([
        "runinator-standalone",
        "--workers",
        "3",
        "--engines",
        "2",
        "--wakers",
        "0",
        "--no-desktop-agent",
        "--no-default-pack",
        "start",
        "--foreground",
    ])
    .unwrap();

    let config = cli.resolved_config().unwrap();
    assert!(cli.command.runs_foreground());
    assert!(!cli.command.tui_requested());
    assert_eq!(config.workers, 3);
    assert_eq!(config.engines, 2);
    assert_eq!(config.wakers, 0);
    assert!(!config.desktop_agent);
    assert!(config.packs.is_empty());
}

#[test]
fn tui_keeps_start_and_restart_in_the_foreground() {
    for command in ["start", "restart"] {
        let cli = Cli::try_parse_from(["runinator-standalone", command, "--tui"]).unwrap();
        assert!(cli.command.runs_foreground());
        assert!(cli.command.tui_requested());
    }
}

#[test]
fn ordinary_start_remains_daemonized() {
    let cli = Cli::try_parse_from(["runinator-standalone", "start"]).unwrap();
    assert!(!cli.command.runs_foreground());
    assert!(!cli.command.tui_requested());
}

#[test]
fn relative_pack_and_ctl_paths_are_resolved_before_daemonization() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let cli = Cli::try_parse_from([
        "runinator-standalone",
        "--pack",
        workspace.join("packs/hello-world").to_str().unwrap(),
        "--ctl-path",
        workspace
            .join("target/debug/runinatorctl")
            .to_str()
            .unwrap(),
        "start",
    ])
    .unwrap();

    let config = cli.resolved_config().unwrap();
    assert!(config.state_dir.is_absolute());
    assert!(config.sqlite_path.is_absolute());
    assert!(config.packs[0].is_absolute());
    assert!(config.ctl_path.as_ref().unwrap().is_absolute());
}

#[test]
fn config_file_paths_are_relative_to_the_config_and_keep_spaces() {
    let root = std::env::temp_dir().join(format!(
        "runinator standalone paths {}",
        uuid::Uuid::now_v7()
    ));
    let pack = root.join("pack with spaces");
    std::fs::create_dir_all(&pack).unwrap();
    let config = StandaloneConfig {
        state_dir: "state with spaces".into(),
        database: "sqlite".into(),
        sqlite_path: "data/runinator db.sqlite".into(),
        database_url: None,
        workers: 1,
        engines: 1,
        wakers: 1,
        worker_concurrency: 1,
        engine_concurrency: 1,
        api_port: 8080,
        broker_port: 7070,
        blob_port: 9100,
        adapter_port: 8790,
        auth_enabled: false,
        desktop_agent: false,
        packs: vec!["pack with spaces".into()],
        ctl_path: Some("bin/runinator ctl".into()),
    };
    let path = root.join("standalone config.json");
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let cli = Cli::try_parse_from([
        "runinator-standalone",
        "--config-json",
        path.to_str().unwrap(),
        "serve",
    ])
    .unwrap();
    let resolved = cli.resolved_config().unwrap();
    assert_eq!(resolved.packs, vec![pack]);
    assert_eq!(resolved.state_dir, root.join("state with spaces"));
    assert_eq!(resolved.sqlite_path, root.join("data/runinator db.sqlite"));
    assert_eq!(resolved.ctl_path, Some(root.join("bin/runinator ctl")));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(not(windows))]
#[test]
fn windows_paths_explain_the_wsl_mapping() {
    let cli = Cli::try_parse_from([
        "runinator-standalone",
        "--pack",
        r"C:\\repo\\packs\\hello-world",
        "start",
    ])
    .unwrap();

    let error = cli.resolved_config().unwrap_err().to_string();
    assert!(error.contains("/mnt/<drive>/"));
}
