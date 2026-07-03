// port of core/services/wdl-language.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart';
import '../domain/models/index.dart';
import 'app_service.dart';

part 'wdl_language_service.g.dart';

class WdlLanguageService {
  const WdlLanguageService(this._app);

  final AppNotifier _app;

  Future<List<WdlDiagnostic>> analyze(String source, [String? sourcePath]) =>
      _app.runOperation('Analyzing WDL', () => analyzeWdl(source, sourcePath));

  Future<String> format(String source) => _app.runOperation('Formatting WDL', () => formatWdl(source));

  Future<WdlCompletionResponse> complete(WdlCompletionRequest request) => completeWdl(request);

  Future<WdlHoverResponse?> hover(WdlHoverRequest request) => hoverWdl(request);

  Future<List<WdlDiagnostic>> analyzeSilent(String source, [String? sourcePath]) => analyzeWdl(source, sourcePath);

  Future<String> formatSilent(String source) => formatWdl(source);
}

List<WdlSettingRef> settingRefsFromCredentials(List<CredentialSummary> settings) => settings
    .map((setting) => WdlSettingRef(scope: setting.scope, name: setting.name, kind: setting.kind ?? SettingKind.secret))
    .toList();

@riverpod
WdlLanguageService wdlLanguageService(Ref ref) => WdlLanguageService(ref.watch(appProvider.notifier));
