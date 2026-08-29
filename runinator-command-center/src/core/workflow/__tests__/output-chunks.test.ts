import { describe, expect, it } from "vitest";
import { outputChunkLines, outputChunkTimestamp } from "../output-chunks";

describe("output chunk formatting", () => {
  it("keeps effect context on every physical line of a multi-line chunk", () => {
    const lines = outputChunkLines([
      {
        id: "event-1",
        effect_id: "effect-1",
        continuation_id: "continuation-1",
        stream: "stderr",
        content: "first\nsecond",
        attempt: 2,
        created_at: "2026-08-29T03:05:31.000Z",
      },
    ]);

    expect(lines).toEqual([
      expect.objectContaining({
        id: "event-1:0",
        content: "first",
        stream: "stderr",
        attempt: 2,
        effectId: "effect-1",
        continuationId: "continuation-1",
      }),
      expect.objectContaining({ id: "event-1:1", content: "second" }),
    ]);
  });

  it("uses an ISO timestamp when the output event has a valid date", () => {
    expect(outputChunkTimestamp("2026-08-29T03:05:31.000Z")).toBe("2026-08-29T03:05:31.000Z");
  });
});
