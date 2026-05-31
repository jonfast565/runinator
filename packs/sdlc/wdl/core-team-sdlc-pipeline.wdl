workflow "Core Team SDLC Pipeline" v1 {
    input {
        ci_poll: { interval_seconds: integer, max_polls: integer }
        claude: { allowed_tools: string, binary: string, extra_args: string[], model: string, output_format: string, permission_mode: string, prompt_intro: string }
        git: { remote: string, repo: string }
        github: { base_branch: string, owner: string, repo: string, token: string }
        jira: { base_url: string, email: string, jql: string, token: string }
        slack: { channel: string, token: string }
        transitions: { done: string, in_progress: string, in_review: string }
    }

    let find_tickets = jira.search(base_url: input.jira.base_url, email: input.jira.email, jql: input.jira.jql, token: input.jira.token)
    for item in find_tickets.issues limit 50 {
        let spawn_ticket_work = spawn "Ticket Work" reuse as "Ticket Work: " ++ item.key with { ci_poll: input.ci_poll, claude: input.claude, git: input.git, github: input.github, jira: input.jira, parent_workflow_run_id: run.run_id, slack: input.slack, ticket: item, transitions: input.transitions }
    }
}
