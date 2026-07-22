workflow "SDLC: QA" v1 {
    // terminal phase: scan In Testing tickets (deployed, awaiting a QA outcome) and react to what a
    // human QA does. Done -> celebrate and clean up the worktree; back to Ready for Development ->
    // note it (the Development scanner re-picks it); any other terminal status -> note and clean up.
    // tickets still under test are left In Testing for the next pass.
    trigger cron "*/30 * * * *"

    // cooldown: collapse a near-simultaneous cron + chained fire into one pass per 5 minutes so a
    // burst never re-scans the inbox back-to-back. cron still drives the baseline cadence, and a
    // chained fire that lands after the window still runs (a passing scanner completes in seconds).
    cooldown "sdlc-qa" every 300s

    mutex "sdlc-qa" every 10s timeout 1800s

    import std

    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }
    alias slack_conn = { token: secret.slack.token, channel: config.slack.channel }

    set name = "SDLC QA scan"

    node tickets <- jira.search(...jira_conn, jql: config.jira.qa_jql)
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

    for ticket in tickets.issues limit config.selection.max_in_flight {
        // re-read ground truth for this ticket so we act on its current status, not the search snapshot.
        node qa <- jira.poll(...jira_conn, key: ticket.key)
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

        if qa.fields.status.name == config.status.done {
            node qa_done_note <- slack.send_message(
                ...slack_conn,
                text: ":white_check_mark: " ++ ticket.key ++ " passed QA and is Done."
            )
                .timeout(15s)
                .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
            node cleanup_done <- git.cleanup(
                repo: config.git.repo,
                path: config.git.worktree_root ++ "/" ++ ticket.key
            )
                .runner("sdlc")
                .timeout(60s)
                .retry(2, backoff: 5s, max: 30s, jitter: true, on: failure)
        }
        if qa.fields.status.name == config.status.ready_for_dev {
            node qa_bounce <- jira.comment(
                ...jira_conn,
                key: ticket.key,
                body: "QA returned " ++ ticket.key ++ " to Ready for Development; the Development scanner will re-pick it."
            )
                .timeout(30s)
        }
        if qa.fields.status.name in config.status.terminal {
            node qa_terminal_note <- jira.comment(
                ...jira_conn,
                key: ticket.key,
                body: "QA moved " ++ ticket.key ++ " to a terminal status ('" ++ qa.fields.status.name ++ "'); cleaning up."
            )
                .timeout(30s)
            node cleanup_terminal <- git.cleanup(
                repo: config.git.repo,
                path: config.git.worktree_root ++ "/" ++ ticket.key
            )
                .runner("sdlc")
                .timeout(60s)
                .retry(2, backoff: 5s, max: 30s, jitter: true, on: failure)
        }
    }
}
