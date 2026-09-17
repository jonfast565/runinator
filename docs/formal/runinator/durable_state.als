module runinator/durable_state

open runinator/types

sig Definition {}
sig Run { runDefinition: one Definition }
sig Continuation {
  continuationRun: one Run,
  continuationKind: one ContinuationKind,
  parentContinuation: lone Continuation
}
sig Effect {
  effectContinuation: one Continuation,
  effectSequence: one Int,
  effectKind: one EffectKind
}
sig Dispatch { dispatchEffect: one Effect }
sig Artifact { artifactRun: one Run }

one sig State {
  var authorizedRuns: set Run,
  var runStatus: Run -> lone RunStatus,
  var continuationStatus: Continuation -> lone ContinuationStatus,
  var effectStatus: Effect -> lone EffectStatus,
  var effectAttempt: Effect -> one Int,
  var dispatchStatus: Dispatch -> lone DispatchStatus,
  var publicationCount: Dispatch -> one Int,
  var reportedOutcome: Effect -> lone EffectStatus,
  var ingressRelayed: set Effect,
  var agentIngress: set Run,
  var blobPresent: set Artifact,
  var artifactRecorded: set Artifact
}

pred createdRun[r: Run] { some State.runStatus[r] }
pred createdContinuation[c: Continuation] { some State.continuationStatus[c] }
pred createdEffect[e: Effect] { some State.effectStatus[e] }
pred createdDispatch[d: Dispatch] { some State.dispatchStatus[d] }

pred noChange[] {
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
  State.artifactRecorded' = State.artifactRecorded
}

pred initialize[] {
  no State.authorizedRuns
  no State.runStatus
  no State.continuationStatus
  no State.effectStatus
  State.effectAttempt = Effect -> 0
  no State.dispatchStatus
  State.publicationCount = Dispatch -> 0
  no State.reportedOutcome
  no State.ingressRelayed
  no State.agentIngress
  no State.blobPresent
  no State.artifactRecorded
}

fact StaticRecordWellFormedness {
  all c: Continuation | c not in c.^parentContinuation
}
