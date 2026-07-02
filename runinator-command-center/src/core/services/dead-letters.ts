import { listDeadLetters } from "../api/commandCenterApi";
import type { JsonRecord } from "../domain/models";
import type { AppService } from "./app";

export function createDeadLettersService(app: AppService) {
  return {
    async list(channel?: string, limit = 200): Promise<JsonRecord[]> {
      return app
        .runOperation("Loading dead letters", () => listDeadLetters(channel, limit))
        .catch(() => []);
    },
  };
}

export type DeadLettersService = ReturnType<typeof createDeadLettersService>;
