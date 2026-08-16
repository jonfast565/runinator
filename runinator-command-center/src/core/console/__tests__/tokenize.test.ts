// covers splitting a command line into arguments, which is where quoted json has to survive.

import { describe, expect, it } from "vitest";
import { ConsoleParseError, tokenize } from "../tokenize";

describe("tokenize", () => {
  it("splits on whitespace", () => {
    expect(tokenize("workflows list")).toEqual(["workflows", "list"]);
    expect(tokenize("  runs   show  7 ")).toEqual(["runs", "show", "7"]);
  });

  it("keeps quoted json as one argument", () => {
    expect(tokenize(`settings set aws key '{"a": 1}'`)).toEqual([
      "settings",
      "set",
      "aws",
      "key",
      `{"a": 1}`,
    ]);
  });

  it("keeps an empty quoted argument", () => {
    expect(tokenize(`runs rename 1 ""`)).toEqual(["runs", "rename", "1", ""]);
  });

  it("keeps backslashes inside single quotes", () => {
    expect(tokenize(String.raw`wdl check 'C:\packs\a.wdl'`)).toEqual([
      "wdl",
      "check",
      String.raw`C:\packs\a.wdl`,
    ]);
  });

  it("escapes a space outside quotes", () => {
    expect(tokenize(String.raw`wdl check my\ pack.wdl`)).toEqual(["wdl", "check", "my pack.wdl"]);
  });

  it("rejects an unterminated quote", () => {
    expect(() => tokenize(`settings set a b "unclosed`)).toThrow(ConsoleParseError);
  });
});
