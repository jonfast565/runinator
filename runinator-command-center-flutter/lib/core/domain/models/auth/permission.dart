// port of core/domain/models/auth/permission.ts.

enum PermissionLevel {
  view("view"),
  run("run"),
  edit("edit"),
  own("own");

  const PermissionLevel(this.wire);

  final String wire;

  static PermissionLevel fromJson(String value) => PermissionLevel.values.firstWhere(
        (level) => level.wire == value,
        orElse: () => throw ArgumentError('unknown PermissionLevel: $value'),
      );

  String toJson() => wire;
}

enum PrincipalType {
  user("user"),
  team("team");

  const PrincipalType(this.wire);

  final String wire;

  static PrincipalType fromJson(String value) => PrincipalType.values.firstWhere(
        (type) => type.wire == value,
        orElse: () => throw ArgumentError('unknown PrincipalType: $value'),
      );

  String toJson() => wire;
}
