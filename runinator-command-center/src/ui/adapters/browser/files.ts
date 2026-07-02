import { apiBaseUrl } from "../../../core/api/httpRuntime";
import type { RunArtifact } from "../../../core/domain/models";
import type { ArtifactUploadRequest } from "../../../core/api/commandCenterApi";

export async function uploadArtifactFromBrowser(request: ArtifactUploadRequest, file: File) {
  const form = new FormData();
  form.set("run_id", request.run_id);
  form.set("name", file.name);
  form.set("mime_type", file.type || "application/octet-stream");

  if (request.workflow_node_run_id != null) {
    form.set("workflow_node_run_id", request.workflow_node_run_id);
  }

  form.set("file", file, file.name);
  const response = await fetch(`${apiBaseUrl()}/artifacts/upload`, { method: "POST", body: form });

  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`POST artifacts/upload -> ${String(response.status)}: ${text}`);
  }

  return (await response.json()) as RunArtifact;
}

export async function downloadArtifactInBrowser(artifactId: string, defaultName: string) {
  const response = await fetch(`${apiBaseUrl()}/artifacts/${artifactId}/download`);

  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`GET artifacts/${artifactId}/download -> ${String(response.status)}: ${text}`);
  }

  const blob = await response.blob();
  downloadBlob(defaultName, blob);
}

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
