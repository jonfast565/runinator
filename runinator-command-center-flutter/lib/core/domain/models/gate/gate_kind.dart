// port of core/domain/models/gate/gate-kind.ts.

enum GateKind {
  manual('manual'),
  condition('condition'),
  external('external');

  const GateKind(this.wire);

  final String wire;

  static GateKind fromJson(String value) => GateKind.values.firstWhere(
        (kind) => kind.wire == value,
        orElse: () => throw ArgumentError('unknown GateKind: $value'),
      );

  String toJson() => wire;
}
