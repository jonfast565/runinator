import {
  createAgentEnrollmentToken,
  invalidateAgentMachine,
  listAgentMachines,
  listAgentEnrollmentTokens,
  revokeAgentEnrollmentToken,
} from "../api/commandCenterApi";
import type { CreateAgentEnrollmentTokenInput } from "../domain/models";
import type { AppService } from "./app";

export function createAgentEnrollmentService(app: AppService) {
  return {
    create(request: CreateAgentEnrollmentTokenInput) {
      return app.runOperation("Creating enrollment token", () =>
        createAgentEnrollmentToken(request),
      );
    },
    list() {
      return app.runOperation("Loading enrollment tokens", () => listAgentEnrollmentTokens());
    },
    revoke(tokenId: string) {
      return app.runOperation("Revoking enrollment token", () =>
        revokeAgentEnrollmentToken(tokenId),
      );
    },
    machines() {
      return app.runOperation("Loading enrolled machines", () => listAgentMachines());
    },
    invalidate(machineId: string) {
      return app.runOperation("Invalidating enrolled machine", () =>
        invalidateAgentMachine(machineId),
      );
    },
  };
}

export type AgentEnrollmentService = ReturnType<typeof createAgentEnrollmentService>;
