import 'package:shared_preferences/shared_preferences.dart';

import '../../core/navigation/nav_config.dart';
import '../../core/services/app_service.dart';
import '../../core/services/display_preferences_service.dart';
import '../../core/services/orgs_service.dart';
import '../../core/services/workflows/runs.dart';
import '../shared/split_pane.dart';

class SharedPreferencesStorage implements WatchExpressionStorage {
  SharedPreferencesStorage(this._prefs);

  final SharedPreferences _prefs;

  String? read(String key) => _prefs.getString(key);

  Future<void> write(String key, String value) async {
    await _prefs.setString(key, value);
  }

  Future<void> remove(String key) async {
    await _prefs.remove(key);
  }

  @override
  int get length => _prefs.getKeys().length;

  @override
  String? keyAt(int index) {
    final keys = _prefs.getKeys().toList()..sort();
    return index >= 0 && index < keys.length ? keys[index] : null;
  }

  @override
  String? getItem(String key) => _prefs.getString(key);

  @override
  void setItem(String key, String value) {
    _prefs.setString(key, value);
  }
}

Future<void> configureStorage(SharedPreferences prefs) async {
  final storage = SharedPreferencesStorage(prefs);

  setNavStorageReader(storage.read);
  setNavStorageWriter = (key, value) {
    storage.write(key, value);
  };

  setDisplayPreferencesStorage(
    reader: storage.read,
    writer: (key, value) {
      storage.write(key, value);
    },
  );

  setOrgsStorage(
    reader: storage.read,
    writer: (key, value) {
      if (value == null) {
        storage.remove(key);
      } else {
        storage.write(key, value);
      }
    },
  );

  setWatchExpressionStorage(storage);

  setSplitPaneStorage(
    reader: storage.read,
    writer: (key, value) {
      storage.write(key, value);
    },
  );
}
