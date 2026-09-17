module runinator/topology

open runinator/types

one sig Topology { permitted: Actor -> Channel -> Actor }

fact RuninatorTopology {
  Topology.permitted =
      Operator->ControlChannel->WebService
    + WebService->ControlChannel->Engine
    + Engine->EffectChannel->Worker
    + Worker->EffectResultChannel->Engine
    + Engine->WakeChannel->Waker
    + Waker->IngressChannel->Engine
    + AdapterHost->IngressChannel->Engine
    + Engine->AgentChannel->Agent
    + Agent->IngressChannel->Engine
    + Engine->ControlChannel->Worker
}

assert OnlyPermittedChannelsExist {
  Topology.permitted =
      Operator->ControlChannel->WebService
    + WebService->ControlChannel->Engine
    + Engine->EffectChannel->Worker
    + Worker->EffectResultChannel->Engine
    + Engine->WakeChannel->Waker
    + Waker->IngressChannel->Engine
    + AdapterHost->IngressChannel->Engine
    + Engine->AgentChannel->Agent
    + Agent->IngressChannel->Engine
    + Engine->ControlChannel->Worker
}
