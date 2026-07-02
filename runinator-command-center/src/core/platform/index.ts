import { setCommandRuntime } from "../api/runtime";
import type { PlatformAdapter } from "./types";

let activePlatform: PlatformAdapter | null = null;
let activeTextEditorFactory: import("./text-editor").TextEditorHostFactory | null = null;

export function setPlatformAdapter(adapter: PlatformAdapter) {
  activePlatform = adapter;
  setCommandRuntime(adapter.runtime);
}

export function getPlatformAdapter(): PlatformAdapter {
  if (!activePlatform) {
    throw new Error("Platform adapter has not been configured. Import bootstrap before App.");
  }

  return activePlatform;
}

export function getPlatformAdapterOptional(): PlatformAdapter | null {
  return activePlatform;
}

export function setTextEditorHostFactory(factory: import("./text-editor").TextEditorHostFactory) {
  activeTextEditorFactory = factory;
}

export function getTextEditorHostFactory(): import("./text-editor").TextEditorHostFactory {
  if (!activeTextEditorFactory) {
    throw new Error("Text editor host factory has not been configured.");
  }

  return activeTextEditorFactory;
}

export type {
  ArtifactDownloadResult,
  ArtifactTransport,
  AuthStorage,
  FilePicker,
  PlatformAdapter,
  PlatformDialogs,
  ServiceDiscovery,
  ServiceStatusSnapshot,
} from "./types";

export type {
  TextEditorDiagnostic,
  TextEditorDiagnosticSeverity,
  TextEditorHost,
  TextEditorHostCreateOptions,
  TextEditorHostFactory,
  TextEditorLanguage,
} from "./text-editor";
