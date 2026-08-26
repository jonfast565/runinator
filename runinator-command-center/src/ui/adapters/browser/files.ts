export function downloadBlob(fileName: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

export function downloadTextFile(fileName: string, contents: string, mimeType = "text/plain") {
  downloadBlob(fileName, new Blob([contents], { type: mimeType }));
}

export function pickFileFromBrowser(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.style.display = "none";
    document.body.appendChild(input);
    let settled = false;
    input.addEventListener("change", () => {
      settled = true;
      const file = input.files?.[0] ?? null;
      input.remove();
      resolve(file);
    });

    window.addEventListener("focus", function onFocus() {
      window.removeEventListener("focus", onFocus);
      setTimeout(() => {
        if (settled) {
          return;
        }

        input.remove();
        resolve(null);
      }, 250);
    });
    input.click();
  });
}
