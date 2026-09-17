module runinator/effect_delivery

open runinator/types
open runinator/durable_state

pred publishAction[d: Dispatch] {
  d.dispatchEffect.effectKind = ActionEffect
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

pred redeliverAction[d: Dispatch] {
  d.dispatchEffect.effectKind = ActionEffect
  State.dispatchStatus[d] = DispatchPublished
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount - d->State.publicationCount[d] + d->State.publicationCount[d].plus[1]
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred workerClaim[e: Effect] {
  e.effectKind = ActionEffect
  State.effectStatus[e] = EffectRequested
  some d: Dispatch | d.dispatchEffect = e and State.dispatchStatus[d] = DispatchPublished
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus - e->EffectRequested + e->EffectRunning
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred workerReportsSuccess[e: Effect] {
  State.effectStatus[e] = EffectRunning
  no State.reportedOutcome[e]
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome + e->EffectSucceeded
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred workerReportsFailure[e: Effect] {
  State.effectStatus[e] = EffectRunning
  no State.reportedOutcome[e]
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome + e->EffectFailed
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred engineSettleReported[e: Effect] {
  State.effectStatus[e] = EffectRunning
  State.reportedOutcome[e] in EffectSucceeded + EffectFailed
  State.continuationStatus[e.effectContinuation] = ContinuationWaiting
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus - e.effectContinuation->ContinuationWaiting + e.effectContinuation->ContinuationRunnable
  State.effectStatus' = State.effectStatus - e->EffectRunning + e->State.reportedOutcome[e]
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}

pred engineRetry[e: Effect, d: Dispatch] {
  State.effectStatus[e] = EffectRunning
  State.reportedOutcome[e] = EffectFailed
  d.dispatchEffect = e
  State.dispatchStatus[d] = DispatchPublished
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus - e->EffectRunning + e->EffectRequested
  State.effectAttempt' = State.effectAttempt - e->State.effectAttempt[e] + e->State.effectAttempt[e].plus[1]
  State.dispatchStatus' = State.dispatchStatus - d->DispatchPublished + d->DispatchPending
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome - e->EffectFailed
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded
}
