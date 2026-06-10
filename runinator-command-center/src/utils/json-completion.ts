import { snippet, type Completion, type CompletionContext, type CompletionResult, type CompletionSource } from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import type { ViewUpdate } from "@codemirror/view";

interface JsonSyntaxNode {
  name: string;
  from: number;
  to: number;
  parent?: JsonSyntaxNode | null;
  firstChild?: JsonSyntaxNode | null;
  nextSibling?: JsonSyntaxNode | null;
}

export interface JsonCompletionHints {
  keyHints?: string[];
}

export function jsonCompletionSource(getHints: () => JsonCompletionHints = () => ({})): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    const state = context.state;
    const doc = state.doc;
    const hints = normalizeHints(getHints().keyHints);
    const node = syntaxTree(state).resolveInner(Math.max(0, context.pos - 1), -1) as JsonSyntaxNode;
    const objectNode = findAncestor(node, "Object");
    const arrayNode = findAncestor(node, "Array");
    const quoteMatch = context.matchBefore(/"[^"]*$/);

    if (objectNode && (quoteMatch || context.explicit) && (quoteMatch ? isObjectKeyContext(doc, quoteMatch.from) : !isValueContext(doc, context.pos, objectNode != null, arrayNode != null))) {
      const localKeys = collectObjectKeys(objectNode, doc);
      const globalKeys = collectPropertyNames(syntaxTree(state).topNode as JsonSyntaxNode, doc);
      const prefix = quoteMatch ? doc.sliceString(quoteMatch.from + 1, context.pos) : "";
      const options = toCompletionOptions(mergeUnique(localKeys, hints, globalKeys), prefix, "property");
      if (options.length || context.explicit) {
        return { from: quoteMatch ? quoteMatch.from + 1 : context.pos, options, validFor: /^[^"]*$/ };
      }
    }

    if (isValueContext(doc, context.pos, objectNode != null, arrayNode != null) || context.explicit) {
      const valuePrefix = context.matchBefore(/[\w.-]*$/);
      const from = valuePrefix?.from ?? context.pos;
      const options = valueCompletionOptions();
      if (options.length || context.explicit) {
        return { from, options, validFor: /^[\w.-]*$/ };
      }
    }

    return null;
  };
}

export function shouldStartJsonCompletion(update: ViewUpdate): boolean {
  if (!update.docChanged) return false;
  if (!update.transactions.some((transaction) => transaction.isUserEvent("input"))) return false;
  const head = update.state.selection.main.head;
  if (head <= 0) return false;
  const previous = update.state.sliceDoc(head - 1, head);
  return /["{}\[\]:,A-Za-z0-9_.-]/.test(previous);
}

function valueCompletionOptions(): Completion[] {
  return [
    literalCompletion("true", "boolean"),
    literalCompletion("false", "boolean"),
    literalCompletion("null", "null"),
    snippetCompletion("{ }", "{\n\t$0\n}", "type", "object literal"),
    snippetCompletion("[ ]", "[\n\t$0\n]", "type", "array literal")
  ];
}

function literalCompletion(label: string, detail: string): Completion {
  return { label, type: "keyword", detail, apply: label };
}

function snippetCompletion(label: string, applyText: string, type: Completion["type"], detail: string): Completion {
  return {
    label,
    type,
    detail,
    apply: snippet(applyText)
  };
}

function toCompletionOptions(keys: string[], prefix: string, type: Completion["type"]): Completion[] {
  const lowerPrefix = prefix.toLowerCase();
  return keys
    .filter((key) => !prefix || key.toLowerCase().startsWith(lowerPrefix))
    .map((key) => ({
      label: key,
      type,
      apply: key
    }));
}

function collectObjectKeys(objectNode: JsonSyntaxNode, doc: { sliceString(from: number, to: number): string }): string[] {
  const keys: string[] = [];
  for (let child = objectNode.firstChild; child; child = child.nextSibling) {
    if (child.name !== "PropertyName") continue;
    const value = unquote(doc.sliceString(child.from, child.to));
    if (value) keys.push(value);
  }
  return keys;
}

function collectPropertyNames(root: JsonSyntaxNode, doc: { sliceString(from: number, to: number): string }): string[] {
  const names: string[] = [];
  const seen = new Set<string>();

  walk(root);
  return names;

  function walk(node: JsonSyntaxNode) {
    if (node.name === "PropertyName") {
      const value = unquote(doc.sliceString(node.from, node.to));
      if (value && !seen.has(value)) {
        seen.add(value);
        names.push(value);
      }
    }
    for (let child = node.firstChild; child; child = child.nextSibling) walk(child);
  }
}

function findAncestor(node: JsonSyntaxNode | null | undefined, name: string): JsonSyntaxNode | null {
  for (let current = node; current; current = current.parent ?? null) {
    if (current.name === name) return current;
  }
  return null;
}

function isObjectKeyContext(doc: { sliceString(from: number, to: number): string }, quoteStart: number): boolean {
  return previousMeaningfulChar(doc, quoteStart) !== ":";
}

function isValueContext(
  doc: { sliceString(from: number, to: number): string },
  pos: number,
  insideObject: boolean,
  insideArray: boolean
): boolean {
  const previous = previousMeaningfulChar(doc, pos);
  if (!previous) return insideObject || insideArray;
  if (previous === ":" || previous === "," || previous === "[" || previous === "{") return true;
  return false;
}

function previousMeaningfulChar(doc: { sliceString(from: number, to: number): string }, pos: number): string {
  for (let index = pos - 1; index >= 0; index -= 1) {
    const char = doc.sliceString(index, index + 1);
    if (!/\s/.test(char)) return char;
  }
  return "";
}

function unquote(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length < 2 || trimmed[0] !== '"' || trimmed[trimmed.length - 1] !== '"') return trimmed;
  return trimmed.slice(1, -1);
}

function mergeUnique(...groups: Array<string[] | undefined>): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const group of groups) {
    for (const value of group ?? []) {
      if (!value || seen.has(value)) continue;
      seen.add(value);
      out.push(value);
    }
  }
  return out;
}

function normalizeHints(values: string[] | undefined): string[] {
  return mergeUnique(values);
}
