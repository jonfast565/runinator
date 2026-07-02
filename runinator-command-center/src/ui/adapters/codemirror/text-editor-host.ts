import { autocompletion, completionKeymap, startCompletion } from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import { linter, type Diagnostic } from "@codemirror/lint";
import { Compartment, EditorState, Prec } from "@codemirror/state";
import { EditorView, keymap, type ViewUpdate } from "@codemirror/view";
import { basicSetup } from "codemirror";
import type {
  TextEditorDiagnostic,
  TextEditorHost,
  TextEditorHostCreateOptions,
  TextEditorHostFactory,
} from "../../../core/platform/text-editor";
import { wdlLanguageService } from "../../../core/services";
import type { CredentialSummary, ProviderMetadata, WdlDiagnostic, WdlSettingRef } from "../../../types/models";
import { osCodeMirrorTheme } from "../../../utils/codemirror-theme";
import { wdl } from "../../../utils/codemirror-lang-wdl";
import { jsonCompletionSource, shouldStartJsonCompletion } from "../../../utils/json-completion";
import { wdlProviderCompletionSource } from "./wdl-completion";
import { wdlHoverTooltip } from "./wdl-hover";

const WDL_LINT_DELAY_MS = 1500;

interface WdlHostContext {
  providers: () => ProviderMetadata[];
  settings: () => WdlSettingRef[];
  sourcePath?: string | null;
}

interface CodeMirrorHostOptions extends TextEditorHostCreateOptions {
  wdlContext?: WdlHostContext;
  jsonKeyHints?: () => string[];
}

function toTextDiagnostics(diagnostics: WdlDiagnostic[]): TextEditorDiagnostic[] {
  return diagnostics.map((diagnostic) => ({
    severity: diagnostic.severity,
    message: diagnostic.message,
    line: diagnostic.line,
    column: diagnostic.column,
  }));
}

function createWdlHost(options: CodeMirrorHostOptions): TextEditorHost {
  const wdlContext = options.wdlContext ?? {
    providers: () => [],
    settings: () => [],
    sourcePath: options.sourcePath,
  };
  const editableCompartment = new Compartment();
  let view: EditorView | null = null;
  let disposeEditorTheme: (() => void) | null = null;
  let diagnostics: WdlDiagnostic[] = [];
  let diagnosticsRequest = 0;
  let silentUpdate = false;

  async function refreshDiagnostics(source: string): Promise<WdlDiagnostic[]> {
    const request = ++diagnosticsRequest;

    try {
      const nextDiagnostics = await wdlLanguageService.analyzeSilent(
        source,
        wdlContext.sourcePath ?? options.sourcePath,
      );

      if (request === diagnosticsRequest) {
        diagnostics = nextDiagnostics;
        options.onDiagnosticsChange?.(toTextDiagnostics(nextDiagnostics));
      }

      return nextDiagnostics;
    } catch {
      return [];
    }
  }

  const wdlLinter = linter(
    async (linterView): Promise<Diagnostic[]> => {
      const source = linterView.state.doc.toString();
      const docLength = linterView.state.doc.length;
      let nextDiagnostics: WdlDiagnostic[];

      try {
        nextDiagnostics = await refreshDiagnostics(source);
      } catch {
        return [];
      }

      return nextDiagnostics.map((diagnostic) => {
        const from = Math.min(Math.max(diagnostic.start, 0), docLength);
        let to = Math.min(Math.max(diagnostic.end, from), docLength);

        if (to <= from) {
          to = Math.min(from + 1, docLength);
        }

        return {
          from,
          to,
          severity: diagnostic.severity === "warning" ? "warning" : "error",
          message: diagnostic.message,
        };
      });
    },
    { delay: WDL_LINT_DELAY_MS },
  );

  function settingRefsFromCredentials(settings: CredentialSummary[]): WdlSettingRef[] {
    return settings.map((setting) => ({
      scope: setting.scope,
      name: setting.name,
      kind: setting.kind ?? "secret",
    }));
  }

  function providers() {
    return wdlContext.providers();
  }

  function settings() {
    const raw = wdlContext.settings();

    if (raw.length && "scope" in raw[0]) {
      return raw as WdlSettingRef[];
    }

    return settingRefsFromCredentials(raw as CredentialSummary[]);
  }

  const host: TextEditorHost = {
    mount(container) {
      const editorTheme = osCodeMirrorTheme();
      const startState = EditorState.create({
        doc: options.value,
        extensions: [
          basicSetup,
          editorTheme.extension,
          Prec.high(
            keymap.of([
              ...completionKeymap,
              {
                key: "Tab",
                run(editor) {
                  if (options.readonly) {
                    return false;
                  }

                  editor.dispatch(editor.state.replaceSelection("    "));
                  return true;
                },
              },
            ]),
          ),
          wdl(wdlProviderCompletionSource(providers, settings)),
          wdlHoverTooltip(providers, settings),
          wdlLinter,
          editableCompartment.of(EditorView.editable.of(!options.readonly)),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !silentUpdate) {
              options.onChange(update.state.doc.toString());
            }

            if (!options.readonly && shouldStartWdlCompletion(update)) {
              startCompletion(update.view);
            }
          }),
          EditorView.theme({
            "&": { height: "100%" },
            ".cm-scroller": { overflow: "auto" },
            ".cm-tooltip": {
              border: "1px solid var(--border-strong)",
              borderRadius: "6px",
              boxShadow: "var(--workflow-menu-shadow)",
            },
            ".wdl-hover": {
              maxWidth: "420px",
              padding: "8px 10px",
              fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
              fontSize: "12px",
              lineHeight: "1.35",
              color: "var(--text)",
            },
            ".wdl-hover-title": {
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
              fontWeight: "700",
              color: "var(--text)",
            },
            ".wdl-hover-meta": {
              marginTop: "3px",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
              color: "var(--text-muted)",
            },
            ".wdl-hover-docs": {
              marginTop: "7px",
              color: "var(--text-subtle)",
              whiteSpace: "pre-line",
            },
          }),
        ],
      });

      view = new EditorView({
        state: startState,
        parent: container,
      });
      disposeEditorTheme = editorTheme.install(view);
    },
    destroy() {
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
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: value },
      });
      silentUpdate = false;
    },
    setReadonly(readonly) {
      view?.dispatch({
        effects: editableCompartment.reconfigure(EditorView.editable.of(!readonly)),
      });
    },
    focus() {
      view?.focus();
    },
    goToPosition(line, column = 1) {
      if (!view) {
        return;
      }

      const diagnostic = diagnostics.find(
        (entry) => entry.line === line && entry.column === column,
      );
      const position = diagnostic
        ? Math.min(Math.max(diagnostic.start, 0), view.state.doc.length)
        : lineColumnToOffset(view.state.doc.toString(), line, column);

      view.dispatch({
        selection: { anchor: position },
        effects: EditorView.scrollIntoView(position, { y: "center" }),
      });
      view.focus();
    },
    getDiagnostics() {
      return toTextDiagnostics(diagnostics);
    },
    async formatDocument() {
      if (!view || options.readonly) {
        return;
      }

      const source = view.state.doc.toString();
      const formatted = await wdlLanguageService.formatSilent(source);
      host.setValue(formatted);
      options.onChange(formatted);
      await refreshDiagnostics(formatted);
    },
  };

  return host;
}

