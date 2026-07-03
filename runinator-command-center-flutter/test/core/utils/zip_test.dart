import 'dart:convert';
import 'dart:typed_data';

import 'package:runinator_command_center_flutter/core/utils/zip.dart';
import 'package:test/test.dart';

// minimal store-only zip reader: walk the end-of-central-directory record and pull each entry back
// out by its local header so we can assert the writer round-trips names and contents.
Map<String, String> readZip(Uint8List bytes) {
  final view = ByteData.sublistView(bytes);
  final eocd = bytes.length - 22;
  expect(view.getUint32(eocd, Endian.little), 0x06054b50);
  final count = view.getUint16(eocd + 10, Endian.little);
  var offset = view.getUint32(eocd + 16, Endian.little);
  final out = <String, String>{};

  for (var i = 0; i < count; i++) {
    expect(view.getUint32(offset, Endian.little), 0x02014b50);
    final size = view.getUint32(offset + 20, Endian.little);
    final nameLen = view.getUint16(offset + 28, Endian.little);
    final extraLen = view.getUint16(offset + 30, Endian.little);
    final commentLen = view.getUint16(offset + 32, Endian.little);
    final localOffset = view.getUint32(offset + 42, Endian.little);
    final name = utf8.decode(bytes.sublist(offset + 46, offset + 46 + nameLen));
    expect(view.getUint32(localOffset, Endian.little), 0x04034b50);
    final localNameLen = view.getUint16(localOffset + 26, Endian.little);
    final localExtraLen = view.getUint16(localOffset + 28, Endian.little);
    final dataStart = localOffset + 30 + localNameLen + localExtraLen;
    out[name] = utf8.decode(bytes.sublist(dataStart, dataStart + size));
    offset += 46 + nameLen + extraLen + commentLen;
  }

  return out;
}

void main() {
  group('createZip', () {
    test('round-trips entry names and contents', () {
      final archive = createZip([
        const ZipEntry(name: 'pack.wdlp', content: '{"version":1}'),
        const ZipEntry(name: 'hello.wdl', content: 'workflow Hello {}\n'),
      ]);
      final entries = readZip(archive);
      expect(entries['pack.wdlp'], '{"version":1}');
      expect(entries['hello.wdl'], 'workflow Hello {}\n');
    });

    test('preserves unicode content', () {
      final archive = createZip([const ZipEntry(name: 'u.wdl', content: 'héllo → wörld')]);
      final entries = readZip(archive);
      expect(entries['u.wdl'], 'héllo → wörld');
    });
  });
}
