module runinator/artifacts

open runinator/types
open runinator/durable_state

pred persistBlob[a: Artifact] {
  a not in State.blobPresent
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent + a
  State.artifactRecorded' = State.artifactRecorded
}

pred recordArtifact[a: Artifact] {
  a in State.blobPresent
  a not in State.artifactRecorded
  some e: Effect | e.effectContinuation.continuationRun = a.artifactRun and State.effectStatus[e] = EffectSucceeded
  State.authorizedRuns' = State.authorizedRuns
  State.runStatus' = State.runStatus
  State.continuationStatus' = State.continuationStatus
  State.effectStatus' = State.effectStatus
  State.effectAttempt' = State.effectAttempt
  State.dispatchStatus' = State.dispatchStatus
  State.publicationCount' = State.publicationCount
  State.reportedOutcome' = State.reportedOutcome
  State.ingressRelayed' = State.ingressRelayed
  State.agentIngress' = State.agentIngress
  State.blobPresent' = State.blobPresent
  State.artifactRecorded' = State.artifactRecorded + a
}
