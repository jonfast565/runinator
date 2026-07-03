// port of core/services/admin-settings.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import 'app_service.dart';

part 'admin_settings_service.g.dart';

const String _languageScope = 'foreign_languages';

class ForeignLanguageSetting {
  const ForeignLanguageSetting({
    required this.language,
    required this.label,
    required this.aliases,
    required this.defaultImage,
    required this.image,
    required this.setupScript,
  });

  final String language;
  final String label;
  final List<String> aliases;
  final String defaultImage;
  final String image;
  final String setupScript;

  ForeignLanguageSetting copyWith({String? image, String? setupScript}) => ForeignLanguageSetting(
        language: language,
        label: label,
        aliases: aliases,
        defaultImage: defaultImage,
        image: image ?? this.image,
        setupScript: setupScript ?? this.setupScript,
      );
}

class _LanguageDefinition {
  const _LanguageDefinition(this.language, this.label, this.aliases, this.defaultImage);

  final String language;
  final String label;
  final List<String> aliases;
  final String defaultImage;
}

const List<_LanguageDefinition> _languageDefinitions = [
  _LanguageDefinition('python', 'Python', ['py'], 'python:3.12'),
  _LanguageDefinition('javascript', 'JavaScript', ['js', 'node'], 'node:22'),
  _LanguageDefinition('bash', 'Bash', ['sh'], 'bash:5.2'),
  _LanguageDefinition('ruby', 'Ruby', ['rb'], 'ruby:3.3'),
  _LanguageDefinition('perl', 'Perl', ['pl'], 'perl:5.40'),
  _LanguageDefinition('php', 'PHP', [], 'php:8.3-cli'),
];

List<ForeignLanguageSetting> createLanguageSettings() => _languageDefinitions
    .map((d) => ForeignLanguageSetting(
          language: d.language,
          label: d.label,
          aliases: d.aliases,
          defaultImage: d.defaultImage,
          image: d.defaultImage,
          setupScript: '',
        ))
    .toList();

class AdminSettingsState {
  const AdminSettingsState({required this.loaded, required this.languages});

  final bool loaded;
  final List<ForeignLanguageSetting> languages;
}

@riverpod
class AdminSettingsNotifier extends _$AdminSettingsNotifier {
  @override
  AdminSettingsState build() => AdminSettingsState(loaded: false, languages: createLanguageSettings());

  void updateLanguageField(String language, String field, String value) {
    state = AdminSettingsState(
      loaded: state.loaded,
      languages: state.languages
          .map((runtime) => runtime.language == language
              ? (field == 'image' ? runtime.copyWith(image: value) : runtime.copyWith(setupScript: value))
              : runtime)
          .toList(),
    );
  }

  Future<void> refresh() async {
    final app = ref.read(appProvider.notifier);
    final settings = await app.runOperation('Loading admin settings', api.fetchCredentials);
    final existing = settings
        .where((setting) => (setting.kind ?? SettingKind.secret) == SettingKind.config && setting.scope == _languageScope)
        .map((setting) => setting.name)
        .toSet();

    final languages = createLanguageSettings();
    final updated = <ForeignLanguageSetting>[];

    for (final runtime in languages) {
      if (!existing.contains(runtime.language)) {
        updated.add(runtime);
        continue;
      }

      final detail = await app.runOperation(
        'Loading ${runtime.label} runtime',
        () => api.fetchForeignLanguageRuntime(runtime.language),
      );
      final value = detail.value;

      var next = runtime;
      if (value is Map) {
        final image = value['image'];
        final setupScript = value['setup_script'];
        next = runtime.copyWith(
          image: (image is String && image.trim().isNotEmpty) ? image : runtime.defaultImage,
          setupScript: setupScript is String ? setupScript : '',
        );
      }
      updated.add(next);
    }

    state = AdminSettingsState(loaded: true, languages: updated);
  }

  Future<void> saveLanguage(String language) async {
    final app = ref.read(appProvider.notifier);
    ForeignLanguageSetting? runtime;
    for (final entry in state.languages) {
      if (entry.language == language) {
        runtime = entry;
        break;
      }
    }

    if (runtime == null) {
      app.setError('Unknown foreign language: $language');
      return;
    }

    final image = runtime.image.trim();

    if (image.isEmpty) {
      app.setError('${runtime.label} Docker image is required');
      return;
    }

    await app.runOperation(
      'Saving ${runtime.label} runtime',
      () => api.saveForeignLanguageRuntime(
        runtime!.language,
        api.ForeignLanguageRuntimeConfig(image: image, setupScript: runtime.setupScript),
      ),
    );

    state = AdminSettingsState(
      loaded: state.loaded,
      languages: state.languages.map((entry) => entry.language == language ? entry.copyWith(image: image) : entry).toList(),
    );
    app.setStatus('${runtime.label} foreign language runtime saved');
  }

  void clear() {
    state = AdminSettingsState(loaded: false, languages: createLanguageSettings());
  }
}
