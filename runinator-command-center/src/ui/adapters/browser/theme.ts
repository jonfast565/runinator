import type { AppTheme } from "../../../core/services/display-preferences";

let mediaCleanup: (() => void) | null = null;

export function applyTheme(theme: AppTheme) {
  if (mediaCleanup) {
    mediaCleanup();
    mediaCleanup = null;
  }

  if (theme === "system") {
    const media = window.matchMedia("(prefers-color-scheme: dark)");

    const update = () => {
      document.documentElement.setAttribute("data-theme", media.matches ? "dark" : "light");
    };

    update();
    media.addEventListener("change", update);

    mediaCleanup = () => {
      media.removeEventListener("change", update);
    };
  } else {
    document.documentElement.setAttribute("data-theme", theme);
  }
}
