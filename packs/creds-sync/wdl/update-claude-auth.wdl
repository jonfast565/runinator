workflow "Update Claude Worker Auth" v1 {
    // hourly: copy the operator's local Claude Code login into the k8s `claude-credentials` Secret
    // the worker pods mount, so cloud workers act as the logged-in identity.
    trigger cron "0 * * * *"

    // serialize against "Update AWS Worker Auth": both jobs run scripts/sync-secrets.sh on the same
    // `creds-sync` runner and would otherwise race on shared build dirs (tools/keychain-export,
    // tools/runinator-secret-sync). the shared lock name makes the two workflows mutually exclusive;
    // it is held until this run ends (or until this timeout lapses, so a wedged holder can never
    // deadlock the lock). the timeout exceeds the node timeout below so a waiter never gives up while
    // the holder is legitimately running.
    mutex "creds-sync" every 10s timeout 600s

    // `.runner("creds-sync")` pins this to the workstation worker that holds the local login and a
    // kubeconfig. if no such worker is connected, the node parks then fails on the timeout below.
    // `interactive: true` runs the sync attached to the operator's desktop session so `keychain-export`
    // can satisfy the macOS Keychain access dialog instead of failing headless. the command is
    // relative: it resolves against the console worker's working directory (the desktop agent's
    // "Working directory" / `RUNINATOR_CONSOLE_WORKING_DIR`), which must be a checkout of this repo.
    node sync_claude <- console.run(
        command: "bash scripts/sync-secrets.sh "
            ++ "--config tools/runinator-secret-sync/secret-sync.claude.json --once",
        interactive: true
    )
        .runner("creds-sync")
        .timeout(300s)
        fail -> fail
}
