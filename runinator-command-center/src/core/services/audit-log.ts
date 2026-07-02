import { listAuditLog } from "../api/commandCenterApi";
import type { JsonRecord } from "../domain/models";
import type { AppService } from "./app";

export function createAuditLogService(app: AppService) {
  return {
    async list(action?: string, limit = 200): Promise<JsonRecord[]> {
      return app.runOperation("Loading audit log", () => listAuditLog(undefined, action, limit)).catch(
        () => [],
      );
    },
  };
}

export type AuditLogService = ReturnType<typeof createAuditLogService>;
