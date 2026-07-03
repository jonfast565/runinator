import 'dart:convert';
import 'dart:typed_data';

import 'browser_files_stub.dart'
    if (dart.library.html) 'browser_files_web.dart' as impl;

void downloadTextFile(String fileName, String contents, [String mimeType = 'text/plain']) {
  impl.downloadTextFile(fileName, contents, mimeType);
}

void downloadBlob(String fileName, Object blob) {
  impl.downloadBlob(fileName, blob);
}

Uint8List zipBytes(Object blob) {
  if (blob is Uint8List) return blob;
  throw ArgumentError('Expected Uint8List blob');
}

Uint8List textToBytes(String text) => Uint8List.fromList(utf8.encode(text));
