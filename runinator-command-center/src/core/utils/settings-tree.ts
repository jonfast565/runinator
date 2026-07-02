import type { CredentialSummary } from "../domain/models";

// a leaf is a concrete setting; a folder groups settings that share a dotted-path prefix.
export interface SettingsTreeLeaf {
  type: "leaf";
  label: string;
  path: string;
  setting: CredentialSummary;
}

export interface SettingsTreeFolder {
  type: "folder";
  label: string;
  path: string;
  children: SettingsTreeNode[];
}

export type SettingsTreeNode = SettingsTreeFolder | SettingsTreeLeaf;

interface FolderBuilder {
  folders: Map<string, FolderBuilder>;
  leaves: SettingsTreeLeaf[];
  path: string;
}

function newFolder(path: string): FolderBuilder {
  return { folders: new Map(), leaves: [], path };
}

// the dotted path of a setting is its scope segments followed by its name segments.
function settingSegments(setting: CredentialSummary): string[] {
  return `${setting.scope}.${setting.name}`
    .split(".")
    .map((segment) => segment.trim())
    .filter(Boolean);
}

function joinPath(prefix: string, segment: string): string {
  return prefix ? `${prefix}.${segment}` : segment;
}

function finalizeFolder(builder: FolderBuilder): SettingsTreeNode[] {
  const folders: SettingsTreeFolder[] = Array.from(builder.folders.values()).map((child) => ({
    type: "folder",
    label: child.path.slice(builder.path ? builder.path.length + 1 : 0),
    path: child.path,
    children: finalizeFolder(child),
  }));
  folders.sort((a, b) => a.label.localeCompare(b.label));
  const leaves = [...builder.leaves].sort((a, b) => a.label.localeCompare(b.label));
  // folders first, then leaves, each alphabetical.
  return [...folders, ...leaves];
}

// group flat settings into a collapsible dotted-path tree (config.<scope>.<name> shape).
export function buildSettingsTree(entries: CredentialSummary[]): SettingsTreeNode[] {
  const root = newFolder("");

  for (const setting of entries) {
    const segments = settingSegments(setting);

    if (segments.length === 0) {
      continue;
    }

    let cursor = root;

    for (let index = 0; index < segments.length - 1; index += 1) {
      const segment = segments[index];
      const path = joinPath(cursor.path, segment);
      let next = cursor.folders.get(segment);

      if (!next) {
        next = newFolder(path);
        cursor.folders.set(segment, next);
      }

      cursor = next;
    }

    const label = segments[segments.length - 1];
    cursor.leaves.push({ type: "leaf", label, path: joinPath(cursor.path, label), setting });
  }

  return finalizeFolder(root);
}
