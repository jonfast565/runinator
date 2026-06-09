workflow "Core Team SDLC Pipeline" v1 {
    trigger cron "0 * * * *"

    // shared settings come from config.* (eager, non-secret); tokens come from secret.* (late, at the worker).
    let find_tickets = jira.search(base_url: config.jira.base_url, email: config.jira.email, jql: config.jira.jql, token: secret.jira.token)

    for item in find_tickets.issues limit 50 {
        let spawn_ticket_work = spawn "Ticket Work" reuse as "Ticket Work: " ++ item.key with { parent_workflow_run_id: run.run_id, ticket: item }
    }
}
