workflow "Ticket Work" v1 {
    params {
        ticket: JiraIssue
        parent_workflow_run_id: string
    }

    import std

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

    set name = "Ticket Work: " ++ params.ticket.key
    set meta { parent_workflow_run_id: params.parent_workflow_run_id, ticket_key: params.ticket.key }

    // -- phase 1: development -------------------------------------------------

    // re-read ground truth: only proceed if the ticket is still Ready for
    // Development. anything else means a human moved it since the scan.
    node status_precheck = jira.poll(
        ...jira_conn,
        key: params.ticket.key
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
        fail -> handle_failure
    if status_precheck.fields.status.name != config.status.ready_for_dev {
        node precheck_note = jira.comment(
            ...jira_conn,
            key: params.ticket.key,
            body: "Automation expected Ready for Development but found '" ++ status_precheck.fields.status.name ++ "'. Stopping to respect the manual change."
        )
            .timeout(30s)
            -> handle_drift
    }

    node transition_in_progress = jira.transition(
        ...jira_conn,
        key: params.ticket.key,
        transition_id: config.transitions.in_progress
    )
        .timeout(30s)
        fail -> handle_failure

    node kickoff_comment = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Automation started for " ++ params.ticket.key ++ ". Run " ++ string(run.run_id)
    )
        .timeout(30s)

    node create_workspace = git.worktree(
        repo: config.git.repo,
        branch: config.branch.prefix ++ params.ticket.key,
        path: config.git.worktree_root ++ "/" ++ params.ticket.key
    )
        .timeout(120s)
        .retry(2, backoff: 5s, max: 30s, jitter: true, on: failure)
        fail -> handle_failure

    // push the empty branch first so github integrations have a ref to attach to.
    node push_branch = git.push(
        workspace: create_workspace.workspace,
        remote: config.git.remote,
        branch: config.branch.prefix ++ params.ticket.key
    )
        .timeout(60s)
        .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
        fail -> handle_failure

    // pull the full comment thread (rendered to text, with any images saved into the
    // worktree) so claude can read the history to determine the course of action.
    node ticket_comments = jira.comments(
        ...jira_conn,
        key: params.ticket.key,
        download_dir: create_workspace.workspace ++ "/.jira-comments"
    )
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

    node implement_change = ai-command.claude_code(
        ...claude_cfg,
        working_dir: create_workspace.workspace,
        prompt: config.claude.prompt_intro
            ++ "\n\nTicket: " ++ params.ticket.key
            ++ "\nSummary: " ++ params.ticket.fields.summary
            ++ "\n\nComment history (any referenced images are saved under .jira-comments/):\n"
            ++ ticket_comments.text
            ++ "\n\nFull issue payload follows as JSON:\n" ++ json(params.ticket)
    )
        .timeout(1800s)
        fail -> handle_failure

    // a development pass that produced no changes is a failure worth surfacing.
    node implement_diff = git.diff(
        workspace: create_workspace.workspace
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    if implement_diff.stdout.trim().len() == 0 {
        node no_change_note = jira.comment(
            ...jira_conn,
            key: params.ticket.key,
            body: "Automated development produced no changes; returning for another pass."
        )
            .timeout(30s)
            -> handle_failure
    }

    node commit_change = git.commit(
        workspace: create_workspace.workspace,
        message: params.ticket.key ++ " " ++ params.ticket.fields.summary
    )
        .timeout(60s)
        fail -> handle_failure

    node push_work = git.push(
        workspace: create_workspace.workspace,
        remote: config.git.remote,
        branch: config.branch.prefix ++ params.ticket.key
    )
        .timeout(60s)
        .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
        fail -> handle_failure

    node create_pr = github.create_pr(
        ...github_conn,
        base: config.github.base_branch,
        head: config.branch.prefix ++ params.ticket.key,
        title: params.ticket.key ++ ": " ++ params.ticket.fields.summary,
        body: "Automated implementation for " ++ params.ticket.key ++ "."
    )
        .timeout(60s)
        fail -> handle_failure

    node link_pr_to_ticket = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Pull request opened: " ++ create_pr.html_url
    )
        .timeout(30s)

    node transition_in_review = jira.transition(
        ...jira_conn,
        key: params.ticket.key,
        transition_id: config.transitions.in_review
    )
        .timeout(30s)

    // -- phase 2: manual review gate + integration wait ----------------------

    gate manual every 5m timeout 24h { label: "Manual review for " ++ params.ticket.key }

    // give branch github integrations/jobs time to run before reading feedback.
    wait config.waits.integration_seconds

    node drift_after_integration = jira.poll(
        ...jira_conn,
        key: params.ticket.key
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    if drift_after_integration.fields.status.name != config.status.in_review {
        node drift1_note = jira.comment(
            ...jira_conn,
            key: params.ticket.key,
            body: "Manual status change detected ('" ++ drift_after_integration.fields.status.name ++ "'). Stopping."
        )
            .timeout(30s)
            -> handle_drift
    }

    // -- phase 3: address copilot feedback -----------------------------------

    // claude reads the PR's copilot feedback itself (via its tools) and addresses it.
    node fix_copilot = ai-command.claude_code(
        ...claude_cfg,
        working_dir: create_workspace.workspace,
        prompt: config.claude.copilot_prompt
            ++ " PR #" ++ string(create_pr.number)
            ++ " in " ++ config.github.owner ++ "/" ++ config.github.repo
            ++ ". If there is no Copilot feedback, make no changes."
    )
        .timeout(1800s)

    node copilot_diff = git.diff(
        workspace: create_workspace.workspace
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    if copilot_diff.stdout.trim().len() > 0 {
        node commit_copilot = git.commit(
            workspace: create_workspace.workspace,
            message: params.ticket.key ++ " address Copilot feedback"
        )
            .timeout(60s)
        node push_copilot = git.push(
            workspace: create_workspace.workspace,
            remote: config.git.remote,
            branch: config.branch.prefix ++ params.ticket.key
        )
            .timeout(60s)
            .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
    }

    // -- phase 4: /claude review loop ----------------------------------------

    node claude_trigger = github.add_comment(
        ...github_conn,
        issue_number: string(create_pr.number),
        body: "/claude"
    )
        .timeout(30s)

    wait config.waits.claude_seconds

    node drift_after_claude = jira.poll(
        ...jira_conn,
        key: params.ticket.key
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    if drift_after_claude.fields.status.name != config.status.in_review {
        node drift2_note = jira.comment(
            ...jira_conn,
            key: params.ticket.key,
            body: "Manual status change detected ('" ++ drift_after_claude.fields.status.name ++ "'). Stopping."
        )
            .timeout(30s)
            -> handle_drift
    }

    // fix only the highest-value feedback and report what was left and why.
    node fix_claude = ai-command.claude_code(
        ...claude_cfg,
        working_dir: create_workspace.workspace,
        prompt: config.claude.claude_feedback_prompt
            ++ " PR #" ++ string(create_pr.number)
            ++ " in " ++ config.github.owner ++ "/" ++ config.github.repo
    )
        .timeout(1800s)

    node post_unfixed = github.add_comment(
        ...github_conn,
        issue_number: string(create_pr.number),
        body: "Automated triage of /claude feedback:\n\n" ++ string(fix_claude.response)
    )
        .timeout(30s)

    node claude_diff = git.diff(
        workspace: create_workspace.workspace
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    if claude_diff.stdout.trim().len() > 0 {
        node commit_claude = git.commit(
            workspace: create_workspace.workspace,
            message: params.ticket.key ++ " address /claude feedback"
        )
            .timeout(60s)
        node push_claude = git.push(
            workspace: create_workspace.workspace,
            remote: config.git.remote,
            branch: config.branch.prefix ++ params.ticket.key
        )
            .timeout(60s)
            .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
    }

    // -- phase 5: human review and merge -------------------------------------

    node add_reviewers = github.request_reviewers(
        ...github_conn,
        pull_number: string(create_pr.number),
        reviewers: config.review.reviewers
    )
        .timeout(30s)

    node add_assignee = github.add_assignees(
        ...github_conn,
        issue_number: string(create_pr.number),
        assignees: config.review.assignees
    )
        .timeout(30s)

    // ci must settle green before we consider merging.
    until ci.status == "passed" || ci.status == "failed" limit 60 {
        wait config.ci_poll.interval_seconds
        node ci = github.checks_summary(
            ...github_conn,
            ref: create_pr.head.sha
        )
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    }
    if ci.status == "passed" {
        node ci_ok = jira.comment(
            ...jira_conn,
            key: params.ticket.key,
            body: "CI checks passed; entering review window."
        )
            .timeout(30s)
    } -> handle_failure

    // poll human reviews until enough approvals land or patience runs out. on a
    // change request, claude assesses and addresses the valid feedback.
    until review_state.ready limit 60 {
        wait config.waits.review_poll_seconds
        node reviews: any = github.reviews(
            ...github_conn,
            pull_number: string(create_pr.number)
        )
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
        node review_state: { approved: integer, changes_requested: integer, two_plus: boolean, ready: boolean } = compute {
            let approved = len(filter(reviews, r => r.state == "APPROVED"))
            let changes = len(filter(reviews, r => r.state == "CHANGES_REQUESTED"))
            return {
                approved: approved,
                changes_requested: changes,
                two_plus: gte(approved, 2),
                ready: or(gte(approved, 2), and(gte(approved, 1), eq(changes, 0)))
            }
        }
        if review_state.changes_requested > 0 {
            node fix_review = ai-command.claude_code(
                ...claude_cfg,
                working_dir: create_workspace.workspace,
                prompt: config.claude.review_fix_prompt
                    ++ " PR #" ++ string(create_pr.number)
                    ++ " in " ++ config.github.owner ++ "/" ++ config.github.repo
            )
                .timeout(1800s)
            node review_diff = git.diff(
                workspace: create_workspace.workspace
            )
                .timeout(30s)
                .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
            if review_diff.stdout.trim().len() > 0 {
                node commit_review = git.commit(
                    workspace: create_workspace.workspace,
                    message: params.ticket.key ++ " address review feedback"
                )
                    .timeout(60s)
                node push_review = git.push(
                    workspace: create_workspace.workspace,
                    remote: config.git.remote,
                    branch: config.branch.prefix ++ params.ticket.key
                )
                    .timeout(60s)
                    .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
            }
        }
    }

    // two approvals -> merge immediately. otherwise we required one approval with no
    // outstanding change requests, so honor the cool-down before merging.
    if review_state.two_plus {
        node merge_fast = github.merge_pr(
            ...github_conn,
            pull_number: string(create_pr.number),
            merge_method: "squash"
        )
            .timeout(60s)
            ok -> merged_diff
            fail -> handle_failure
    }

    wait config.waits.post_approval_seconds

    node drift_premerge = jira.poll(
        ...jira_conn,
        key: params.ticket.key
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    if drift_premerge.fields.status.name != config.status.in_review {
        node drift3_note = jira.comment(
            ...jira_conn,
            key: params.ticket.key,
            body: "Manual status change detected ('" ++ drift_premerge.fields.status.name ++ "') before merge. Stopping."
        )
            .timeout(30s)
            -> handle_drift
    }

    node merge_slow = github.merge_pr(
        ...github_conn,
        pull_number: string(create_pr.number),
        merge_method: "squash"
    )
        .timeout(60s)
        fail -> handle_failure

    // -- phase 6: path-based deploy ------------------------------------------

    node merged_diff = git.diff(
        workspace: create_workspace.workspace
    )
        .timeout(60s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

    node impact: { api: boolean, dashboards: boolean, lambdas: string[] } = compute {
        let files = split(merged_diff.stdout, "\n")
        return {
            api: any(files, f => f.starts_with(config.deploy.api_prefix)),
            dashboards: any(files, f => f.starts_with(config.deploy.dashboard_prefix)),
            lambdas: filter(files, f => f.starts_with(config.deploy.lambda_prefix))
        }
    }

    parallel {
        branch {
            if impact.api {
                node deploy_api = github.dispatch(
                    ...github_conn,
                    workflow_id: config.deploy.api_workflow,
                    ref: config.github.base_branch
                )
                    .timeout(60s)
                    fail -> handle_failure
            }
        }
        branch {
            if impact.dashboards {
                node deploy_dash = github.dispatch(
                    ...github_conn,
                    workflow_id: config.deploy.dashboard_workflow,
                    ref: config.github.base_branch
                )
                    .timeout(60s)
                    fail -> handle_failure
            }
        }
        branch {
            for lambda_path in impact.lambdas limit none {
                node deploy_lambda = github.dispatch(
                    ...github_conn,
                    workflow_id: config.deploy.lambda_workflow,
                    ref: config.github.base_branch,
                    inputs: { path: lambda_path }
                )
                    .timeout(60s)
                    fail -> handle_failure
            }
        }
    } join all

    // let the dispatched runs start, then confirm none failed before advancing.
    wait config.waits.deploy_settle_seconds

    node deploy_runs: any = github.workflow_runs(
        ...github_conn,
        branch: config.github.base_branch
    )
        .timeout(30s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    node deploy_state: { failed: integer } = compute {
        let runs = deploy_runs.workflow_runs
        return { failed: len(filter(runs, r => r.conclusion == "failure")) }
    }
    if deploy_state.failed > 0 {
        node deploy_failed_note = jira.comment(
            ...jira_conn,
            key: params.ticket.key,
            body: "A deployment workflow failed; returning ticket for follow-up."
        )
            .timeout(30s)
            -> handle_failure
    }

    // -- phase 7: ready for testing -> QA ------------------------------------

    node transition_testing = jira.transition(
        ...jira_conn,
        key: params.ticket.key,
        transition_id: config.transitions.ready_for_testing
    )
        .timeout(30s)
        fail -> handle_failure

    node ready_comment = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Merged " ++ create_pr.html_url ++ " and deployed. Ready for Testing."
    )
        .timeout(30s)

    node notify_ready = slack.send_message(
        ...slack_conn,
        text: ":rocket: " ++ params.ticket.key ++ " deployed and moved to Ready for Testing."
    )
        .timeout(15s)
        .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)

    // park on QA. react to whatever status QA lands on. claude on a re-picked run
    // reads the full comment history to determine the course of action.
    until qa.fields.status.name == config.status.done
        || qa.fields.status.name == config.status.ready_for_dev
        || qa.fields.status.name in config.status.terminal limit 480 {
        wait config.waits.qa_poll_seconds
        node qa = jira.poll(
            ...jira_conn,
            key: params.ticket.key
        )
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
    }

    match qa.fields.status.name {
        config.status.done -> {
            node qa_done_note = slack.send_message(
                ...slack_conn,
                text: ":white_check_mark: " ++ params.ticket.key ++ " passed QA and is Done."
            )
                .timeout(15s)
                .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
                -> cleanup_workspace
        }
        config.status.ready_for_dev -> {
            node qa_bounce = jira.comment(
                ...jira_conn,
                key: params.ticket.key,
                body: "QA returned this ticket to Ready for Development; the scanner will re-pick it for another development pass."
            )
                .timeout(30s)
                -> cleanup_workspace
        }
        else -> {
            node qa_other = jira.comment(
                ...jira_conn,
                key: params.ticket.key,
                body: "Automation stopping: ticket left the QA window in an unexpected status ('" ++ qa.fields.status.name ++ "')."
            )
                .timeout(30s)
                -> cleanup_workspace
        }
    }

    // -- terminals -----------------------------------------------------------

    // hard failure: return the ticket to Ready for Development so the scanner can
    // re-pick it (the branch and PR persist, so the next pass resumes the work).
    node handle_failure = jira.comment(
        ...jira_conn,
        key: params.ticket.key,
        body: "Automation failed; returning ticket to Ready for Development for another pass."
    )
        .timeout(30s)
    node failure_reset = jira.transition(
        ...jira_conn,
        key: params.ticket.key,
        transition_id: config.transitions.back_to_dev
    )
        .timeout(30s)
    node failure_notify = slack.send_message(
        ...slack_conn,
        text: ":x: SDLC automation failed on " ++ params.ticket.key ++ "."
    )
        .timeout(15s)
        .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
        -> cleanup_workspace

    // drift: a human changed the status. do not fight it; just stop and clean up.
    node handle_drift = slack.send_message(
        ...slack_conn,
        text: ":warning: SDLC automation halted on " ++ params.ticket.key ++ " due to a manual JIRA status change."
    )
        .timeout(15s)
        .retry(3, backoff: 5s, max: 45s, jitter: true, on: failure)
        -> cleanup_workspace

    node cleanup_workspace = git.cleanup(
        repo: config.git.repo,
        path: create_workspace.workspace
    )
        .timeout(60s)
        .retry(2, backoff: 5s, max: 30s, jitter: true, on: failure)
        -> done
}
