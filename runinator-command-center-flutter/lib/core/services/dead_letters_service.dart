// port of core/services/dead-letters.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' show listDeadLetters;
import '../domain/json.dart';
import 'app_service.dart';

part 'dead_letters_service.g.dart';

class DeadLettersService {
  const DeadLettersService(this._app);

  final AppNotifier _app;

  Future<List<JsonRecord>> list({String? channel, int limit = 200}) async {
    try {
      return await _app.runOperation('Loading dead letters', () => listDeadLetters(channel: channel, limit: limit));
    } catch (_) {
      return [];
    }
  }
}

@riverpod
DeadLettersService deadLettersService(Ref ref) => DeadLettersService(ref.watch(appProvider.notifier));
