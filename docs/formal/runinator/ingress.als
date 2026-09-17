module runinator/ingress

open runinator/types
open runinator/durable_state

pred armTimer[d: Dispatch] {
  d.dispatchEffect.effectKind = TimerEffect
  State.dispatchStatus[d] = DispatchPending
  State.effectStatus[d.dispatchEffect] = EffectRequested
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus - d->DispatchPending + d->DispatchPublished
  State.publicationCount' = State.publicationCount - d->State.publicationCount[d] + d->State.publicationCount[d].plus[1]
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred wakerRelaysTimer[e: Effect] {
  e.effectKind = TimerEffect
  State.effectStatus[e] = EffectRequested
  some d: Dispatch | d.dispatchEffect = e and State.dispatchStatus[d] = DispatchPublished
  e not in State.ingressRelayed
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed + e
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred engineSettlesTimer[e: Effect] {
  e.effectKind = TimerEffect
  State.effectStatus[e] = EffectRequested
  e in State.ingressRelayed
  State.continuationStatus[e.effectContinuation] = ContinuationWaiting
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus - e.effectContinuation->ContinuationWaiting + e.effectContinuation->ContinuationRunnable
  State.effectStatus' = State.effectStatus - e->EffectRequested + e->EffectSucceeded
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred agentReports[r: Run] {
  createdRun[r]
  r not in State.agentIngress
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress + r
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}
