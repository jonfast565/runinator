// minimal, dependency-free zip writer (store / no compression). enough to bundle a few small text
// files (e.g. an exported .wdlp pack and its .wdl sources) into one downloadable archive.

export interface ZipEntry {
  name: string;
  content: string | Uint8Array;
}

const crcTable = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) {
    crc = crcTable[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

// dos-format modification time/date, as the zip local/central headers expect.
function dosDateTime(date: Date): { time: number; date: number } {
  const time = ((date.getHours() & 0x1f) << 11) | ((date.getMinutes() & 0x3f) << 5) | ((date.getSeconds() >> 1) & 0x1f);
  const day = (((date.getFullYear() - 1980) & 0x7f) << 9) | (((date.getMonth() + 1) & 0x0f) << 5) | (date.getDate() & 0x1f);
  return { time, date: day };
}

function toBytes(content: string | Uint8Array): Uint8Array {
  return typeof content === "string" ? new TextEncoder().encode(content) : content;
}

// build a store-only zip archive blob from the given entries.
export function createZip(entries: ZipEntry[]): Blob {
  const now = dosDateTime(new Date());
  const encoder = new TextEncoder();
  const localChunks: Uint8Array[] = [];
  const centralChunks: Uint8Array[] = [];
  let offset = 0;

  for (const entry of entries) {
    const nameBytes = encoder.encode(entry.name);
    const data = toBytes(entry.content);
    const crc = crc32(data);

    const local = new DataView(new ArrayBuffer(30 + nameBytes.length));
    local.setUint32(0, 0x04034b50, true);
    local.setUint16(4, 20, true); // version needed
    local.setUint16(6, 0, true); // flags
    local.setUint16(8, 0, true); // method: store
    local.setUint16(10, now.time, true);
    local.setUint16(12, now.date, true);
    local.setUint32(14, crc, true);
    local.setUint32(18, data.length, true); // compressed size
    local.setUint32(22, data.length, true); // uncompressed size
    local.setUint16(26, nameBytes.length, true);
    local.setUint16(28, 0, true); // extra length
    const localBytes = new Uint8Array(local.buffer);
    localBytes.set(nameBytes, 30);

    const central = new DataView(new ArrayBuffer(46 + nameBytes.length));
    central.setUint32(0, 0x02014b50, true);
    central.setUint16(4, 20, true); // version made by
    central.setUint16(6, 20, true); // version needed
    central.setUint16(8, 0, true); // flags
    central.setUint16(10, 0, true); // method: store
    central.setUint16(12, now.time, true);
    central.setUint16(14, now.date, true);
    central.setUint32(16, crc, true);
    central.setUint32(20, data.length, true);
    central.setUint32(24, data.length, true);
    central.setUint16(28, nameBytes.length, true);
    central.setUint16(30, 0, true); // extra length
    central.setUint16(32, 0, true); // comment length
    central.setUint16(34, 0, true); // disk number start
    central.setUint16(36, 0, true); // internal attrs
    central.setUint32(38, 0, true); // external attrs
    central.setUint32(42, offset, true); // local header offset
    const centralBytes = new Uint8Array(central.buffer);
    centralBytes.set(nameBytes, 46);

    localChunks.push(localBytes, data);
    centralChunks.push(centralBytes);
    offset += localBytes.length + data.length;
  }

  const centralSize = centralChunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(4, 0, true); // disk number
  end.setUint16(6, 0, true); // disk with central directory
  end.setUint16(8, entries.length, true); // records on this disk
  end.setUint16(10, entries.length, true); // total records
  end.setUint32(12, centralSize, true);
  end.setUint32(16, offset, true); // central directory offset
  end.setUint16(20, 0, true); // comment length

  const chunks = [...localChunks, ...centralChunks, new Uint8Array(end.buffer)];
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const combined = new Uint8Array(total);
  let cursor = 0;
  for (const chunk of chunks) {
    combined.set(chunk, cursor);
    cursor += chunk.length;
  }
  return new Blob([combined], { type: "application/zip" });
}
