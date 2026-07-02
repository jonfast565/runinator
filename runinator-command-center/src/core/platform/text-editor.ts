export type TextEditorLanguage = "wdl" | "json" | "expression";

export type TextEditorDiagnosticSeverity = "error" | "warning" | "info";

export interface TextEditorDiagnostic {
  severity: TextEditorDiagnosticSeverity;
  message: string;
  line: number;
  column: number;
}

export interface TextEditorHostCreateOptions {
  language: TextEditorLanguage;
  value: string;
  readonly?: boolean;
  sourcePath?: string | null;
  onChange(value: string): void;
  onDiagnosticsChange?(diagnostics: TextEditorDiagnostic[]): void;
}

/** Framework-neutral editor surface; CodeMirror implements this in ui/adapters/codemirror/. */
export interface TextEditorHost {
  mount(container: HTMLElement): void;
  destroy(): void;
  getValue(): string;
  setValue(value: string, silent?: boolean): void;
  setReadonly(readonly: boolean): void;
  focus(): void;
  goToPosition(line: number, column?: number): void;
  getDiagnostics(): TextEditorDiagnostic[];
  formatDocument?(): Promise<void>;
}

export interface TextEditorHostFactory {
  create(options: TextEditorHostCreateOptions): TextEditorHost;
}
