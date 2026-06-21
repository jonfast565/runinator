import type { Extension } from "@codemirror/state";
import { Compartment } from "@codemirror/state";
import { EditorView } from "codemirror";

export interface OsCodeMirrorTheme {
  extension: Extension;
  install(view: EditorView): () => void;
}

const darkTheme = EditorView.theme(
  {
    "&": {
      color: "var(--text)",
      backgroundColor: "var(--surface-sunken)"
    },
    ".cm-content": {
      caretColor: "var(--accent)"
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--accent)"
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "rgba(98, 162, 255, 0.28)"
    },
    ".cm-panels": {
      backgroundColor: "var(--surface)",
      color: "var(--text)"
    },
    ".cm-panels.cm-panels-top": {
      borderBottomColor: "var(--border-subtle)"
    },
    ".cm-panels.cm-panels-bottom": {
      borderTopColor: "var(--border-subtle)"
    },
    ".cm-searchMatch": {
      backgroundColor: "var(--warning-bg)",
      outlineColor: "var(--warning-fg)"
    },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: "var(--accent-soft)"
    },
    ".cm-activeLine": {
      backgroundColor: "var(--surface-hover)"
    },
    ".cm-gutters": {
      backgroundColor: "var(--surface-sunken)",
      color: "var(--text-muted)",
      borderRightColor: "var(--border-subtle)"
    },
    ".cm-activeLineGutter": {
      backgroundColor: "var(--surface-hover)"
    },
    ".cm-tooltip": {
      backgroundColor: "var(--surface)",
      color: "var(--text)",
      borderColor: "var(--border-strong)"
    },
    ".cm-tooltip-autocomplete ul li[aria-selected]": {
      backgroundColor: "var(--accent)",
      color: "#ffffff"
    },
    ".cm-diagnostic": {
      backgroundColor: "var(--surface)"
    },
    ".cm-diagnostic-error": {
      borderLeftColor: "var(--danger-solid)"
    },
    ".cm-diagnostic-warning": {
      borderLeftColor: "var(--warn-solid)"
    }
  },
  { dark: true }
);

export function osCodeMirrorTheme(): OsCodeMirrorTheme {
  const compartment = new Compartment();
  const media = window.matchMedia("(prefers-color-scheme: dark)");

  return {
    extension: compartment.of(media.matches ? darkTheme : []),
    install(view) {
      const onChange = (event: MediaQueryListEvent) => {
        view.dispatch({
          effects: compartment.reconfigure(event.matches ? darkTheme : [])
        });
      };

      media.addEventListener("change", onChange);
      return () => media.removeEventListener("change", onChange);
    }
  };
}
