// port of core/services/display-preferences.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'display_preferences_service.g.dart';

enum AppTheme { system, light, dark }

const String _themeKey = 'command-center.theme';
const String _defaultTabKey = 'command-center.defaultTab';

class DefaultTabOption {
  const DefaultTabOption({required this.value, required this.label});

  final String value;
  final String label;
}

const List<DefaultTabOption> defaultTabOptions = [
  DefaultTabOption(value: 'Workflows', label: 'Workflows'),
  DefaultTabOption(value: 'Runs', label: 'Runs'),
  DefaultTabOption(value: 'Providers', label: 'Providers'),
  DefaultTabOption(value: 'Replicas', label: 'Replicas'),
  DefaultTabOption(value: 'Approvals', label: 'Approvals'),
  DefaultTabOption(value: 'Notifications', label: 'Notifications'),
];

final List<String> _allowedTabs = defaultTabOptions.map((option) => option.value).toList();

class DisplayPreferencesState {
  const DisplayPreferencesState({required this.theme, required this.defaultTab});

  final AppTheme theme;
  final String defaultTab;

  DisplayPreferencesState copyWith({AppTheme? theme, String? defaultTab}) => DisplayPreferencesState(
        theme: theme ?? this.theme,
        defaultTab: defaultTab ?? this.defaultTab,
      );
}

/// core/ has no browser localStorage dependency; a concrete web platform adapter
/// (future UI pass) supplies one via [setDisplayPreferencesStorage]. with none
/// configured (as in a `dart test` run) this behaves like the ts source's
/// try/catch fallback path for unavailable storage.
String? Function(String key)? _storageReader;
void Function(String key, String value)? _storageWriter;

void setDisplayPreferencesStorage({
  String? Function(String key)? reader,
  void Function(String key, String value)? writer,
}) {
  _storageReader = reader;
  _storageWriter = writer;
}

String _readStored(String key, List<String> allowed, String fallback) {
  final stored = _storageReader?.call(key);
  return (stored != null && allowed.contains(stored)) ? stored : fallback;
}

void _writeStored(String key, String value) => _storageWriter?.call(key, value);

@riverpod
class DisplayPreferencesNotifier extends _$DisplayPreferencesNotifier {
  @override
  DisplayPreferencesState build() => DisplayPreferencesState(
        theme: AppTheme.values.firstWhere(
          (t) => t.name == _readStored(_themeKey, AppTheme.values.map((t) => t.name).toList(), 'system'),
          orElse: () => AppTheme.system,
        ),
        defaultTab: _readStored(_defaultTabKey, _allowedTabs, 'Workflows'),
      );

  void setTheme(AppTheme theme) {
    state = state.copyWith(theme: theme);
    _writeStored(_themeKey, theme.name);
  }

  void setDefaultTab(String defaultTab) {
    state = state.copyWith(defaultTab: defaultTab);
    _writeStored(_defaultTabKey, defaultTab);
  }
}
