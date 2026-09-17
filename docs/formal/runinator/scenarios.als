module runinator/scenarios

open runinator/types
open runinator/durable_state
open runinator/control_plane
open runinator/vm_lifecycle
open runinator/effect_delivery
open runinator/ingress

pred successfulActionScenario {
  some r: Run, c: Continuation, e: Effect, d: Dispatch |
    e.effectKind = ActionEffect
    and authorizedStart[r, c]
    and after (yieldEffect[e, d]
    and after (publishAction[d]
    and after (workerClaim[e]
    and after (workerReportsSuccess[e]
    and after engineSettleReported[e]))))
}

pred timerWakeScenario {
  some r: Run, c: Continuation, e: Effect, d: Dispatch |
    e.effectKind = TimerEffect
    and authorizedStart[r, c]
    and after (yieldEffect[e, d]
    and after (armTimer[d]
    and after (wakerRelaysTimer[e]
    and after engineSettlesTimer[e])))
}

pred redeliveryScenario {
  some r: Run, c: Continuation, e: Effect, d: Dispatch |
    e.effectKind = ActionEffect
    and authorizedStart[r, c]
    and after (yieldEffect[e, d]
    and after (publishAction[d]
    and after (redeliverAction[d]
    and after workerClaim[e])))
}

pred retryScenario {
  some r: Run, c: Continuation, e: Effect, d: Dispatch |
    e.effectKind = ActionEffect
    and authorizedStart[r, c]
    and after (yieldEffect[e, d]
    and after (publishAction[d]
    and after (workerClaim[e]
    and after (workerReportsFailure[e]
    and after (engineRetry[e, d]
    and after publishAction[d])))))
}

pred deniedControlScenario {
  some r: Run | unauthorizedRequestDenied[r] and no State.runStatus[r]
}
