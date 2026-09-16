# Classify deployment status

Close only for a successful matching deployment, continue monitoring while it is queued, in
progress, or not yet visible, and hand off on failure or ambiguity. Return the response required by
the supplied JSON schema.
