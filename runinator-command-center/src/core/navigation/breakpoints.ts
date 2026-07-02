/** Responsive layout breakpoints — keep in sync with `@media` literals in `styles/layout.css`. */
export const BREAKPOINTS = {
  /** stack split panes, narrower sidebar. */
  tablet: 1180,
  /** drawer nav, master-detail toggle, single-column forms. */
  mobile: 760,
  /** extra-tight toolbars, card tables. */
  compact: 480,
} as const;

export type ViewportMode = "desktop" | "tablet" | "mobile" | "compact";

export function viewportModeForWidth(width: number): ViewportMode {
  if (width <= BREAKPOINTS.compact) {
    return "compact";
  }

  if (width <= BREAKPOINTS.mobile) {
    return "mobile";
  }

  if (width <= BREAKPOINTS.tablet) {
    return "tablet";
  }

  return "desktop";
}

export function breakpointCustomProperties(): Record<string, string> {
  return {
    "--breakpoint-tablet": `${String(BREAKPOINTS.tablet)}px`,
    "--breakpoint-mobile": `${String(BREAKPOINTS.mobile)}px`,
    "--breakpoint-compact": `${String(BREAKPOINTS.compact)}px`,
  };
}
