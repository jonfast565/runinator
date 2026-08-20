import { describe, expect, it } from "vitest";
import { expressionJsonToRexRap, parseRexRapExpression } from "../rexrap-expression";

describe("REXRAP expression conversion", () => {
  it("renders lowered references and operators as REXRAP surface expressions", () => {
    expect(expressionJsonToRexRap({ $ref: { params: ["ticket_id"] } })).toBe("params.ticket_id");
    expect(expressionJsonToRexRap({ $ref: { input: ["ticket_id"] } })).toBe(
      '{ input: ["ticket_id"] }',
    );
    expect(expressionJsonToRexRap({ $ref: { workflow: ["attempt"] } })).toBe("run.attempt");
    expect(expressionJsonToRexRap({ $ref: { node: "create_ticket", output: ["id"] } })).toBe(
      "create_ticket.id",
    );
    expect(expressionJsonToRexRap({ $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }] })).toBe(
      '"ticket " ++ params.ticket_id',
    );
    expect(expressionJsonToRexRap({ $coalesce: [{ $ref: { prev: ["name"] } }, "unknown"] })).toBe(
      'prev.name ?? "unknown"',
    );
    expect(expressionJsonToRexRap({ $to_string: { $ref: { prev: ["count"] } } })).toBe(
      "string(prev.count)",
    );
  });

  it("parses REXRAP surface expressions back into lowered JSON values", () => {
    expect(parseRexRapExpression("params.ticket_id")).toEqual({ $ref: { params: ["ticket_id"] } });
    expect(parseRexRapExpression('"ticket " ++ params.ticket_id')).toEqual({
      $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }],
    });
    expect(parseRexRapExpression("input.ticket_id")).toEqual({
      $ref: { node: "input", output: ["ticket_id"] },
    });
    expect(parseRexRapExpression('prev.name ?? "unknown"')).toEqual({
      $coalesce: [{ $ref: { prev: ["name"] } }, "unknown"],
    });
    expect(parseRexRapExpression("string(prev.count)")).toEqual({
      $to_string: { $ref: { prev: ["count"] } },
    });
    expect(parseRexRapExpression("{ message: string(prev.count), tags: [params.tag] }")).toEqual({
      message: { $to_string: { $ref: { prev: ["count"] } } },
      tags: [{ $ref: { params: ["tag"] } }],
    });
  });
});
