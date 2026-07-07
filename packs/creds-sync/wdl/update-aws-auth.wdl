workflow "Update AWS Worker Auth" v1 {
    // hourly: refresh the operator's local AWS SSO cache and copy it into the k8s `aws-sso-cache`
    // Secret the worker pods mount, so cloud workers use the SSO identity. the sync spec is the
    // single point that grows to carry API-key credential files in the future.
    trigger cron "0 * * * *"

    // serialize against "Update Claude Worker Auth": both jobs run scripts/sync-secrets.sh on the
    // same `creds-sync` runner and would otherwise race on shared build dirs (tools/keychain-export,
    // tools/runinator-secret-sync). the shared lock name makes the two workflows mutually exclusive;
    // it is held until this run ends. the timeout exceeds the node timeout below so a waiter never
    // gives up while the holder is legitimately running.
    mutex "creds-sync" every 10s timeout 600s

    // `.runner("creds-sync")` pins this to the workstation worker that holds the local login and a
    // kubeconfig. if no such worker is connected, the node parks then fails on the timeout below.
    node sync_aws <- console.run(
        command: "bash " ++ config.creds_sync.workspace
            ++ "/scripts/sync-secrets.sh --config "
            ++ config.creds_sync.workspace
            ++ "/tools/runinator-secret-sync/secret-sync.aws.json --once"
    )
        .runner("creds-sync")
        .timeout(300s)
        fail -> fail
}
