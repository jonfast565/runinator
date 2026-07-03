// port of core/domain/models/workflow-state/debug-mode.ts.

/// debug step granularity: pause before every node, or only at breakpoints.
enum DebugMode {
  stepAll('step_all'),
  breakpoints('breakpoints');

  const DebugMode(this.wire);

  final String wire;

  static DebugMode? fromWireOrNull(String? value) =>
      value == 'step_all' ? DebugMode.stepAll : (value == 'breakpoints' ? DebugMode.breakpoints : null);

  String toJson() => wire;
}
