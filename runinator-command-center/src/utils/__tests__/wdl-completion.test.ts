import { describe, expect, it } from "vitest";
import {
  buildWdlCompletionRequest,
  completionResponseToCodeMirror,
  utf16OffsetToUtf8ByteOffset,
  utf8ByteOffsetToUtf16Offset
} from "../wdl-completion";
import type { ProviderMetadata, WdlCompletionResponse } from "../../types/models";

describe("wdl completion adapter", () => {
  it("builds a compiler request with provider metadata and utf-8 cursor bytes", () => {
    const source = 'workflow "Snowman" v1 {\n  ☃.run\n}';
    const cursor = source.indexOf(".run");
    const providers = [provider()];

    const request = buildWdlCompletionRequest(source, cursor, providers);

    expect(request.source).toBe(source);
    expect(request.providers).toBe(providers);
    expect(request.cursor_byte).toBe(utf16OffsetToUtf8ByteOffset(source, cursor));
    expect(request.cursor_byte).toBeGreaterThan(cursor);
  });

  it("maps byte replacement ranges back to codemirror offsets", () => {
    const source = 'workflow "Snowman" v1 {\n  ☃.ru\n}';
    const replaceStart = source.indexOf("ru");
    const response: WdlCompletionResponse = {
      replace_start_byte: utf16OffsetToUtf8ByteOffset(source, replaceStart),
      replace_end_byte: utf16OffsetToUtf8ByteOffset(source, replaceStart + 2),
      items: [
        {
          label: "run",
          kind: "function",
          detail: "shell",
          documentation: "Run a command",
          insert_text: "run",
          is_snippet: false
        }
      ]
    };

    const result = completionResponseToCodeMirror(source, response);

    expect(result.from).toBe(replaceStart);
    expect(result.to).toBe(replaceStart + 2);
    expect(result.options[0]).toMatchObject({
      label: "run",
      type: "function",
      detail: "shell",
      apply: "run"
    });
  });

  it("converts snippet responses into codemirror apply functions", () => {
    const response: WdlCompletionResponse = {
      replace_start_byte: 0,
      replace_end_byte: 0,
      items: [
        {
          label: "token",
          kind: "property",
          detail: "required string",
          documentation: null,
          insert_text: "token: ${}",
          is_snippet: true
        }
      ]
    };

    const result = completionResponseToCodeMirror("", response);

    expect(result.options[0].label).toBe("token");
    expect(typeof result.options[0].apply).toBe("function");
  });

  it("round trips byte and editor offsets around non-ascii text", () => {
    const source = "a☃b";
    const offset = 2;
    const byteOffset = utf16OffsetToUtf8ByteOffset(source, offset);

    expect(byteOffset).toBe(4);
    expect(utf8ByteOffsetToUtf16Offset(source, byteOffset)).toBe(offset);
  });
});

function provider(): ProviderMetadata {
  return {
    name: "console",
    actions: [],
    metadata: { credential_scopes: [], contract: null }
  };
}
