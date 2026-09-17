module runinator/vm_lifecycle

open runinator/types
open runinator/durable_state

pred yieldEffect[e: Effect, d: Dispatch] {
  State.continuationStatus[e.effectContinuation] = ContinuationRunnable
  State.runStatus[e.effectContinuation.continuationRun] = RunRunning
  no State.effectStatus[e]
  no other: Effect |
    other != e
    and createdEffect[other]
    and other.effectContinuation = e.effectContinuation
    and other.effectSequence = e.effectSequence
  no State.dispatchStatus[d]
  d.dispatchEffect = e
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus - e.effectContinuation->ContinuationRunnable + e.effectContinuation->ContinuationWaiting
  State.effectStatus' = State.effectStatus + e->EffectRequested
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus + d->DispatchPending
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred fork[parent, child: Continuation] {
  State.continuationStatus[parent] = ContinuationRunnable
  State.runStatus[parent.continuationRun] = RunRunning
  no State.continuationStatus[child]
  child.continuationRun = parent.continuationRun
  child.parentContinuation = parent
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus + child->ContinuationRunnable
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

pred completeContinuation[c: Continuation] {
  State.continuationStatus[c] = ContinuationRunnable
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus - c->ContinuationRunnable + c->ContinuationSucceeded
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

pred settleRun[r: Run] {
  State.runStatus[r] = RunRunning
  some c: Continuation |
    c.continuationRun = r
    and c.continuationKind = OrdinaryContinuation
    and createdContinuation[c]
  all c: Continuation |
    c.continuationRun = r
    and c.continuationKind = OrdinaryContinuation
    and createdContinuation[c]
    implies terminalContinuation[State.continuationStatus[c]]
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus - r->RunRunning + r->RunSucceeded
  State.continuationStatus' = State.continuationStatus
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
