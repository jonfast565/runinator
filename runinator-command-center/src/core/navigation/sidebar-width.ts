// width of the desktop sidebar rail. the rail is a grid track on `.app-shell`, so the width is
// published as the `--sidebar-width` custom property rather than set on the element itself.

export const SIDEBAR_WIDTH_STORAGE_KEY = "runinator.sidebar-width";
export const SIDEBAR_DEFAULT_WIDTH = 220;
export const SIDEBAR_MIN_WIDTH = 168;
export const SIDEBAR_MAX_WIDTH = 420;

/** clamps a requested width; on a narrow viewport the rail may not take more than 40% of it. */
export function clampSidebarWidth(width: number, viewportWidth = 0): number {
  if (!Number.isFinite(width)) {
    return SIDEBAR_DEFAULT_WIDTH;
  }

  const ceiling =
    viewportWidth > 0
      ? Math.min(SIDEBAR_MAX_WIDTH, Math.round(viewportWidth * 0.4))
      : SIDEBAR_MAX_WIDTH;
  // the minimum wins over the ceiling: a rail narrower than its labels is worse than a wide one.
  const max = Math.max(SIDEBAR_MIN_WIDTH, ceiling);
  return Math.min(max, Math.max(SIDEBAR_MIN_WIDTH, Math.round(width)));
}

/** reads a persisted width back, falling back to the default for a missing or junk value. */
export function parseSidebarWidth(stored: string | null, viewportWidth = 0): number {
  if (!stored) {
    return SIDEBAR_DEFAULT_WIDTH;
  }

  const parsed = Number(stored);

  if (!Number.isFinite(parsed) || parsed <= 0) {
    return SIDEBAR_DEFAULT_WIDTH;
  }

  return clampSidebarWidth(parsed, viewportWidth);
}
