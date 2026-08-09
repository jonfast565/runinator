// codemirror support for the runinator wdl workflow dsl.

import { LanguageSupport, syntaxHighlighting } from "@codemirror/language";
import type { CompletionSource } from "@codemirror/autocomplete";
import { wdlCompletion } from "./wdl-completions";
import { wdlHighlightStyle, wdlParser } from "./wdl-tokenizer";

export { wdlCompletion, wdlStaticCompletionLabels } from "./wdl-completions";

/** codemirror language support for wdl highlighting and completion. */
export function wdl(providerCompletion?: CompletionSource): LanguageSupport {
  const autocomplete = wdlParser.data.of({ autocomplete: providerCompletion ?? wdlCompletion });
  return new LanguageSupport(wdlParser, [syntaxHighlighting(wdlHighlightStyle), autocomplete]);
}
