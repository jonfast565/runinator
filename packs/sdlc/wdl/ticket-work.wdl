workflow "Ticket Work" v1 {
    params {
        ticket: JiraIssue
        parent_workflow_run_id: string
    }

    type JiraIssue = {
        key: string,
        fields: {
            summary: string,
            status: {
                name: string,
                ...: any
            },
            ...: any
        },
        ...: any
    }
    type PullRequestHead = {
        sha: string,
        ref: string,
        ...: any
    }
    type PullRequest = {
        number: integer,
        html_url: string,
        head: PullRequestHead,
        ...: any
    }
    type CheckSummary = {
        status: string,
        passed: integer,
        pending: integer,
        failed: integer,
        total: integer,
        raw: any
    }
    type GitWorktreeResult = {
        stdout: string,
        action: string,
        workspace: string
    }
    type GitCommandResult = {
        stdout: string,
        action: string
    }
    type AnyResponse = {
        response: any
    }
    type SlackMessage = {
        ok: boolean,
        channel: string,
        ts: string,
        message: map<any>
    }

    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }
    alias github_conn = { token: secret.github.token, owner: config.github.owner, repo: config.github.repo }
    alias slack_conn = { token: secret.slack.token, channel: config.slack.channel }

    set name = "Ticket Work: " ++ params.ticket.key
    set meta { parent_workflow_run_id: params.parent_workflow_run_id, ticket_key: params.ticket.key }

    let transition_in_progress: JiraIssue = jira.transition(
        ...jira_conn,
        key: params.ticket.key,
        transition_id: config.transitions.in_progress
    )
        .timeout(30s)
        fail -> notify_failure

    let kickoff_comment: JiraIssue = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Automation started for " ++ params.ticket.key ++ ". Run " ++ string(run.run_id)
    )
        .timeout(30s)

    let create_workspace: GitWorktreeResult = git.worktree(
        repo: config.git.repo,
        branch: "feature/" ++ params.ticket.key,
        path: config.git.repo ++ "/../runinator-worktrees/" ++ params.ticket.key
    )
        .timeout(120s)
        fail -> notify_failure

    let implement_change: AnyResponse = ai-command.claude_code(
        binary: config.claude.binary,
        model: config.claude.model,
        output_format: config.claude.output_format,
        allowed_tools: config.claude.allowed_tools,
        permission_mode: config.claude.permission_mode,
        extra_args: config.claude.extra_args,
        working_dir: create_workspace.workspace,
        prompt: config.claude.prompt_intro
            ++ "\n\nTicket: " ++ params.ticket.key
            ++ "\nSummary: " ++ params.ticket.fields.summary
            ++ "\n\nFull issue payload follows as JSON:\n" ++ json(params.ticket)
    )
        .timeout(1800s)
        fail -> notify_failure

    let commit_change: GitCommandResult = git.commit(
        workspace: create_workspace.workspace,
        message: params.ticket.key ++ " " ++ params.ticket.fields.summary
    )
        .timeout(60s)
        fail -> notify_failure

    let push_branch: GitCommandResult = git.push(
        workspace: create_workspace.workspace,
        remote: config.git.remote,
        branch: "feature/" ++ params.ticket.key
    )
        .timeout(60s)
        fail -> notify_failure

    let create_pr: PullRequest = github.create_pr(
        ...github_conn,
        base: config.github.base_branch,
        head: "feature/" ++ params.ticket.key,
        title: params.ticket.key ++ ": " ++ params.ticket.fields.summary,
        body: "Automated implementation for " ++ params.ticket.key ++ "."
    )
        .timeout(60s)
        fail -> notify_failure

    let link_pr_to_ticket: JiraIssue = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Pull request opened: " ++ create_pr.html_url
    )
        .timeout(30s)

    let transition_in_review: JiraIssue = jira.transition(
        ...jira_conn,
        key: params.ticket.key,
        transition_id: config.transitions.in_review
    )
        .timeout(30s)

    // poll ci on the configured interval until the checks settle, capped at 30 polls.
    until poll_checks.status == "passed" || poll_checks.status == "failed" limit 30 {
        wait config.ci_poll.interval_seconds
        let poll_checks: CheckSummary = github.checks_summary(
            ...github_conn,
            ref: create_pr.head.sha
        )
            .timeout(30s)
    }

    // passed -> ask for merge approval; anything else (failed or exhausted) -> notify failure.
    if poll_checks.status == "passed" {
        approve "Approve merge of automated PR for " ++ params.ticket.key type "merge"
            ok -> merge_pr
            reject -> comment_rejected
    } -> notify_failure

    let merge_pr: AnyResponse = github.merge_pr(
        ...github_conn,
        pull_number: string(create_pr.number),
        merge_method: "squash"
    )
        .timeout(60s)
        fail -> notify_failure

    let transition_done: JiraIssue = jira.transition(
        ...jira_conn,
        key: params.ticket.key,
        transition_id: config.transitions.done
    )
        .timeout(30s)

    let comment_merged: JiraIssue = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Merged " ++ create_pr.html_url
    )
        .timeout(30s)

    let notify_done: SlackMessage = slack.send_message(
        ...slack_conn,
        text: ":white_check_mark: " ++ params.ticket.key ++ " merged and closed."
    )
        .timeout(15s)
        -> cleanup_workspace

    let comment_rejected: JiraIssue = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Reviewer rejected automated merge. Manual follow-up required."
    )
        .timeout(30s)
        -> cleanup_workspace

    let notify_failure: SlackMessage = slack.send_message(
        ...slack_conn,
        text: ":x: SDLC pipeline failed on " ++ params.ticket.key
    )
        .timeout(15s)
        -> cleanup_workspace

    let cleanup_workspace: GitCommandResult = git.cleanup(
        repo: config.git.repo,
        path: create_workspace.workspace
    )
        .timeout(60s)
        -> done
}
