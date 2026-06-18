import { describe, expect, it } from "vitest";
import { CompletionContext, type CompletionSource } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { wdl, wdlCompletion, wdlStaticCompletionLabels } from "../codemirror-lang-wdl";
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

  it("maps semantic wdl kinds to distinct codemirror completion icon types", () => {
    const response: WdlCompletionResponse = {
      replace_start_byte: 0,
      replace_end_byte: 0,
      items: [
        {
          label: "ok",
          kind: "edge",
          detail: "success edge",
          documentation: null,
          insert_text: "ok -> ${target}",
          is_snippet: true
        },
        {
          label: "cleanup",
          kind: "node",
          detail: "node target",
          documentation: null,
          insert_text: "cleanup",
          is_snippet: false
        },
        {
          label: "console",
          kind: "provider",
          detail: "provider",
          documentation: null,
          insert_text: "console",
          is_snippet: false
        }
      ]
    };

    const result = completionResponseToCodeMirror("", response);

    expect(result.options.map((option) => [option.label, option.type])).toEqual([
      ["ok", "constant"],
      ["cleanup", "interface"],
      ["console", "namespace"]
    ]);
  });

  it("round trips byte and editor offsets around non-ascii text", () => {
    const source = "a☃b";
    const offset = 2;
    const byteOffset = utf16OffsetToUtf8ByteOffset(source, offset);

    expect(byteOffset).toBe(4);
    expect(utf8ByteOffsetToUtf16Offset(source, byteOffset)).toBe(offset);
  });
});

describe("wdl language completions", () => {
  it("includes recent workflow language surfaces in the static vocabulary", () => {
    expect(wdlStaticCompletionLabels).toEqual(expect.arrayContaining([
      "fn",
      "import std",
      "returns",
      "watch",
      "gate condition",
      "signal",
      "compensate",
      "enum",
      "range",
      "lambda"
    ]));
  });

  it("completes std modules and module functions without provider metadata", async () => {
    const modules = await completeLabels("workflow \"x\" { node compute { return std.<> }");
    expect(modules).toEqual(expect.arrayContaining(["strings", "collections", "exec"]));

    const functions = await completeLabels("workflow \"x\" { node compute { return std.strings.<> }");
    expect(functions).toEqual(expect.arrayContaining(["upper", "split"]));
    expect(functions).toContain("upper");
    expect(functions).not.toContain("http_get");
  });

  it("completes fluent method names after a value dot", async () => {
    const labels = await completeLabels("workflow \"x\" { node console.run(command: params.name.<> }");

    expect(labels).toEqual(expect.arrayContaining(["upper", "trim", "map", "http_get"]));
  });

  it("offers identifier completions during normal typing", async () => {
    const labels = await completeLabels("workflow \"x\" { wo", false);

    expect(labels).toEqual(expect.arrayContaining(["workflow", "watch"]));
  });

  it("offers dot completions during normal typing", async () => {
    const modules = await completeLabels("workflow \"x\" { node compute { return std.", false);
    expect(modules).toEqual(expect.arrayContaining(["strings", "collections", "exec"]));

    const functions = await completeLabels("workflow \"x\" { node compute { return std.strings.", false);
    expect(functions).toEqual(expect.arrayContaining(["upper", "split"]));
    expect(functions).not.toContain("http_get");
  });

  it("uses the rust-backed completion source when one is supplied", async () => {
    const source = "workflow \"x\" { node compute { return std.";
    const providerSource: CompletionSource = () => ({
      from: source.length,
      options: [{ label: "provider-sentinel" }]
    });
    const state = EditorState.create({
      doc: source,
      extensions: [wdl(providerSource)]
    });
    const sources = state.languageDataAt("autocomplete", source.length) as CompletionSource[];

    expect(sources).toHaveLength(1);

    const result = await sources[0](new CompletionContext(state, source.length, false));
    const labels = result?.options.map((option) => option.label) ?? [];
    expect(labels).toEqual(["provider-sentinel"]);
  });
});

async function completeLabels(source: string, explicit = true): Promise<string[]> {
  const cursor = source.indexOf("<>");
  const doc = cursor >= 0 ? source.replace("<>", "") : source;
  const state = EditorState.create({ doc });
  const result = await wdlCompletion(new CompletionContext(state, cursor >= 0 ? cursor : doc.length, explicit));
  return result?.options.map((option) => option.label) ?? [];
}

function provider(): ProviderMetadata {
  return {
    name: "console",
    actions: [],
    metadata: { credential_scopes: [], contract: null }
  };
}
