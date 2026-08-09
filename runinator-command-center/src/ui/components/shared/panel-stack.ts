import type { IconName } from "../../../core/domain/icons";

/** one view in a `PanelStack`: an eclipse-style tab over a panel body. */
export interface PanelStackTab {
  id: string;
  label: string;
  /** tooltip; falls back to the label. */
  title?: string;
  icon?: IconName;
  /** a count rendered as a pill on the tab; falsy hides it. */
  badge?: number | string;
  badgeTone?: "error" | "warning" | "info";
}
