import { describe, expect, it } from "vitest";
import { createZip } from "../zip";

// minimal store-only zip reader: walk the end-of-central-directory record and pull each entry back
// out by its local header so we can assert the writer round-trips names and contents.
async function readZip(blob: Blob): Promise<Map<string, string>> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  const view = new DataView(bytes.buffer);
  const eocd = bytes.length - 22;
  expect(view.getUint32(eocd, true)).toBe(0x06054b50);
  const count = view.getUint16(eocd + 10, true);
  let offset = view.getUint32(eocd + 16, true);
  const decoder = new TextDecoder();
  const out = new Map<string, string>();

  for (let i = 0; i < count; i += 1) {
    expect(view.getUint32(offset, true)).toBe(0x02014b50);
    const size = view.getUint32(offset + 20, true);
    const nameLen = view.getUint16(offset + 28, true);
    const extraLen = view.getUint16(offset + 30, true);
    const commentLen = view.getUint16(offset + 32, true);
    const localOffset = view.getUint32(offset + 42, true);
    const name = decoder.decode(bytes.subarray(offset + 46, offset + 46 + nameLen));
    // jump to the local header to find where the file data starts.
    expect(view.getUint32(localOffset, true)).toBe(0x04034b50);
    const localNameLen = view.getUint16(localOffset + 26, true);
    const localExtraLen = view.getUint16(localOffset + 28, true);
    const dataStart = localOffset + 30 + localNameLen + localExtraLen;
    out.set(name, decoder.decode(bytes.subarray(dataStart, dataStart + size)));
    offset += 46 + nameLen + extraLen + commentLen;
  }

  return out;
}

describe("createZip", () => {
  it("round-trips entry names and contents", async () => {
    const blob = createZip([
      { name: "pack.wdlm", content: '{"version":1}' },
      { name: "hello.wdl", content: "workflow Hello {}\n" },
    ]);
    const entries = await readZip(blob);
    expect(entries.get("pack.wdlm")).toBe('{"version":1}');
    expect(entries.get("hello.wdl")).toBe("workflow Hello {}\n");
  });

  it("preserves unicode content", async () => {
    const blob = createZip([{ name: "u.wdl", content: "héllo → wörld" }]);
    const entries = await readZip(blob);
    expect(entries.get("u.wdl")).toBe("héllo → wörld");
  });
});
