import type { DevPackFile } from "../../core/domain/models";

export const DEV_OPTIONS_STORAGE_KEY = "runinator.devPack.options";
const RECENT_PACKS_STORAGE_KEY = "runinator.devPack.recentPaths";

export function loadDevOptions(): Record<string, unknown> {
  try {
    return JSON.parse(window.localStorage.getItem(DEV_OPTIONS_STORAGE_KEY) ?? "{}") as Record<
      string,
      unknown
    >;
  } catch {
    return {};
  }
}

export function loadRecentPacks(): string[] {
  try {
    return JSON.parse(window.localStorage.getItem(RECENT_PACKS_STORAGE_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}

export function fingerprint(files: DevPackFile[]): string {
  return files
    .map((file) => `${file.path}:${file.modified_at ?? ""}:${String(file.size_bytes ?? "")}`)
    .join("|");
}

export function relativePackPath(path: string, manifestPath: string): string {
  const root = manifestPath.replace(/\/[^/]*$/, "");
  return path.startsWith(root) ? path.slice(root.length + 1) || path : path;
}

export function fileMeta(file: DevPackFile): string {
  const size = file.size_bytes == null ? "-" : `${String(file.size_bytes)}b`;
  const time = file.modified_at ? new Date(file.modified_at).toLocaleTimeString() : "-";
  return `${size} · ${time}`;
}
