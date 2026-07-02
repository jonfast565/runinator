import { fetchSupervisorStatus, type SupervisorStatus } from "../api/commandCenterApi";

export function createSupervisorService() {
  return {
    fetchStatus(): Promise<SupervisorStatus> {
      return fetchSupervisorStatus();
    },
  };
}

export type SupervisorService = ReturnType<typeof createSupervisorService>;
export type { SupervisorStatus };
