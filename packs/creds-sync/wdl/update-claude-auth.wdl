workflow "Update Claude Worker Auth" v1 {
    // hourly: copy the operator's local Claude Code login into the k8s `claude-credentials` Secret
    // the worker pods mount, so cloud workers act as the logged-in identity.
    trigger cron "0 * * * *"

    // `.runner("creds-sync")` pins this to the workstation worker that holds the local login and a
    // kubeconfig. if no such worker is connected, the node parks then fails on the timeout below.
    node sync_claude <- console.run(
        command: "bash " ++ config.creds_sync.workspace
            ++ "/scripts/sync-secrets.sh --config "
            ++ config.creds_sync.workspace
            ++ "/tools/runinator-secret-sync/secret-sync.claude.json --once"
    )
        .runner("creds-sync")
        .timeout(300s)
        fail -> fail
}
