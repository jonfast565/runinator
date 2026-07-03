// port of core/domain/models/setting.ts.

enum SettingKind {
  secret('secret'),
  config('config');

  const SettingKind(this.wire);

  final String wire;

  static SettingKind fromJson(String value) => SettingKind.values.firstWhere(
        (kind) => kind.wire == value,
        orElse: () => throw ArgumentError('unknown SettingKind: $value'),
      );

  String toJson() => wire;
}
