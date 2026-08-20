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
import { rexrapLanguageService } from "../../../core/services";
import type {
  CredentialSummary,
  ProviderMetadata,
  RexRapDiagnostic,
  RexRapSettingRef,
} from "../../../core/domain/models";
import { osCodeMirrorTheme } from "./codemirror-theme";
import { rexrap } from "./codemirror-lang-rexrap";
import { jsonCompletionSource, shouldStartJsonCompletion } from "./json-completion";
import { rexrapProviderCompletionSource } from "./rexrap-completion";
import { rexrapHoverTooltip } from "./rexrap-hover";
import { createCodeEditorHost } from "./code-editor-host";

const REXRAP_LINT_DELAY_MS = 1500;

interface RexRapHostContext {
  providers: () => ProviderMetadata[];
  settings: () => RexRapSettingRef[];
  sourcePath?: string | null;
}

interface CodeMirrorHostOptions extends TextEditorHostCreateOptions {
  rexrapContext?: RexRapHostContext;
  jsonKeyHints?: () => string[];
}

function toTextDiagnostics(diagnostics: RexRapDiagnostic[]): TextEditorDiagnostic[] {
  return diagnostics.map((diagnostic) => ({
    severity: diagnostic.severity,
    message: diagnostic.message,
    line: diagnostic.line,
    column: diagnostic.column,
  }));
}

function createRexRapHost(options: CodeMirrorHostOptions): TextEditorHost {
  const rexrapContext = options.rexrapContext ?? {
    providers: () => [],
    settings: () => [],
    sourcePath: options.sourcePath,
  };
  const editableCompartment = new Compartment();
  let view: EditorView | null = null;
  let disposeEditorTheme: (() => void) | null = null;
  let diagnostics: RexRapDiagnostic[] = [];
  let diagnosticsRequest = 0;
  let silentUpdate = false;

  async function refreshDiagnostics(source: string): Promise<RexRapDiagnostic[]> {
    const request = ++diagnosticsRequest;

    try {
      const nextDiagnostics = await rexrapLanguageService.analyzeSilent(
        source,
        rexrapContext.sourcePath ?? options.sourcePath,
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

  const rexrapLinter = linter(
    async (linterView): Promise<Diagnostic[]> => {
      const source = linterView.state.doc.toString();
      const docLength = linterView.state.doc.length;
      let nextDiagnostics: RexRapDiagnostic[];

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
    { delay: REXRAP_LINT_DELAY_MS },
  );

  function settingRefsFromCredentials(settings: CredentialSummary[]): RexRapSettingRef[] {
    return settings.map((setting) => ({
      scope: setting.scope,
      name: setting.name,
      kind: setting.kind ?? "secret",
    }));
  }

  function providers() {
    return rexrapContext.providers();
  }

  function settings() {
    const raw = rexrapContext.settings();

    if (raw.length && "scope" in raw[0]) {
      return raw;
    }

    return settingRefsFromCredentials(raw);
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
          rexrap(rexrapProviderCompletionSource(providers, settings)),
          rexrapHoverTooltip(providers, settings),
          rexrapLinter,
          editableCompartment.of(EditorView.editable.of(!options.readonly)),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !silentUpdate) {
              options.onChange(update.state.doc.toString());
            }

            if (!options.readonly && shouldStartRexRapCompletion(update)) {
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
            ".rexrap-hover": {
              maxWidth: "420px",
              padding: "8px 10px",
              fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
              fontSize: "12px",
              lineHeight: "1.35",
              color: "var(--text)",
            },
            ".rexrap-hover-title": {
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
              fontWeight: "700",
              color: "var(--text)",
            },
            ".rexrap-hover-meta": {
              marginTop: "3px",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
              color: "var(--text-muted)",
            },
            ".rexrap-hover-docs": {
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
      const formatted = await rexrapLanguageService.formatSilent(source);
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

function shouldStartRexRapCompletion(update: ViewUpdate): boolean {
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

      if (options.language === "rexrap" || options.language === "expression") {
        return createRexRapHost(hostOptions);
      }

      if (options.language !== "json") {
        return createCodeEditorHost(options);
      }

      return createJsonHost(hostOptions);
    },
  };
}

export type { RexRapHostContext, CodeMirrorHostOptions };
