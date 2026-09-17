module runinator/properties

open runinator/types
open runinator/topology
open runinator/durable_state

assert LogicalEffectIdentity {
  always all disj first, second: Effect |
    not (createdEffect[first] and createdEffect[second]
      and first.effectContinuation = second.effectContinuation
      and first.effectSequence = second.effectSequence)
}

assert TerminalEffectIsImmutable {
  always all e: Effect |
    terminalEffect[State.effectStatus[e]] implies State.effectStatus'[e] = State.effectStatus[e]
}

assert WaitingContinuationHasOneOutstandingEffect {
  always all c: Continuation |
    State.continuationStatus[c] = ContinuationWaiting implies
      one e: Effect |
        e.effectContinuation = c
        and createdEffect[e]
        and not terminalEffect[State.effectStatus[e]]
}

assert TerminalRunHasNoLiveOrdinaryContinuation {
  always all r: Run |
    terminalRun[State.runStatus[r]] implies
      all c: Continuation |
        c.continuationRun = r
        and c.continuationKind = OrdinaryContinuation
        and createdContinuation[c]
        implies terminalContinuation[State.continuationStatus[c]]
}

assert TimerSettlementRequiresIngress {
  always all e: Effect |
    e.effectKind = TimerEffect and State.effectStatus[e] = EffectSucceeded implies e in State.ingressRelayed
}

assert ArtifactRecordHasDurableBlob {
  always State.artifactRecorded in State.blobPresent
}
