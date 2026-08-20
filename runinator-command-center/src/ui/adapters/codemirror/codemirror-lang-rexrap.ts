// codemirror support for the runinator rexrap workflow dsl.

import { LanguageSupport, syntaxHighlighting } from "@codemirror/language";
import type { CompletionSource } from "@codemirror/autocomplete";
import { rexrapCompletion } from "./rexrap-completions";
import { rexrapHighlightStyle, rexrapParser } from "./rexrap-tokenizer";

export { rexrapCompletion, rexrapStaticCompletionLabels } from "./rexrap-completions";

/** codemirror language support for rexrap highlighting and completion. */
export function rexrap(providerCompletion?: CompletionSource): LanguageSupport {
  const autocomplete = rexrapParser.data.of({ autocomplete: providerCompletion ?? rexrapCompletion });
  return new LanguageSupport(rexrapParser, [syntaxHighlighting(rexrapHighlightStyle), autocomplete]);
}
