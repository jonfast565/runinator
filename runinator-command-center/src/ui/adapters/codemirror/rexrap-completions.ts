// static keyword and snippet completion for the rexrap editor.

import {
  snippetCompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSource,
} from "@codemirror/autocomplete";
import { KEYWORDS, STD_INTRINSICS, STD_MODULES } from "./rexrap-vocabulary";

// keyword + snippet completion. provider/action-aware completion is supplied by the
// command-center editor as an async source backed by runinator-rexrap.
const keywordCompletions = [...KEYWORDS].map((label) => ({ label, type: "keyword" }));

const snippets = [
  snippetCompletion("if ${condition} {\n\t${}\n} else {\n\t${}\n}", {
    label: "if",
    type: "keyword",
    detail: "if/else",
  }),
  snippetCompletion("for ${item} in ${collection} {\n\t${}\n}", {
    label: "for",
    type: "keyword",
    detail: "for loop",
  }),
  snippetCompletion('workflow "${name}" {\n\tparams {\n\t\t${}\n\t}\n\n\tdo {\n\t\t${}\n\t}\n}', {
    label: "workflow",
    type: "keyword",
    detail: "workflow scaffold",
  }),
  snippetCompletion("let ${name} = ${provider}.${action}(${args})", {
    label: "action",
    type: "function",
    detail: "provider action node",
  }),
  snippetCompletion("let ${name} = async ${provider}.${action}(${args})", {
    label: "async",
    type: "keyword",
    detail: "schedule a call as a task",
  }),
  snippetCompletion("routes {\n\ton success {\n\t\tcontinue ${target}\n\t}\n}", {
    label: "routes",
    type: "keyword",
    detail: "attached routes section",
  }),
  snippetCompletion("join ${name} {\n\t${}\n}", {
    label: "join",
    type: "keyword",
    detail: "named continuation",
  }),
  snippetCompletion("task fn ${name}(${arg}: ${type}) do {\n\t${}\n}", {
    label: "task fn",
    type: "function",
    detail: "runtime function inlined at each call site",
  }),
  snippetCompletion("fn ${name}(${arg}: ${type}) -> ${return_type} = ${value}", {
    label: "fn",
    type: "function",
    detail: "function definition",
  }),
  snippetCompletion("import std.${module} as ${alias}", {
    label: "import std",
    type: "keyword",
    detail: "standard-library import",
  }),
  snippetCompletion('trigger cron "${cron}" with { ${} }', {
    label: "trigger cron",
    type: "keyword",
    detail: "cron trigger",
  }),
  snippetCompletion('trigger on_success workflow "${target}"', {
    label: "trigger on_success",
    type: "keyword",
    detail: "chained trigger",
  }),
  snippetCompletion("interrupt on wake {\n    ${}\n    resume\n}", {
    label: "interrupt",
    detail: "interrupt handler region",
    type: "keyword",
  }),
  snippetCompletion("resume", {
    label: "resume",
    detail: "return control from an interrupt handler",
    type: "keyword",
  }),
  snippetCompletion("watch ${condition} -> ${target}", {
    label: "watch",
    type: "keyword",
    detail: "workflow guard",
  }),
  snippetCompletion("gate condition when ${condition} every ${interval} timeout ${deadline}", {
    label: "gate condition",
    type: "keyword",
    detail: "condition gate",
  }),
  snippetCompletion('signal "${name}" key ${correlation}', {
    label: "signal",
    type: "keyword",
    detail: "external signal wait",
  }),
  snippetCompletion("compensate ${provider}.${action}(${args})", {
    label: "compensate",
    type: "keyword",
    detail: "compensating action",
  }),
  snippetCompletion('assert {\n\t"${name}": ${condition}\n}', {
    label: "assert",
    type: "keyword",
    detail: "invariant assertions",
  }),
  snippetCompletion("transform {\n\t${name} = ${expr}\n}", {
    label: "transform",
    type: "keyword",
    detail: "data reshape bindings",
  }),
  snippetCompletion('audit action "${action}" actor ${actor}', {
    label: "audit",
    type: "keyword",
    detail: "compliance audit record",
  }),
  snippetCompletion('checkpoint "${name}"', {
    label: "checkpoint",
    type: "keyword",
    detail: "named state snapshot",
  }),
  snippetCompletion('mutex "${name}" {\n\t${body}\n}', {
    label: "mutex",
    type: "keyword",
    detail: "cross-run exclusive lock (critical section)",
  }),
  snippetCompletion('throttle "${name}" rate ${n} per ${window}', {
    label: "throttle",
    type: "keyword",
    detail: "cross-run rate limiter",
  }),
  snippetCompletion('cooldown "${name}" every ${window}', {
    label: "cooldown",
    type: "keyword",
    detail: "cross-run cooldown; one pass per window",
  }),
  snippetCompletion('await workflow "${name}" key ${correlation} mode "all"', {
    label: "await",
    type: "keyword",
    detail: "wait for run(s) of a named workflow",
  }),
  snippetCompletion("correlate key ${expr}", {
    label: "correlate",
    type: "keyword",
    detail: "declare this run's correlation key",
  }),
  snippetCompletion('debounce "${name}" delay ${delay}', {
    label: "debounce",
    type: "keyword",
    detail: "trailing-delay debounce",
  }),
  snippetCompletion('collect "${name}" max ${count} timeout ${deadline}', {
    label: "collect",
    type: "keyword",
    detail: "timed accumulator",
  }),
  snippetCompletion('barrier "${name}" count ${n} timeout ${deadline}', {
    label: "barrier",
    type: "keyword",
    detail: "multi-run rendezvous",
  }),
  snippetCompletion(
    'circuit_breaker "${name}" threshold ${n} window ${window} cooldown ${cooldown}',
    {
      label: "circuit_breaker",
      type: "keyword",
      detail: "cross-run failure guard",
    },
  ),
  snippetCompletion('event_source type "${event_type}" max ${count} timeout ${deadline}', {
    label: "event_source",
    type: "keyword",
    detail: "stream-driven iteration",
  }),
  snippetCompletion("type ${Name} {\n\t${field}: ${type}\n}", {
    label: "type struct",
    type: "type",
    detail: "named struct type",
  }),
  snippetCompletion("enum[${value}]", {
    label: "enum",
    type: "type",
    detail: "enum type",
  }),
  snippetCompletion("${integer} range ${0}..${10}", {
    label: "range",
    type: "type",
    detail: "bounded type",
  }),
  snippetCompletion("${item} => ${expr}", {
    label: "lambda",
    type: "function",
    detail: "lambda expression",
  }),
];

