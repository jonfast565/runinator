// port of core/navigation/breakpoints.ts.
//
// responsive layout breakpoints — keep in sync with `@media` literals in the
// future Flutter UI pass's theme/layout code.

class Breakpoints {
  /// stack split panes, narrower sidebar.
  static const int tablet = 1180;

  /// drawer nav, master-detail toggle, single-column forms.
  static const int mobile = 760;

  /// extra-tight toolbars, card tables.
  static const int compact = 480;
}

enum ViewportMode { desktop, tablet, mobile, compact }

ViewportMode viewportModeForWidth(double width) {
  if (width <= Breakpoints.compact) {
    return ViewportMode.compact;
  }

  if (width <= Breakpoints.mobile) {
    return ViewportMode.mobile;
  }

  if (width <= Breakpoints.tablet) {
    return ViewportMode.tablet;
  }

  return ViewportMode.desktop;
}
