export type PointerResult = {
  value: unknown;
  exists: boolean;
  error?: string;
};

/// Resolve a JSON pointer (RFC 6901) or dotted/bracketed path against a value.
/// Accepts "/foo/bar", "foo.bar", "foo[0].bar". Returns { exists: false } when
/// any segment is missing; never throws.
export function evaluatePointer(input: unknown, pointer: string): PointerResult {
  const trimmed = pointer.trim();
  if (!trimmed) return { value: input, exists: true };

  const segments = parseSegments(trimmed);
  let current: any = input;
  for (const seg of segments) {
    if (current == null) return { value: undefined, exists: false };
    if (typeof current !== "object") return { value: undefined, exists: false };
    if (!(seg in current)) return { value: undefined, exists: false };
    current = current[seg];
  }
  return { value: current, exists: true };
}

function parseSegments(pointer: string): string[] {
  if (pointer.startsWith("/")) {
    return pointer
      .slice(1)
      .split("/")
      .map((s) => s.replace(/~1/g, "/").replace(/~0/g, "~"));
  }
  const segments: string[] = [];
  let buf = "";
  for (let i = 0; i < pointer.length; i++) {
    const ch = pointer[i];
    if (ch === ".") {
      if (buf) segments.push(buf);
      buf = "";
    } else if (ch === "[") {
      if (buf) segments.push(buf);
      buf = "";
      const end = pointer.indexOf("]", i);
      if (end < 0) break;
      const inner = pointer.slice(i + 1, end);
      segments.push(inner.replace(/^['"]|['"]$/g, ""));
      i = end;
    } else {
      buf += ch;
    }
  }
  if (buf) segments.push(buf);
  return segments;
}
