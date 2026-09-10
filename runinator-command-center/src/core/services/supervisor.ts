import { defaultApi, type SupervisorApi } from "../api/ports/supervisor";
import type { SupervisorStatus } from "../api/commandCenterApi";

export function createSupervisorService(api: SupervisorApi = defaultApi) {
  return {
    fetchStatus(): Promise<SupervisorStatus> {
      return api.fetchSupervisorStatus();
    },
  };
}

export type SupervisorService = ReturnType<typeof createSupervisorService>;
export type { SupervisorStatus };
