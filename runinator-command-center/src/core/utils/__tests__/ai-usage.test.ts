import { describe, expect, it } from "vitest";
import { aiTokenTotal, formatAiCost } from "../ai-usage";

describe("AI usage presentation", () => {
  it("renders priced usage from integer micro-USD", () => {
    expect(formatAiCost(12_345)).toBe("$0.012345");
  });

  it("renders unknown cost as Unpriced instead of zero", () => {
    expect(formatAiCost(null)).toBe("Unpriced");
  });

  it("keeps empty usage at zero tokens", () => {
    expect(
      aiTokenTotal({
        input_tokens: 0,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: 0,
      }),
    ).toBe(0);
  });

  it("sums every token category for mixed providers", () => {
    const claude = aiTokenTotal({
      input_tokens: 10,
      cached_input_tokens: 3,
      cache_creation_input_tokens: 2,
      output_tokens: 4,
      reasoning_tokens: 0,
    });
    const codex = aiTokenTotal({
      input_tokens: 5,
      cached_input_tokens: 1,
      cache_creation_input_tokens: 0,
      output_tokens: 2,
      reasoning_tokens: 6,
    });

    expect(claude + codex).toBe(33);
  });
});
