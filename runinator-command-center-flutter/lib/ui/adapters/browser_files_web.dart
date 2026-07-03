import 'dart:html' as html;
import 'dart:typed_data';

void downloadTextFile(String fileName, String contents, [String mimeType = 'text/plain']) {
  downloadBlob(fileName, html.Blob([contents], mimeType));
}

void downloadBlob(String fileName, Object blob) {
  final htmlBlob = blob is html.Blob
      ? blob
      : blob is Uint8List
          ? html.Blob([blob])
          : html.Blob([blob]);
  final url = html.Url.createObjectUrlFromBlob(htmlBlob);
  final anchor = html.AnchorElement(href: url)
    ..download = fileName
    ..style.display = 'none';
  html.document.body?.append(anchor);
  anchor.click();
  anchor.remove();
  html.Url.revokeObjectUrl(url);
}

html.Blob bytesBlob(Uint8List bytes, [String mimeType = 'application/octet-stream']) =>
    html.Blob([bytes], mimeType);
