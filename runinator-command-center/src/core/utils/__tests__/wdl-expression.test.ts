import { describe, expect, it } from "vitest";
import { expressionJsonToWdl, parseWdlExpression } from "../wdl-expression";

describe("WDL expression conversion", () => {
  it("renders lowered references and operators as WDL surface expressions", () => {
    expect(expressionJsonToWdl({ $ref: { params: ["ticket_id"] } })).toBe("params.ticket_id");
    expect(expressionJsonToWdl({ $ref: { input: ["ticket_id"] } })).toBe(
      '{ input: ["ticket_id"] }',
    );
    expect(expressionJsonToWdl({ $ref: { workflow: ["attempt"] } })).toBe("run.attempt");
    expect(expressionJsonToWdl({ $ref: { node: "create_ticket", output: ["id"] } })).toBe(
      "create_ticket.id",
    );
    expect(expressionJsonToWdl({ $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }] })).toBe(
      '"ticket " ++ params.ticket_id',
    );
    expect(expressionJsonToWdl({ $coalesce: [{ $ref: { prev: ["name"] } }, "unknown"] })).toBe(
      'prev.name ?? "unknown"',
    );
    expect(expressionJsonToWdl({ $to_string: { $ref: { prev: ["count"] } } })).toBe(
      "string(prev.count)",
    );
  });

  it("parses WDL surface expressions back into lowered JSON values", () => {
    expect(parseWdlExpression("params.ticket_id")).toEqual({ $ref: { params: ["ticket_id"] } });
    expect(parseWdlExpression('"ticket " ++ params.ticket_id')).toEqual({
      $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }],
    });
    expect(parseWdlExpression("input.ticket_id")).toEqual({
      $ref: { node: "input", output: ["ticket_id"] },
    });
    expect(parseWdlExpression('prev.name ?? "unknown"')).toEqual({
      $coalesce: [{ $ref: { prev: ["name"] } }, "unknown"],
    });
    expect(parseWdlExpression("string(prev.count)")).toEqual({
      $to_string: { $ref: { prev: ["count"] } },
    });
    expect(parseWdlExpression("{ message: string(prev.count), tags: [params.tag] }")).toEqual({
      message: { $to_string: { $ref: { prev: ["count"] } } },
      tags: [{ $ref: { params: ["tag"] } }],
    });
  });
});
