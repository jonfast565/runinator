module runinator/runinator

open runinator/types
open runinator/topology
open runinator/durable_state
open runinator/control_plane
open runinator/vm_lifecycle
open runinator/effect_delivery
open runinator/ingress
open runinator/artifacts
open runinator/properties
open runinator/scenarios

pred stutter[] { noChange[] }

pred step[] {
  stutter[]
  or some r: Run, c: Continuation | authorizedStart[r, c]
  or some r: Run | unauthorizedRequestDenied[r]
  or some e: Effect, d: Dispatch | yieldEffect[e, d]
  or some parent, child: Continuation | fork[parent, child]
  or some c: Continuation | completeContinuation[c]
  or some r: Run | settleRun[r]
  or some d: Dispatch | publishAction[d]
  or some d: Dispatch | redeliverAction[d]
  or some e: Effect | workerClaim[e]
  or some e: Effect | workerReportsSuccess[e]
  or some e: Effect | workerReportsFailure[e]
  or some e: Effect | engineSettleReported[e]
  or some e: Effect, d: Dispatch | engineRetry[e, d]
  or some d: Dispatch | armTimer[d]
  or some e: Effect | wakerRelaysTimer[e]
  or some e: Effect | engineSettlesTimer[e]
  or some r: Run | agentReports[r]
  or some a: Artifact | persistBlob[a]
  or some a: Artifact | recordArtifact[a]
}

fact Trace {
  initialize[]
  always step[]
}

run successfulActionScenario for 3 but 8 steps
run timerWakeScenario for 3 but 7 steps
run redeliveryScenario for 3 but 7 steps
run retryScenario for 3 but 10 steps
run deniedControlScenario for 3 but 2 steps

check OnlyPermittedChannelsExist for 3
check LogicalEffectIdentity for 3 but 10 steps
check TerminalEffectIsImmutable for 3 but 10 steps
check WaitingContinuationHasOneOutstandingEffect for 3 but 10 steps
check TerminalRunHasNoLiveOrdinaryContinuation for 3 but 10 steps
check TimerSettlementRequiresIngress for 3 but 10 steps
check ArtifactRecordHasDurableBlob for 3 but 10 steps
