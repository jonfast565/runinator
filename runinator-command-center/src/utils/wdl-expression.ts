import type { JsonRecord } from "../types/models";

type TokenKind = "ident" | "string" | "number" | "op" | "punct" | "eof";

interface Token {
  kind: TokenKind;
  text: string;
}

const expressionKeys = ["$ref", "$concat", "$coalesce", "$literal", "$to_string", "$to_json_string", "$node"];

export function expressionJsonToWdl(value: unknown): string {
  if (value === null) return "null";
  if (typeof value === "boolean" || typeof value === "number") return String(value);
  if (typeof value === "string") return secretRefToWdl(value) ?? quote(value);
  if (Array.isArray(value)) return `[${value.map(expressionJsonToWdl).join(", ")}]`;
  if (!isRecord(value)) return "null";

  const keys = Object.keys(value);
  if (keys.length === 1) {
    if (isRecord(value.$ref)) return refToWdl(value.$ref);
    if (Array.isArray(value.$concat)) return joinOperator(value.$concat, " ++ ");
    if (Array.isArray(value.$coalesce)) return joinOperator(value.$coalesce, " ?? ");
    if (Object.prototype.hasOwnProperty.call(value, "$to_string")) return `string(${expressionJsonToWdl(value.$to_string)})`;
    if (Object.prototype.hasOwnProperty.call(value, "$to_json_string")) return `json(${expressionJsonToWdl(value.$to_json_string)})`;
  }

  const entries = Object.entries(value).map(([key, nested]) => `${objectKey(key)}: ${expressionJsonToWdl(nested)}`);
  return `{ ${entries.join(", ")} }`;
}

export function parseWdlExpression(source: string): unknown {
  const parser = new Parser(tokenize(source));
  const value = parser.parseExpression();
  parser.expect("eof");
  return value;
}

export function isWorkflowExpressionValue(value: unknown): value is JsonRecord {
  return isRecord(value) && expressionKeys.some((key) => Object.prototype.hasOwnProperty.call(value, key));
}

function joinOperator(items: unknown[], operator: string): string {
  return items.map((item) => wrapBinaryPart(expressionJsonToWdl(item))).join(operator);
}

function wrapBinaryPart(value: string): string {
  return /\s(?:\+\+|\?\?)\s/.test(value) ? `(${value})` : value;
}

function refToWdl(ref: JsonRecord): string {
  if (Array.isArray(ref.params)) return appendPath("params", ref.params);
  if (Array.isArray(ref.prev)) return appendPath("prev", ref.prev);
  if (Array.isArray(ref.workflow)) return appendPath("run", ref.workflow);
  if (Array.isArray(ref.config)) return appendPath("config", ref.config);
  if (typeof ref.node === "string" && Array.isArray(ref.output)) return appendPath(ref.node, ref.output);
  return objectLiteral(ref);
}

function appendPath(head: string, path: unknown[]): string {
  return [head, ...path.map((segment) => String(segment))].join(".");
}

function objectLiteral(record: JsonRecord): string {
  const entries = Object.entries(record).map(([key, value]) => `${objectKey(key)}: ${expressionJsonToWdl(value)}`);
  return `{ ${entries.join(", ")} }`;
}

function objectKey(key: string): string {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(key) ? key : quote(key);
}

function quote(value: string): string {
  return JSON.stringify(value);
}

function secretRefToWdl(value: string): string | null {
  if (!value.startsWith("secret://")) return null;
  const rest = value.slice("secret://".length);
  const [scope, ...name] = rest.split("/");
  if (!scope || name.length === 0 || ![scope, ...name].every((part) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(part))) return null;
  return ["secret", scope, ...name].join(".");
}

function tokenize(source: string): Token[] {
  const tokens: Token[] = [];
  let index = 0;
  while (index < source.length) {
    const char = source[index];
    if (/\s/.test(char)) {
      index += 1;
      continue;
    }
    if (source.startsWith("++", index) || source.startsWith("??", index)) {
      tokens.push({ kind: "op", text: source.slice(index, index + 2) });
      index += 2;
      continue;
    }
    if ("{}[]():,.".includes(char)) {
      tokens.push({ kind: "punct", text: char });
      index += 1;
      continue;
    }
    if (char === "\"") {
      const start = index;
      index += 1;
      let escaped = false;
      while (index < source.length) {
        const next = source[index++];
        if (escaped) {
          escaped = false;
          continue;
        }
        if (next === "\\") {
          escaped = true;
          continue;
        }
        if (next === "\"") break;
      }
      tokens.push({ kind: "string", text: source.slice(start, index) });
      continue;
    }
    const number = source.slice(index).match(/^-?\d+(?:\.\d+)?/);
    if (number) {
      tokens.push({ kind: "number", text: number[0] });
      index += number[0].length;
      continue;
    }
    const ident = source.slice(index).match(/^[A-Za-z_$][A-Za-z0-9_$-]*/);
    if (ident) {
      tokens.push({ kind: "ident", text: ident[0] });
      index += ident[0].length;
      continue;
    }
    throw new Error(`Unexpected character ${char}`);
  }
  tokens.push({ kind: "eof", text: "" });
  return tokens;
}

