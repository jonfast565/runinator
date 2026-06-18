workflow "Core Team SDLC Pipeline" v1 {
    trigger cron "0 * * * *"

    import std

    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }

    // fresh work waiting in Ready for Development.
    node tickets = jira.search(
        ...jira_conn,
        jql: config.jira.ready_jql
    )
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

    // tickets the automation already owns and is moving through the pipeline. jira
    // is the source of truth for the concurrency cap, so no self-referential api
    // call is needed.
    node in_flight = jira.search(
        ...jira_conn,
        jql: config.jira.in_flight_jql
    )
        .timeout(120s)
        .retry(4, backoff: 5s, max: 60s, jitter: true, on: any)

    // how many new tickets we may admit this tick: the cap minus what is already in
    // flight. a non-positive budget admits nothing (the reducer clamps to zero).
    node budget: integer = compute {
        return sub(config.selection.max_in_flight, len(in_flight.issues))
    }

    // spawn up to the budget. starting a ticket transitions it out of Ready for
    // Development, so the next tick's in-flight count reflects it and the cap holds.
    // `reuse` makes re-scans idempotent: an open run for a ticket is never duplicated.
    for ticket in tickets.issues limit budget {
        node spawn "Ticket Work" reuse as "Ticket Work: " ++ ticket.key with {
            ticket,
            parent_workflow_run_id: run.run_id
        }
    }
}
