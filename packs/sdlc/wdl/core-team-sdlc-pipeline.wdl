workflow "Core Team SDLC Pipeline" v1 {
    trigger cron "0 * * * *"

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
    type JiraSearchResult = {
        issues: JiraIssue[],
        ...: any
    }

    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }

    node tickets: JiraSearchResult = jira.search(
        ...jira_conn,
        jql: config.jira.jql
    )
        .timeout(120s)
        .retry(3)

    for ticket in tickets.issues limit 50 {
        node spawn "Ticket Work" reuse as "Ticket Work: " ++ ticket.key with {
            ticket,
            parent_workflow_run_id: run.run_id
        }
    }
}