function createJsonHost(options: CodeMirrorHostOptions): TextEditorHost {
  const editableCompartment = new Compartment();
  let view: EditorView | null = null;
  let disposeEditorTheme: (() => void) | null = null;
  let silentUpdate = false;

  const host: TextEditorHost = {
    mount(container) {
      const editorTheme = osCodeMirrorTheme();
      const keyHints = options.jsonKeyHints ?? (() => []);

      const startState = EditorState.create({
        doc: options.value,
        extensions: [
          basicSetup,
          json(),
          editorTheme.extension,
          autocompletion({
            override: [jsonCompletionSource(() => ({ keyHints: keyHints() }))],
          }),
          keymap.of(completionKeymap),
          editableCompartment.of(EditorView.editable.of(!options.readonly)),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !silentUpdate) {
              options.onChange(update.state.doc.toString());
            }

            if (!options.readonly && shouldStartJsonCompletion(update)) {
              startCompletion(update.view);
            }
          }),
          EditorView.theme({
            "&": { height: "100%" },
            ".cm-scroller": { overflow: "auto" },
          }),
        ],
      });

      view = new EditorView({
        state: startState,
        parent: container,
      });
      disposeEditorTheme = editorTheme.install(view);
    },
    destroy() {
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
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: value },
      });
      silentUpdate = false;
    },
    setReadonly(readonly) {
      view?.dispatch({
        effects: editableCompartment.reconfigure(EditorView.editable.of(!readonly)),
      });
    },
    focus() {
      view?.focus();
    },
    goToPosition(line, column = 1) {
      if (!view) {
        return;
      }

      const position = lineColumnToOffset(view.state.doc.toString(), line, column);
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

  return host;
}

function lineColumnToOffset(source: string, line: number, column: number): number {
  const lines = source.split("\n");
  const lineIndex = Math.max(0, Math.min(line - 1, lines.length - 1));
  let offset = 0;

  for (let index = 0; index < lineIndex; index += 1) {
    offset += lines[index].length + 1;
  }

  return offset + Math.max(0, column - 1);
}

function shouldStartWdlCompletion(update: ViewUpdate): boolean {
  if (!update.docChanged) {
    return false;
  }

  if (!update.transactions.some((transaction) => transaction.isUserEvent("input"))) {
    return false;
  }

  const head = update.state.selection.main.head;

  if (head <= 0) {
    return false;
  }

  const previous = update.state.sliceDoc(head - 1, head);
  return /[\w.]/.test(previous);
}

export function createCodeMirrorTextEditorHostFactory(): TextEditorHostFactory {
  return {
    create(options: TextEditorHostCreateOptions): TextEditorHost {
      const hostOptions = options as CodeMirrorHostOptions;

      if (options.language === "wdl" || options.language === "expression") {
        return createWdlHost(hostOptions);
      }

      return createJsonHost(hostOptions);
    },
  };
}

export type { WdlHostContext, CodeMirrorHostOptions };
