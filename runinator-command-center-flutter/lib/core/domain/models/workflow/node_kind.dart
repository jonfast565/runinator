// port of core/domain/models/workflow/node-kind.ts.

enum WorkflowNodeKind {
  start('start'),
  action('action'),
  wait('wait'),
  condition('condition'),
  switch_('switch'),
  toggle('toggle'),
  percentage('percentage'),
  approval('approval'),
  gate('gate'),
  signal('signal'),
  loop('loop'),
  parallel('parallel'),
  join('join'),
  try_('try'),
  map('map'),
  race('race'),
  output('output'),
  input('input'),
  subflow('subflow'),
  config('config'),
  assert_('assert'),
  transform('transform'),
  audit('audit'),
  checkpoint('checkpoint'),
  mutex('mutex'),
  throttle('throttle'),
  awaitRun('await_run'),
  debounce('debounce'),
  collect('collect'),
  barrier('barrier'),
  circuitBreaker('circuit_breaker'),
  eventSource('event_source'),
  end('end'),
  fail('fail');

  const WorkflowNodeKind(this.wire);

  final String wire;

  static WorkflowNodeKind? fromWire(String? value) {
    if (value == null) {
      return null;
    }

    for (final kind in WorkflowNodeKind.values) {
      if (kind.wire == value) {
        return kind;
      }
    }

    return null;
  }

  String toJson() => wire;
}
