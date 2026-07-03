// port of core/utils/zip.ts.
//
// minimal, dependency-free zip writer (store / no compression). enough to bundle a few small text
// files (e.g. an exported .wdlp pack and its .wdl sources) into one downloadable archive.
//
// `Blob` is a browser type with no Flutter-independent equivalent; this returns
// the raw archive bytes (Uint8List) instead. the future UI pass wraps them in
// a Blob/download mechanism as needed.

import 'dart:convert';
import 'dart:typed_data';

class ZipEntry {
  const ZipEntry({required this.name, required this.content});

  final String name;

  /// String or Uint8List, mirroring the ts source's `string | Uint8Array`.
  final Object content;
}

final Uint32List _crcTable = _buildCrcTable();

Uint32List _buildCrcTable() {
  final table = Uint32List(256);

  for (var n = 0; n < 256; n++) {
    var c = n;

    for (var k = 0; k < 8; k++) {
      c = (c & 1) != 0 ? (0xedb88320 ^ (c >> 1)) : (c >> 1);
    }

    table[n] = c & 0xffffffff;
  }

  return table;
}

int _crc32(Uint8List bytes) {
  var crc = 0xffffffff;

  for (final byte in bytes) {
    crc = _crcTable[(crc ^ byte) & 0xff] ^ (crc >> 8);
  }

  return (crc ^ 0xffffffff) & 0xffffffff;
}

// dos-format modification time/date, as the zip local/central headers expect.
class _DosDateTime {
  const _DosDateTime(this.time, this.date);

  final int time;
  final int date;
}

_DosDateTime _dosDateTime(DateTime date) {
  final time = ((date.hour & 0x1f) << 11) | ((date.minute & 0x3f) << 5) | ((date.second >> 1) & 0x1f);
  final day = (((date.year - 1980) & 0x7f) << 9) | (((date.month) & 0x0f) << 5) | (date.day & 0x1f);
  return _DosDateTime(time, day);
}

Uint8List _toBytes(Object content) => content is String ? Uint8List.fromList(utf8.encode(content)) : content as Uint8List;

// build a store-only zip archive blob from the given entries.
Uint8List createZip(List<ZipEntry> entries) {
  final now = _dosDateTime(DateTime.now());
  final localChunks = <Uint8List>[];
  final centralChunks = <Uint8List>[];
  var offset = 0;

  for (final entry in entries) {
    final nameBytes = Uint8List.fromList(utf8.encode(entry.name));
    final data = _toBytes(entry.content);
    final crc = _crc32(data);

    final local = ByteData(30 + nameBytes.length);
    local.setUint32(0, 0x04034b50, Endian.little);
    local.setUint16(4, 20, Endian.little); // version needed
    local.setUint16(6, 0, Endian.little); // flags
    local.setUint16(8, 0, Endian.little); // method: store
    local.setUint16(10, now.time, Endian.little);
    local.setUint16(12, now.date, Endian.little);
    local.setUint32(14, crc, Endian.little);
    local.setUint32(18, data.length, Endian.little); // compressed size
    local.setUint32(22, data.length, Endian.little); // uncompressed size
    local.setUint16(26, nameBytes.length, Endian.little);
    local.setUint16(28, 0, Endian.little); // extra length
    final localBytes = Uint8List(30 + nameBytes.length)
      ..setRange(0, 30, local.buffer.asUint8List())
      ..setRange(30, 30 + nameBytes.length, nameBytes);

    final central = ByteData(46 + nameBytes.length);
    central.setUint32(0, 0x02014b50, Endian.little);
    central.setUint16(4, 20, Endian.little); // version made by
    central.setUint16(6, 20, Endian.little); // version needed
    central.setUint16(8, 0, Endian.little); // flags
    central.setUint16(10, 0, Endian.little); // method: store
    central.setUint16(12, now.time, Endian.little);
    central.setUint16(14, now.date, Endian.little);
    central.setUint32(16, crc, Endian.little);
    central.setUint32(20, data.length, Endian.little);
    central.setUint32(24, data.length, Endian.little);
    central.setUint16(28, nameBytes.length, Endian.little);
    central.setUint16(30, 0, Endian.little); // extra length
    central.setUint16(32, 0, Endian.little); // comment length
    central.setUint16(34, 0, Endian.little); // disk number start
    central.setUint16(36, 0, Endian.little); // internal attrs
    central.setUint32(38, 0, Endian.little); // external attrs
    central.setUint32(42, offset, Endian.little); // local header offset
    final centralBytes = Uint8List(46 + nameBytes.length)
      ..setRange(0, 46, central.buffer.asUint8List())
      ..setRange(46, 46 + nameBytes.length, nameBytes);

    localChunks.add(localBytes);
    localChunks.add(data);
    centralChunks.add(centralBytes);
    offset += localBytes.length + data.length;
  }

  final centralSize = centralChunks.fold<int>(0, (sum, chunk) => sum + chunk.length);
  final end = ByteData(22);
  end.setUint32(0, 0x06054b50, Endian.little);
  end.setUint16(4, 0, Endian.little); // disk number
  end.setUint16(6, 0, Endian.little); // disk with central directory
  end.setUint16(8, entries.length, Endian.little); // records on this disk
  end.setUint16(10, entries.length, Endian.little); // total records
  end.setUint32(12, centralSize, Endian.little);
  end.setUint32(16, offset, Endian.little); // central directory offset
  end.setUint16(20, 0, Endian.little); // comment length

  final chunks = [...localChunks, ...centralChunks, end.buffer.asUint8List()];
  final total = chunks.fold<int>(0, (sum, chunk) => sum + chunk.length);
  final combined = Uint8List(total);
  var cursor = 0;

  for (final chunk in chunks) {
    combined.setRange(cursor, cursor + chunk.length, chunk);
    cursor += chunk.length;
  }

  return combined;
}
