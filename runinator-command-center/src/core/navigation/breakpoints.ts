export const BREAKPOINTS = {
  mobile: 640,
  tablet: 1024,
} as const;

export type ViewportMode = "mobile" | "tablet" | "desktop";

export function viewportModeForWidth(width: number): ViewportMode {
  if (width < BREAKPOINTS.mobile) {
    return "mobile";
  }

  if (width < BREAKPOINTS.tablet) {
    return "tablet";
  }

  return "desktop";
}
