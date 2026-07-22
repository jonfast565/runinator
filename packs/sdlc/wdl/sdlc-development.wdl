workflow "SDLC: Development" v1 {
    // pipeline head: scan Ready for Development on a schedule (the chained phases also carry their
    // own cron backstop). one pass admits up to the in-flight budget, implements each ticket, opens a
    // PR, and moves it to In Review — the inbox the Review phase scans next.
    trigger cron "0 * * * *"

    // cooldown: this phase admits and implements tickets (expensive Claude runs), so guard against a
    // duplicate/manual re-fire within 5 minutes double-spending the budget. the hourly cron is the
    // real cadence; the mutex below still serializes any concurrent overlap.
    cooldown "sdlc-development" every 300s

    // serialize passes so an overlapping cron/chained fire never double-picks a ticket.
    mutex "sdlc-development" every 10s timeout 900s

    import std

    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }
    alias github_conn = { token: secret.github.token, owner: config.github.owner, repo: config.github.repo }
    alias claude_cfg = {
        binary: config.claude.binary,
        model: config.claude.model,
        output_format: config.claude.output_format,
        allowed_tools: config.claude.allowed_tools,
        permission_mode: config.claude.permission_mode,
        extra_args: config.claude.extra_args
    }

    set name = "SDLC Development scan"

    // fresh work waiting in Ready for Development, and the work already in flight (for the cap).
    node tickets <- jira.search(...jira_conn, jql: config.jira.ready_jql)
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    node in_flight <- jira.search(...jira_conn, jql: config.jira.in_flight_jql)
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    node budget: integer <- compute {
        return sub(config.selection.max_in_flight, len(in_flight.issues))
    }

    // admit up to the budget; each admitted ticket transitions out of Ready for Development so the
    // next pass's in-flight count reflects it and the cap holds.
    for ticket in tickets.issues limit budget {
        node transition_in_progress <- jira.transition(
            ...jira_conn,
            key: ticket.key,
            transition_id: config.transitions.in_progress
        )
            .timeout(30s)

        node kickoff_comment <- jira.comment(
            ...jira_conn,
            key: ticket.key,
            body: "SDLC automation started development for " ++ ticket.key ++ "."
        )
            .timeout(30s)

        // deterministic worktree path so later phases re-attach to the same checkout on the sdlc
        // runner. `.runner("sdlc")` pins every worktree-touching node to that worker.
        node create_workspace <- git.worktree(
            repo: config.git.repo,
            branch: config.branch.prefix ++ ticket.key,
            path: config.git.worktree_root ++ "/" ++ ticket.key
        )
            .runner("sdlc")
            .timeout(120s)
            .retry(2, backoff: 5s, max: 30s, jitter: true, on: failure)

        // push the empty branch first so github integrations have a ref to attach to.
        node push_branch <- git.push(
            workspace: create_workspace.workspace,
            remote: config.git.remote,
            branch: config.branch.prefix ++ ticket.key
        )
            .runner("sdlc")
            .timeout(60s)
            .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)

        node ticket_comments <- jira.comments(
            ...jira_conn,
            key: ticket.key,
            download_dir: create_workspace.workspace ++ "/.jira-comments"
        )
            .timeout(120s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

        node implement_change <- ai-command.claude_code(
            ...claude_cfg,
            working_dir: create_workspace.workspace,
            prompt: config.claude.prompt_intro
                ++ "\n\nTicket: " ++ ticket.key
                ++ "\nSummary: " ++ ticket.fields.summary
                ++ "\n\nComment history (any referenced images are saved under .jira-comments/):\n"
                ++ ticket_comments.text
                ++ "\n\nFull issue payload follows as JSON:\n" ++ json(ticket)
        )
            .runner("sdlc")
            .timeout(1800s)

        node implement_diff <- git.diff(workspace: create_workspace.workspace)
            .runner("sdlc")
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

        // a development pass that produced changes advances to review; an empty pass leaves the
        // ticket In Progress for the next scan rather than opening an empty PR.
        if implement_diff.stdout.trim().len() > 0 {
            node commit_change <- git.commit(
                workspace: create_workspace.workspace,
                message: ticket.key ++ " " ++ ticket.fields.summary
            )
                .runner("sdlc")
                .timeout(60s)
            node push_work <- git.push(
                workspace: create_workspace.workspace,
                remote: config.git.remote,
                branch: config.branch.prefix ++ ticket.key
            )
                .runner("sdlc")
                .timeout(60s)
                .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
            node create_pr <- github.create_pr(
                ...github_conn,
                base: config.github.base_branch,
                head: config.branch.prefix ++ ticket.key,
                title: ticket.key ++ ": " ++ ticket.fields.summary,
                body: "Automated implementation for " ++ ticket.key ++ "."
            )
                .timeout(60s)
            node link_pr <- jira.comment(
                ...jira_conn,
                key: ticket.key,
                body: "Pull request opened: " ++ create_pr.html_url
            )
                .timeout(30s)
            node transition_in_review <- jira.transition(
                ...jira_conn,
                key: ticket.key,
                transition_id: config.transitions.in_review
            )
                .timeout(30s)
        }
    }
}
