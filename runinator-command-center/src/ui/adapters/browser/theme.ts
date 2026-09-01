import type { AppTheme } from "../../../core/services/display-preferences";

let mediaCleanup: (() => void) | null = null;

function setResolvedTheme(theme: "light" | "dark") {
  document.documentElement.setAttribute("data-theme", theme);
}

function listenForSystemTheme(media: MediaQueryList, update: () => void) {
  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", update);

    return () => {
      media.removeEventListener("change", update);
    };
  }

  // Safari 13 and older expose the original MediaQueryList listener API.
  const legacyMedia = media as unknown as {
    addListener: (listener: () => void) => void;
    removeListener: (listener: () => void) => void;
  };
  legacyMedia.addListener(update);

  return () => {
    legacyMedia.removeListener(update);
  };
}

export function applyTheme(theme: AppTheme) {
  if (mediaCleanup) {
    mediaCleanup();
    mediaCleanup = null;
  }

  if (theme === "system") {
    const media = window.matchMedia("(prefers-color-scheme: dark)");

    const update = () => {
      setResolvedTheme(media.matches ? "dark" : "light");
    };

    update();
    mediaCleanup = listenForSystemTheme(media, update);
  } else {
    setResolvedTheme(theme);
  }
}
