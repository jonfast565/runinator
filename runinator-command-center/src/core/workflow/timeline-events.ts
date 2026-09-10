import type { WorkflowNodeRun } from "../domain/models";

export interface TimelineEventCategory {
  id: string;
  label: string;
  title: string;
}

const KNOWN_CATEGORIES: Partial<Record<string, TimelineEventCategory>> = {
  user: {
    id: "user",
    label: "User",
    title: "An authored workflow lifecycle event.",
  },
  system: {
    id: "system",
    label: "System",
    title: "A runtime-managed workflow lifecycle event.",
  },
};

function normalizeCategoryId(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value.trim().toLowerCase() : null;
}

function categoryLabel(id: string): string {
  return id.replaceAll(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

/** Resolve a timeline row to a category. Unknown explicit categories remain visible and filterable. */
export function timelineEventCategory(node: WorkflowNodeRun): TimelineEventCategory {
  const explicit =
    normalizeCategoryId(node.timeline_category) ??
    normalizeCategoryId(node.state?.timeline_category);
  const id = explicit ?? (typeof node.state?.workspace_phase === "string" ? "system" : "user");
  const known = KNOWN_CATEGORIES[id];

  if (known) {
    return known;
  }

  const label = categoryLabel(id);
  return {
    id,
    label,
    title: `Workflow lifecycle event categorized as ${label}.`,
  };
}
