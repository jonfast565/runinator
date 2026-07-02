import {
  analyzeWdl,
  completeWdl,
  formatWdl,
  hoverWdl,
} from "../api/commandCenterApi";
import type {
  CredentialSummary,
  ProviderMetadata,
  WdlCompletionRequest,
  WdlDiagnostic,
  WdlHoverRequest,
} from "../domain/models";
import type { AppService } from "./app";

export function createWdlLanguageService(app: AppService) {
  return {
    analyze(source: string, sourcePath?: string | null) {
      return app.runOperation("Analyzing WDL", () => analyzeWdl(source, sourcePath));
    },
    format(source: string) {
      return app.runOperation("Formatting WDL", () => formatWdl(source));
    },
    complete(request: WdlCompletionRequest) {
      return completeWdl(request);
    },
    hover(request: WdlHoverRequest) {
      return hoverWdl(request);
    },
    analyzeSilent(source: string, sourcePath?: string | null): Promise<WdlDiagnostic[]> {
      return analyzeWdl(source, sourcePath);
    },
    formatSilent(source: string): Promise<string> {
      return formatWdl(source);
    },
  };
}

export type WdlLanguageService = ReturnType<typeof createWdlLanguageService>;

export function settingRefsFromCredentials(settings: CredentialSummary[]) {
  return settings.map((setting) => ({
    scope: setting.scope,
    name: setting.name,
    kind: setting.kind ?? "secret",
  }));
}

export type { ProviderMetadata, WdlDiagnostic };
