module runinator/types

abstract sig Actor {}
one sig Operator, WebService, Engine, DurableStore, Broker, Worker, Waker, AdapterHost, Agent, BlobStore extends Actor {}

abstract sig Channel {}
one sig ControlChannel, EffectChannel, EffectResultChannel, WakeChannel, IngressChannel, AgentChannel extends Channel {}

abstract sig RunStatus {}
one sig RunQueued, RunRunning, RunSucceeded, RunFailed, RunTimedOut, RunCanceled extends RunStatus {}

abstract sig ContinuationStatus {}
one sig ContinuationRunnable, ContinuationWaiting, ContinuationJoined, ContinuationSucceeded, ContinuationFailed, ContinuationCanceled extends ContinuationStatus {}

abstract sig EffectStatus {}
one sig EffectRequested, EffectRunning, EffectSucceeded, EffectFailed, EffectTimedOut, EffectCanceled extends EffectStatus {}

abstract sig DispatchStatus {}
one sig DispatchPending, DispatchPublished extends DispatchStatus {}

abstract sig EffectKind {}
one sig ActionEffect, TimerEffect, AdapterEffect, AgentEffect extends EffectKind {}

abstract sig ContinuationKind {}
one sig OrdinaryContinuation, InterruptHandlerContinuation extends ContinuationKind {}

pred terminalRun[s: lone RunStatus] { some s and s in RunSucceeded + RunFailed + RunTimedOut + RunCanceled }
pred terminalContinuation[s: lone ContinuationStatus] { some s and s in ContinuationSucceeded + ContinuationFailed + ContinuationCanceled }
pred terminalEffect[s: lone EffectStatus] { some s and s in EffectSucceeded + EffectFailed + EffectTimedOut + EffectCanceled }
