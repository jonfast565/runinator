module runinator/control_plane

open runinator/types
open runinator/durable_state

pred authorizedStart[r: Run, c: Continuation] {
  no State.runStatus[r]
  c.continuationRun = r
  no c.parentContinuation
  c.continuationKind = OrdinaryContinuation
  State.authorizedRuns' = State.authorizedRuns + r
  State.runStatus' = State.runStatus + r->RunRunning
  State.continuationStatus' = State.continuationStatus + c->ContinuationRunnable
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred unauthorizedRequestDenied[r: Run] {
  r not in State.authorizedRuns
  noChange[]
}
