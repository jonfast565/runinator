import { defaultApi, type DeadLettersApi } from "../api/ports/dead-letters";

import type { JsonRecord } from "../domain/models";
import type { AppService } from "./app";

export function createDeadLettersService(app: AppService, api: DeadLettersApi = defaultApi) {
  return {
    async list(channel?: string, limit = 200): Promise<JsonRecord[]> {
      return app
        .runOperation("Loading dead letters", () => api.listDeadLetters(channel, limit), {
          retryable: true,
        })
        .catch(() => []);
    },
  };
}

export type DeadLettersService = ReturnType<typeof createDeadLettersService>;
