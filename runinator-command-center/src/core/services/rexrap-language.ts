import { analyzeRexRap, completeRexRap, formatRexRap, hoverRexRap } from "../api/commandCenterApi";
import type {
  CredentialSummary,
  ProviderMetadata,
  RexRapCompletionRequest,
  RexRapDocumentKind,
  RexRapDiagnostic,
  RexRapHoverRequest,
} from "../domain/models";
import type { AppService } from "./app";

export function createRexRapLanguageService(app: AppService) {
  return {
    analyze(source: string, sourcePath?: string | null, document: RexRapDocumentKind = "workflow") {
      return app.runOperation("Analyzing REXRAP", () =>
        analyzeRexRap(source, sourcePath, document),
      );
    },
    format(source: string, document: RexRapDocumentKind = "workflow") {
      return app.runOperation("Formatting REXRAP", () => formatRexRap(source, document));
    },
    complete(request: RexRapCompletionRequest) {
      return completeRexRap(request);
    },
    hover(request: RexRapHoverRequest) {
      return hoverRexRap(request);
    },
    analyzeSilent(
      source: string,
      sourcePath?: string | null,
      document: RexRapDocumentKind = "workflow",
    ): Promise<RexRapDiagnostic[]> {
      return analyzeRexRap(source, sourcePath, document);
    },
    formatSilent(source: string, document: RexRapDocumentKind = "workflow"): Promise<string> {
      return formatRexRap(source, document);
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
