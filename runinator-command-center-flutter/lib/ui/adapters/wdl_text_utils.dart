import 'dart:convert';

int utf16OffsetToUtf8ByteOffset(String source, int offset) {
  return utf8.encode(source.substring(0, offset.clamp(0, source.length))).length;
}

int utf8ByteOffsetToUtf16Offset(String source, int byteOffset) {
  final bytes = utf8.encode(source);
  final clamped = byteOffset.clamp(0, bytes.length);
  return utf8.decode(bytes.sublist(0, clamped)).length;
}