const moduleCompletions: Completion[] = STD_MODULES.map((label) => ({
  label,
  type: "module",
  detail: "std module",
}));

const intrinsicCompletions: Completion[] = STD_INTRINSICS.map(({ label, module }) => ({
  label,
  type: "function",
  detail: `std.${module}.${label}`,
}));

function intrinsicCompletionsFor(module: string): Completion[] {
  return intrinsicCompletions.filter(
    (completion) => completion.detail === `std.${module}.${completion.label}`,
  );
}

export const rexrapStaticCompletionLabels = [
  ...new Set([
    ...snippets.map((completion) => completion.label),
    ...keywordCompletions.map((completion) => completion.label),
    ...moduleCompletions.map((completion) => completion.label),
    ...intrinsicCompletions.map((completion) => completion.label),
  ]),
].sort();

export const rexrapCompletion: CompletionSource = (
  context: CompletionContext,
): CompletionResult | null => {
  const word = context.matchBefore(/[A-Za-z_][A-Za-z0-9_-]*/);
  const tokenStart = word?.from ?? context.pos;
  const beforeToken = context.state.sliceDoc(0, tokenStart);
  const stdModule = /\bstd\.([A-Za-z_][A-Za-z0-9_]*)\.$/.exec(beforeToken);
  const afterDot = beforeToken.endsWith(".");

  if (!context.explicit && !word && !afterDot) {
    return null;
  }

  if (stdModule) {
    return {
      from: tokenStart,
      options: intrinsicCompletionsFor(stdModule[1]),
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  }

  if (/\bstd\.$/.test(beforeToken)) {
    return {
      from: tokenStart,
      options: moduleCompletions,
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  }

  if (afterDot) {
    return {
      from: tokenStart,
      options: intrinsicCompletions,
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  }

  return {
    from: word?.from ?? context.pos,
    options: [...snippets, ...keywordCompletions],
    validFor: /^[A-Za-z_][A-Za-z0-9_-]*$/,
  };
};
