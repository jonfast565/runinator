// shared exponential-backoff-with-jitter helper for websocket reconnects.
//
// a fixed reconnect delay means every client that drops at the same moment (a
// server restart, a rate-limit blip that closes sockets) reconnects in lockstep,
// which regenerates the exact spike that caused the drop. this staggers retries
// across clients and backs off on repeated failures instead of hammering at a
// constant interval forever.

import 'dart:math';

class ReconnectBackoff {
  ReconnectBackoff({
    this.initial = const Duration(milliseconds: 2000),
    this.max = const Duration(seconds: 30),
    this.factor = 1.7,
    Random? random,
  }) : _random = random ?? Random();

  final Duration initial;
  final Duration max;
  final double factor;
  final Random _random;

  int _attempt = 0;

  /// next delay to wait before reconnecting; grows with repeated failures and
  /// is jittered so concurrent clients don't retry in lockstep.
  Duration next() {
    final initialMs = initial.inMilliseconds.toDouble();
    final maxMs = max.inMilliseconds.toDouble();
    final rawMs = (initialMs * pow(factor, _attempt)).clamp(initialMs, maxMs);
    _attempt += 1;

    // full jitter within [initialMs/2, rawMs], so retries spread out immediately
    // rather than only once the backoff has grown.
    final floorMs = initialMs / 2;
    final jitteredMs = floorMs + _random.nextDouble() * (rawMs - floorMs);
    return Duration(milliseconds: jitteredMs.round());
  }

  /// call after a successful connection so the next drop starts from scratch.
  void reset() {
    _attempt = 0;
  }
}
