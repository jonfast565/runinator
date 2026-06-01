import { snippet, type Completion, type CompletionContext, type CompletionResult, type CompletionSource } from "@codemirror/autocomplete";
import { completeWdl } from "../api/commandCenterApi";
import type { ProviderMetadata, WdlCompletionRequest, WdlCompletionResponse } from "../types/models";

export function wdlProviderCompletionSource(providers: () => ProviderMetadata[]): CompletionSource {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const source = context.state.doc.toString();
    const request = buildWdlCompletionRequest(source, context.pos, providers());
    const response = await completeWdl(request);
    const result = completionResponseToCodeMirror(source, response);
    if (!result.options.length && !context.explicit) return null;
    return result;
  };
}

export function buildWdlCompletionRequest(source: string, cursorOffset: number, providers: ProviderMetadata[]): WdlCompletionRequest {
  return {
    source,
    cursor_byte: utf16OffsetToUtf8ByteOffset(source, cursorOffset),
    providers
  };
}

export function completionResponseToCodeMirror(source: string, response: WdlCompletionResponse): CompletionResult {
  return {
    from: utf8ByteOffsetToUtf16Offset(source, response.replace_start_byte),
    to: utf8ByteOffsetToUtf16Offset(source, response.replace_end_byte),
    options: response.items.map(itemToCompletion)
  };
}

export function utf16OffsetToUtf8ByteOffset(source: string, offset: number): number {
  return new TextEncoder().encode(source.slice(0, offset)).length;
}

export function utf8ByteOffsetToUtf16Offset(source: string, byteOffset: number): number {
  const bytes = new TextEncoder().encode(source);
  const clamped = Math.max(0, Math.min(byteOffset, bytes.length));
  return new TextDecoder().decode(bytes.slice(0, clamped)).length;
}

function itemToCompletion(item: WdlCompletionResponse["items"][number]): Completion {
  const completion: Completion = {
    label: item.label,
    type: item.kind,
    detail: item.detail ?? undefined,
    info: item.documentation ?? undefined
  };
  if (item.is_snippet) {
    completion.apply = snippet(item.insert_text);
  } else {
    completion.apply = item.insert_text;
  }
  return completion;
}
