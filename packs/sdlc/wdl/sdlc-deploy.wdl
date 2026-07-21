workflow "SDLC: Deploy" v1 {
    // scan Ready for Testing tickets (merged, awaiting deploy). each pass classifies the merged diff
    // by path, dispatches the matching deploy workflows, verifies none failed, then moves the ticket
    // to In Testing (the QA phase's inbox). the status transition is the deploy marker: a ticket in
    // In Testing has already been dispatched, so it is never double-deployed.
    trigger cron "*/30 * * * *"

    mutex "sdlc-deploy" every 10s timeout 1800s

    import std

    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }
    alias github_conn = { token: secret.github.token, owner: config.github.owner, repo: config.github.repo }

    set name = "SDLC Deploy scan"

    node tickets <- jira.search(...jira_conn, jql: config.jira.deploy_jql)
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

    for ticket in tickets.issues limit config.selection.max_in_flight {
        node workspace <- git.worktree(
            repo: config.git.repo,
            branch: config.branch.prefix ++ ticket.key,
            path: config.git.worktree_root ++ "/" ++ ticket.key
        )
            .runner("sdlc")
            .timeout(120s)
            .retry(2, backoff: 5s, max: 30s, jitter: true, on: failure)

        node merged_diff <- git.diff(workspace: workspace.workspace)
            .runner("sdlc")
            .timeout(60s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

        node impact: { api: boolean, dashboards: boolean, lambdas: string[] } <- compute {
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
                    node deploy_api <- github.dispatch(
                        ...github_conn,
                        workflow_id: config.deploy.api_workflow,
                        ref: config.github.base_branch
                    )
                        .timeout(60s)
                }
            }
            branch {
                if impact.dashboards {
                    node deploy_dash <- github.dispatch(
                        ...github_conn,
                        workflow_id: config.deploy.dashboard_workflow,
                        ref: config.github.base_branch
                    )
                        .timeout(60s)
                }
            }
            branch {
                for lambda_path in impact.lambdas limit none {
                    node deploy_lambda <- github.dispatch(
                        ...github_conn,
                        workflow_id: config.deploy.lambda_workflow,
                        ref: config.github.base_branch,
                        inputs: { path: lambda_path }
                    )
                        .timeout(60s)
                }
            }
        } join all

        // let the dispatched runs start, then confirm none failed before handing off to QA.
        wait config.waits.deploy_settle_seconds

        node deploy_runs: any <- github.workflow_runs(
            ...github_conn,
            branch: config.github.base_branch
        )
            .timeout(30s)
            .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)
        node deploy_state: { failed: integer } <- compute {
            let runs = deploy_runs.workflow_runs
            return { failed: len(filter(runs, r => r.conclusion == "failure")) }
        }

        // only advance a clean deploy; a failed deploy leaves the ticket in Ready for Testing with a
        // note so the next pass retries.
        if deploy_state.failed > 0 {
            node deploy_failed_note <- jira.comment(
                ...jira_conn,
                key: ticket.key,
                body: "A deployment workflow failed for " ++ ticket.key ++ "; leaving in Ready for Testing to retry."
            )
                .timeout(30s)
        }
        if eq(deploy_state.failed, 0) {
            node transition_testing <- jira.transition(
                ...jira_conn,
                key: ticket.key,
                transition_id: config.transitions.in_testing
            )
                .timeout(30s)
            node deployed_note <- jira.comment(
                ...jira_conn,
                key: ticket.key,
                body: "Deployed " ++ ticket.key ++ "; moving to In Testing for QA."
            )
                .timeout(30s)
        }
    }
}
