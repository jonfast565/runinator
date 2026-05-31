workflow "Ticket Work" v1 {
    input {
        jira: { base_url: string, email: string, token: string, jql: string }
        transitions: { in_progress: string, in_review: string, done: string }
        github: { token: string, owner: string, repo: string, base_branch: string }
        slack: { token: string, channel: string }
        git: { repo: string, remote: string }
        claude: {
            binary: string,
            model: string,
            prompt_intro: string,
            allowed_tools: string,
            output_format: string,
            permission_mode: string,
            extra_args: string[]
        }
        ci_poll: { interval_seconds: integer, max_polls: integer }
        ticket: { key: string, fields: { summary: string } }
        parent_workflow_run_id: integer
    }

    set name = "Ticket Work: " ++ input.ticket.key
    set meta { parent_workflow_run_id: input.parent_workflow_run_id, ticket_key: input.ticket.key }

    let transition_in_progress = jira.transition(
        base_url: input.jira.base_url,
        email: input.jira.email,
        token: input.jira.token,
        key: input.ticket.key,
        transition_id: input.transitions.in_progress
    ).timeout(30s)
        fail -> notify_failure

    let kickoff_comment = jira.comment(
        base_url: input.jira.base_url,
        email: input.jira.email,
        token: input.jira.token,
        key: input.ticket.key,
        body: "Automation started for " ++ input.ticket.key ++ ". Run " ++ string(run.run_id)
    ).timeout(30s)

    let create_workspace = git.worktree(
        repo: input.git.repo,
        branch: "feature/" ++ input.ticket.key,
        path: input.git.repo ++ "/../runinator-worktrees/" ++ input.ticket.key
    ).timeout(120s)
        fail -> notify_failure

    let implement_change = ai-command.claude_code(
        binary: input.claude.binary,
        model: input.claude.model,
        output_format: input.claude.output_format,
        allowed_tools: input.claude.allowed_tools,
        permission_mode: input.claude.permission_mode,
        extra_args: input.claude.extra_args,
        working_dir: create_workspace.workspace,
        prompt: input.claude.prompt_intro
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
        remote: input.git.remote,
        branch: "feature/" ++ input.ticket.key
    ).timeout(60s)
        fail -> notify_failure

    let create_pr = github.create_pr(
        token: input.github.token,
        owner: input.github.owner,
        repo: input.github.repo,
        base: input.github.base_branch,
        head: "feature/" ++ input.ticket.key,
        title: input.ticket.key ++ ": " ++ input.ticket.fields.summary,
        body: "Automated implementation for " ++ input.ticket.key ++ "."
    ).timeout(60s)
        fail -> notify_failure

    let link_pr_to_ticket = jira.comment(
        base_url: input.jira.base_url,
        email: input.jira.email,
        token: input.jira.token,
        key: input.ticket.key,
        body: "Pull request opened: " ++ create_pr.html_url
    ).timeout(30s)

    let transition_in_review = jira.transition(
        base_url: input.jira.base_url,
        email: input.jira.email,
        token: input.jira.token,
        key: input.ticket.key,
        transition_id: input.transitions.in_review
    ).timeout(30s)

    // poll CI on the configured interval until the checks settle, capped at 30 polls.
    until poll_checks.status == "passed" || poll_checks.status == "failed" limit 30 {
        wait input.ci_poll.interval_seconds
        let poll_checks = github.checks_summary(
            token: input.github.token,
            owner: input.github.owner,
            repo: input.github.repo,
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
        token: input.github.token,
        owner: input.github.owner,
        repo: input.github.repo,
        pull_number: string(create_pr.number),
        merge_method: "squash"
    ).timeout(60s)
        fail -> notify_failure

    let transition_done = jira.transition(
        base_url: input.jira.base_url,
        email: input.jira.email,
        token: input.jira.token,
        key: input.ticket.key,
        transition_id: input.transitions.done
    ).timeout(30s)

    let comment_merged = jira.comment(
        base_url: input.jira.base_url,
        email: input.jira.email,
        token: input.jira.token,
        key: input.ticket.key,
        body: "Merged " ++ create_pr.html_url
    ).timeout(30s)

    let notify_done = slack.send_message(
        token: input.slack.token,
        channel: input.slack.channel,
        text: ":white_check_mark: " ++ input.ticket.key ++ " merged and closed."
    ).timeout(15s)
        -> cleanup_workspace

    let comment_rejected = jira.comment(
        base_url: input.jira.base_url,
        email: input.jira.email,
        token: input.jira.token,
        key: input.ticket.key,
        body: "Reviewer rejected automated merge. Manual follow-up required."
    ).timeout(30s)
        -> cleanup_workspace

    let notify_failure = slack.send_message(
        token: input.slack.token,
        channel: input.slack.channel,
        text: ":x: SDLC pipeline failed on " ++ input.ticket.key
    ).timeout(15s)
        -> cleanup_workspace

    let cleanup_workspace = git.cleanup(
        repo: input.git.repo,
        path: create_workspace.workspace
    ).timeout(60s)
        -> done
}
