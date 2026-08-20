import {
  analyzeRexRap,
  completeRexRap,
  formatRexRap,
  hoverRexRap,
} from "../api/commandCenterApi";
import type {
  CredentialSummary,
  ProviderMetadata,
  RexRapCompletionRequest,
  RexRapDiagnostic,
  RexRapHoverRequest,
} from "../domain/models";
import type { AppService } from "./app";

export function createRexRapLanguageService(app: AppService) {
  return {
    analyze(source: string, sourcePath?: string | null) {
      return app.runOperation("Analyzing REXRAP", () => analyzeRexRap(source, sourcePath));
    },
    format(source: string) {
      return app.runOperation("Formatting REXRAP", () => formatRexRap(source));
    },
    complete(request: RexRapCompletionRequest) {
      return completeRexRap(request);
    },
    hover(request: RexRapHoverRequest) {
      return hoverRexRap(request);
    },
    analyzeSilent(source: string, sourcePath?: string | null): Promise<RexRapDiagnostic[]> {
      return analyzeRexRap(source, sourcePath);
    },
    formatSilent(source: string): Promise<string> {
      return formatRexRap(source);
    },
  };
}

export type RexRapLanguageService = ReturnType<typeof createRexRapLanguageService>;

export function settingRefsFromCredentials(settings: CredentialSummary[]) {
  return settings.map((setting) => ({
    scope: setting.scope,
    name: setting.name,
    kind: setting.kind ?? "secret",
  }));
}

export type { ProviderMetadata, RexRapDiagnostic };
