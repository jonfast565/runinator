workflow "SDLC: Review" v1 {
    // scan In Review tickets the automation owns. each pass re-attaches the worktree, has claude
    // address open Copilot/human feedback, checks CI and approvals, and merges when ready — moving
    // the ticket to Ready for Testing (the Deploy phase's inbox). tickets not yet ready are left in
    // In Review for the next pass.
    trigger cron "*/30 * * * *"

    // cooldown: collapse a near-simultaneous cron + chained fire into one pass per 5 minutes so a
    // burst never re-scans the inbox back-to-back. cron still drives the baseline cadence, and a
    // chained fire that lands after the window still runs.
    cooldown "sdlc-review" every 300s

    mutex "sdlc-review" every 10s timeout 1800s

    import std

    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }
    alias github_conn = { token: secret.github.token, owner: config.github.owner, repo: config.github.repo }
    alias slack_conn = { token: secret.slack.token, channel: config.slack.channel }
    alias claude_cfg = {
        binary: config.claude.binary,
        model: config.claude.model,
        output_format: config.claude.output_format,
        allowed_tools: config.claude.allowed_tools,
        permission_mode: config.claude.permission_mode,
        extra_args: config.claude.extra_args
    }

    set name = "SDLC Review scan"

    node tickets <- jira.search(...jira_conn, jql: config.jira.in_review_jql)
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

    for ticket in tickets.issues limit config.selection.max_in_flight {
        // re-attach the shared worktree on the sdlc worker (create-or-attach from the remote branch).
        node workspace <- git.worktree(
            repo: config.git.repo,
            branch: config.branch.prefix ++ ticket.key,
            path: config.git.worktree_root ++ "/" ++ ticket.key
        )
            .runner("sdlc")
            .timeout(120s)
            .retry(2, backoff: 5s, max: 30s, jitter: true, on: failure)

        // create-or-update returns the existing open PR for this branch (no duplicate is opened).
        node pr <- github.create_pr(
            ...github_conn,
            base: config.github.base_branch,
            head: config.branch.prefix ++ ticket.key,
            title: ticket.key ++ ": " ++ ticket.fields.summary,
            body: "Automated implementation for " ++ ticket.key ++ "."
        )
            .timeout(60s)

        node add_reviewers <- github.request_reviewers(
            ...github_conn,
            pull_number: string(pr.number),
            reviewers: config.review.reviewers
        )
            .timeout(30s)
        node add_assignee <- github.add_assignees(
            ...github_conn,
            issue_number: string(pr.number),
            assignees: config.review.assignees
        )
            .timeout(30s)

        // one claude pass addresses whatever Copilot/review feedback is open on the PR; commit and
        // push any resulting changes so CI re-runs before the approval check below.
        node fix_feedback <- ai-command.claude_code(
            ...claude_cfg,
            working_dir: workspace.workspace,
            prompt: config.claude.review_fix_prompt
                ++ " PR #" ++ string(pr.number)
                ++ " in " ++ config.github.owner ++ "/" ++ config.github.repo
        )
            .runner("sdlc")
            .timeout(1800s)
        node fix_diff <- git.diff(workspace: workspace.workspace)
            .runner("sdlc")
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
        if fix_diff.stdout.trim().len() > 0 {
            node commit_fix <- git.commit(
                workspace: workspace.workspace,
                message: ticket.key ++ " address review feedback"
            )
                .runner("sdlc")
                .timeout(60s)
            node push_fix <- git.push(
                workspace: workspace.workspace,
                remote: config.git.remote,
                branch: config.branch.prefix ++ ticket.key
            )
                .runner("sdlc")
                .timeout(60s)
                .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
        }

        // gate a merge on green CI and enough approvals: two approvals, or one with no outstanding
        // change requests. anything short leaves the ticket In Review for the next pass.
        node ci <- github.checks_summary(...github_conn, ref: pr.head.sha)
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
        node reviews: any <- github.reviews(...github_conn, pull_number: string(pr.number))
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
        node review_state: { approved: integer, changes_requested: integer, two_plus: boolean, ready: boolean } <- compute {
            let approved = len(filter(reviews, r => r.state == "APPROVED"))
            let changes = len(filter(reviews, r => r.state == "CHANGES_REQUESTED"))
            return {
                approved: approved,
                changes_requested: changes,
                two_plus: gte(approved, 2),
                ready: or(gte(approved, 2), and(gte(approved, 1), eq(changes, 0)))
            }
        }

        if and(ci.status == "passed", review_state.ready) {
            node merge_pr <- github.merge_pr(
                ...github_conn,
                pull_number: string(pr.number),
                merge_method: "squash"
            )
                .timeout(60s)
            node transition_testing <- jira.transition(
                ...jira_conn,
                key: ticket.key,
                transition_id: config.transitions.ready_for_testing
            )
                .timeout(30s)
            node ready_comment <- jira.comment(
                ...jira_conn,
                key: ticket.key,
                body: "Merged " ++ pr.html_url ++ "; moving to Ready for Testing."
            )
                .timeout(30s)
            node notify_merged <- slack.send_message(
                ...slack_conn,
                text: ":twisted_rightwards_arrows: " ++ ticket.key ++ " merged and ready to deploy."
            )
                .timeout(15s)
                .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
        }
    }
}
