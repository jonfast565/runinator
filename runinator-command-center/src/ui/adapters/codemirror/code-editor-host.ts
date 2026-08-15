import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { basicSetup } from "codemirror";
import type {
  TextEditorHost,
  TextEditorHostCreateOptions,
  TextEditorLanguage,
} from "../../../core/platform/text-editor";
import { osCodeMirrorTheme } from "./codemirror-theme";
import { loadForeignLanguageExtension } from "./foreign-language";

export function createCodeEditorHost(options: TextEditorHostCreateOptions): TextEditorHost {
  const editableCompartment = new Compartment();
  const languageCompartment = new Compartment();
  let view: EditorView | null = null;
  let disposeEditorTheme: (() => void) | null = null;
  let silentUpdate = false;
  let languageRequest = 0;

  async function configureLanguage(language: TextEditorLanguage) {
    const request = ++languageRequest;
    const extension = await loadForeignLanguageExtension(language);

    if (request !== languageRequest || !view) {
      return;
    }

    view.dispatch({ effects: languageCompartment.reconfigure(extension) });
  }

  return {
    mount(container) {
      const editorTheme = osCodeMirrorTheme();
      const state = EditorState.create({
        doc: options.value,
        extensions: [
          basicSetup,
          editorTheme.extension,
          languageCompartment.of([]),
          editableCompartment.of(EditorView.editable.of(!options.readonly)),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !silentUpdate) {
              options.onChange(update.state.doc.toString());
            }
          }),
          EditorView.theme({
            "&": { height: "100%" },
            ".cm-scroller": { overflow: "auto" },
          }),
        ],
      });

      view = new EditorView({ state, parent: container });
      disposeEditorTheme = editorTheme.install(view);
      void configureLanguage(options.language);
    },
    destroy() {
      languageRequest += 1;
      disposeEditorTheme?.();
      view?.destroy();
      view = null;
    },
    getValue() {
      return view?.state.doc.toString() ?? options.value;
    },
    setValue(value, silent = false) {
      if (!view || value === view.state.doc.toString()) {
        return;
      }

      silentUpdate = silent;
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
      silentUpdate = false;
    },
    setReadonly(readonly) {
      view?.dispatch({
        effects: editableCompartment.reconfigure(EditorView.editable.of(!readonly)),
      });
    },
    setLanguage(language: TextEditorLanguage) {
      void configureLanguage(language);
    },
    focus() {
      view?.focus();
    },
    goToPosition(line, column = 1) {
      if (!view) {
        return;
      }

      const targetLine = view.state.doc.line(Math.min(Math.max(line, 1), view.state.doc.lines));
      const position = Math.min(targetLine.from + Math.max(column - 1, 0), targetLine.to);
      view.dispatch({
        selection: { anchor: position },
        effects: EditorView.scrollIntoView(position, { y: "center" }),
      });
      view.focus();
    },
    getDiagnostics() {
      return [];
    },
  };
}
