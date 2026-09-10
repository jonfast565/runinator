import { defaultApi, type AuditLogApi } from "../api/ports/audit-log";

import type { JsonRecord } from "../domain/models";
import type { AppService } from "./app";

export function createAuditLogService(app: AppService, api: AuditLogApi = defaultApi) {
  return {
    async list(action?: string, limit = 200): Promise<JsonRecord[]> {
      return app
        .runOperation("Loading audit log", () => api.listAuditLog(undefined, action, limit), {
          retryable: true,
        })
        .catch(() => []);
    },
  };
}

export type AuditLogService = ReturnType<typeof createAuditLogService>;