class Parser {
  private index = 0;

  constructor(private readonly tokens: Token[]) {}

  parseExpression(): unknown {
    return this.parseCoalesce();
  }

  expect(kind: TokenKind, text?: string): Token {
    const token = this.peek();
    if (token.kind !== kind || (text !== undefined && token.text !== text)) {
      throw new Error(text ? `Expected ${text}` : `Expected ${kind}`);
    }
    this.index += 1;
    return token;
  }

  private parseCoalesce(): unknown {
    const parts = [this.parseConcat()];
    while (this.match("op", "??")) parts.push(this.parseConcat());
    return parts.length === 1 ? parts[0] : { "$coalesce": parts };
  }

  private parseConcat(): unknown {
    const parts = [this.parsePrimary()];
    while (this.match("op", "++")) parts.push(this.parsePrimary());
    return parts.length === 1 ? parts[0] : { "$concat": parts };
  }

  private parsePrimary(): unknown {
    const token = this.peek();
    if (this.match("punct", "(")) {
      const value = this.parseExpression();
      this.expect("punct", ")");
      return value;
    }
    if (this.match("punct", "{")) return this.parseObject();
    if (this.match("punct", "[")) return this.parseArray();
    if (token.kind === "string") {
      this.index += 1;
      return JSON.parse(token.text);
    }
    if (token.kind === "number") {
      this.index += 1;
      return token.text.includes(".") ? Number.parseFloat(token.text) : Number.parseInt(token.text, 10);
    }
    if (token.kind === "ident") return this.parseIdentPrimary();
    throw new Error("Expected expression");
  }

  private parseIdentPrimary(): unknown {
    const head = this.expect("ident").text;
    if (head === "true") return true;
    if (head === "false") return false;
    if (head === "null") return null;
    if ((head === "string" || head === "json") && this.match("punct", "(")) {
      const nested = this.parseExpression();
      this.expect("punct", ")");
      return head === "string" ? { "$to_string": nested } : { "$to_json_string": nested };
    }
    const path = [head];
    while (this.match("punct", ".")) path.push(this.expectPathSegment());
    return lowerPath(path);
  }

  private parseObject(): JsonRecord {
    const record: JsonRecord = {};
    while (!this.match("punct", "}")) {
      const keyToken = this.peek();
      let key: string;
      if (keyToken.kind === "string") {
        key = JSON.parse(this.expect("string").text);
      } else {
        key = this.expect("ident").text;
      }
      let value: unknown;
      if (this.match("punct", ":")) {
        value = this.parseExpression();
      } else {
        value = lowerPath([key]);
      }
      record[key] = value;
      if (this.match("punct", ",")) continue;
      this.expect("punct", "}");
      break;
    }
    return record;
  }

  private parseArray(): unknown[] {
    const items: unknown[] = [];
    while (!this.match("punct", "]")) {
      items.push(this.parseExpression());
      if (this.match("punct", ",")) continue;
      this.expect("punct", "]");
      break;
    }
    return items;
  }

  private expectPathSegment(): string {
    const token = this.peek();
    if (token.kind !== "ident" && token.kind !== "number") throw new Error("Expected path segment");
    this.index += 1;
    return token.text;
  }

  private match(kind: TokenKind, text?: string): boolean {
    const token = this.peek();
    if (token.kind !== kind || (text !== undefined && token.text !== text)) return false;
    this.index += 1;
    return true;
  }

  private peek(): Token {
    return this.tokens[this.index] ?? { kind: "eof", text: "" };
  }
}

function lowerPath(path: string[]): unknown {
  const [head, ...rest] = path;
  if (head === "params" || head === "prev" || head === "config") return { "$ref": { [head]: pathSegments(rest) } };
  if (head === "run" || head === "workflow") return { "$ref": { workflow: pathSegments(rest) } };
  if (head === "secret" && rest.length >= 2) return `secret://${rest[0]}/${rest.slice(1).join("/")}`;
  return { "$ref": { node: head, output: pathSegments(rest) } };
}

function pathSegments(path: string[]): Array<string | number> {
  return path.map((segment) => (/^\d+$/.test(segment) ? Number.parseInt(segment, 10) : segment));
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
