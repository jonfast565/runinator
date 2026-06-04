workflow "Ticket Work" v1 {
    // only the per-ticket payload is dynamic input; everything else is shared config.* and secret.*.
    input {
        ticket: { key: string, fields: { summary: string } }
        parent_workflow_run_id: integer
    }

    set name = "Ticket Work: " ++ input.ticket.key
    set meta { parent_workflow_run_id: input.parent_workflow_run_id, ticket_key: input.ticket.key }

    let transition_in_progress = jira.transition(
        base_url: config.jira.base_url,
        email: config.jira.email,
        token: secret.jira.token,
        key: input.ticket.key,
        transition_id: config.transitions.in_progress
    ).timeout(30s)
        fail -> notify_failure

    let kickoff_comment = jira.comment(
        base_url: config.jira.base_url,
        email: config.jira.email,
        token: secret.jira.token,
        key: input.ticket.key,
        body: "Automation started for " ++ input.ticket.key ++ ". Run " ++ string(run.run_id)
    ).timeout(30s)

    let create_workspace = git.worktree(
        repo: config.git.repo,
        branch: "feature/" ++ input.ticket.key,
        path: config.git.repo ++ "/../runinator-worktrees/" ++ input.ticket.key
    ).timeout(120s)
        fail -> notify_failure

    let implement_change = ai-command.claude_code(
        binary: config.claude.binary,
        model: config.claude.model,
        output_format: config.claude.output_format,
        allowed_tools: config.claude.allowed_tools,
        permission_mode: config.claude.permission_mode,
        extra_args: config.claude.extra_args,
        working_dir: create_workspace.workspace,
        prompt: config.claude.prompt_intro
            ++ "\n\nTicket: " ++ input.ticket.key
            ++ "\nSummary: " ++ input.ticket.fields.summary
            ++ "\n\nFull issue payload follows as JSON:\n" ++ json(input.ticket)
    ).timeout(1800s)
        fail -> notify_failure

    let commit_change = git.commit(
        workspace: create_workspace.workspace,
        message: input.ticket.key ++ " " ++ input.ticket.fields.summary
    ).timeout(60s)
        fail -> notify_failure

    let push_branch = git.push(
        workspace: create_workspace.workspace,
        remote: config.git.remote,
        branch: "feature/" ++ input.ticket.key
    ).timeout(60s)
        fail -> notify_failure

    let create_pr = github.create_pr(
        token: secret.github.token,
        owner: config.github.owner,
        repo: config.github.repo,
        base: config.github.base_branch,
        head: "feature/" ++ input.ticket.key,
        title: input.ticket.key ++ ": " ++ input.ticket.fields.summary,
        body: "Automated implementation for " ++ input.ticket.key ++ "."
    ).timeout(60s)
        fail -> notify_failure

    let link_pr_to_ticket = jira.comment(
        base_url: config.jira.base_url,
        email: config.jira.email,
        token: secret.jira.token,
        key: input.ticket.key,
        body: "Pull request opened: " ++ create_pr.html_url
    ).timeout(30s)

    let transition_in_review = jira.transition(
        base_url: config.jira.base_url,
        email: config.jira.email,
        token: secret.jira.token,
        key: input.ticket.key,
        transition_id: config.transitions.in_review
    ).timeout(30s)

    // poll CI on the configured interval until the checks settle, capped at 30 polls.
    until poll_checks.status == "passed" || poll_checks.status == "failed" limit 30 {
        wait config.ci_poll.interval_seconds
        let poll_checks = github.checks_summary(
            token: secret.github.token,
            owner: config.github.owner,
            repo: config.github.repo,
            ref: create_pr.head.sha
        ).timeout(30s)
    }

    // passed -> ask for merge approval; anything else (failed or exhausted) -> notify failure.
    if poll_checks.status == "passed" {
        approve "Approve merge of automated PR for " ++ input.ticket.key type "merge"
            ok -> merge_pr
            reject -> comment_rejected
    } -> notify_failure

    let merge_pr = github.merge_pr(
        token: secret.github.token,
        owner: config.github.owner,
        repo: config.github.repo,
        pull_number: string(create_pr.number),
        merge_method: "squash"
    ).timeout(60s)
        fail -> notify_failure

    let transition_done = jira.transition(
        base_url: config.jira.base_url,
        email: config.jira.email,
        token: secret.jira.token,
        key: input.ticket.key,
        transition_id: config.transitions.done
    ).timeout(30s)

    let comment_merged = jira.comment(
        base_url: config.jira.base_url,
        email: config.jira.email,
        token: secret.jira.token,
        key: input.ticket.key,
        body: "Merged " ++ create_pr.html_url
    ).timeout(30s)

    let notify_done = slack.send_message(
        token: secret.slack.token,
        channel: config.slack.channel,
        text: ":white_check_mark: " ++ input.ticket.key ++ " merged and closed."
    ).timeout(15s)
        -> cleanup_workspace

    let comment_rejected = jira.comment(
        base_url: config.jira.base_url,
        email: config.jira.email,
        token: secret.jira.token,
        key: input.ticket.key,
        body: "Reviewer rejected automated merge. Manual follow-up required."
    ).timeout(30s)
        -> cleanup_workspace

    let notify_failure = slack.send_message(
        token: secret.slack.token,
        channel: config.slack.channel,
        text: ":x: SDLC pipeline failed on " ++ input.ticket.key
    ).timeout(15s)
        -> cleanup_workspace

    let cleanup_workspace = git.cleanup(
        repo: config.git.repo,
        path: create_workspace.workspace
    ).timeout(60s)
        -> done
}
