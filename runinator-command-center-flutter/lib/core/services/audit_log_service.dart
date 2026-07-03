// port of core/services/audit-log.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' show listAuditLog;
import '../domain/json.dart';
import 'app_service.dart';

part 'audit_log_service.g.dart';

class AuditLogService {
  const AuditLogService(this._app);

  final AppNotifier _app;

  Future<List<JsonRecord>> list({String? action, int limit = 200}) async {
    try {
      return await _app.runOperation('Loading audit log', () => listAuditLog(action: action, limit: limit));
    } catch (_) {
      return [];
    }
  }
}

@riverpod
AuditLogService auditLogService(Ref ref) => AuditLogService(ref.watch(appProvider.notifier));
