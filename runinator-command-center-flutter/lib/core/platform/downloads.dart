typedef DownloadTextFileFn = void Function(String fileName, String contents, [String mimeType]);
typedef DownloadBlobFn = void Function(String fileName, Object blob);

DownloadTextFileFn? _downloadTextFile;
DownloadBlobFn? _downloadBlob;

void setDownloadHandlers({DownloadTextFileFn? downloadTextFile, DownloadBlobFn? downloadBlob}) {
  _downloadTextFile = downloadTextFile;
  _downloadBlob = downloadBlob;
}

void downloadTextFile(String fileName, String contents, [String mimeType = 'text/plain']) {
  _downloadTextFile?.call(fileName, contents, mimeType);
}

void downloadBlob(String fileName, Object blob) {
  _downloadBlob?.call(fileName, blob);
}
