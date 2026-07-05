// exponential backoff with jitter for websocket reconnects.
//
// a fixed reconnect delay means every client that drops at the same moment (a
// server restart, a rate-limit blip that closes sockets) reconnects in lockstep,
// which regenerates the exact spike that caused the drop. this staggers retries
// across clients and backs off on repeated failures instead of hammering at a
// constant interval forever.

export interface ReconnectBackoffOptions {
  initialMs?: number;
  maxMs?: number;
  factor?: number;
  random?: () => number;
}

export class ReconnectBackoff {
  private readonly initialMs: number;
  private readonly maxMs: number;
  private readonly factor: number;
  private readonly random: () => number;
  private attempt = 0;

  constructor(options: ReconnectBackoffOptions = {}) {
    this.initialMs = options.initialMs ?? 2000;
    this.maxMs = options.maxMs ?? 30000;
    this.factor = options.factor ?? 1.7;
    this.random = options.random ?? Math.random;
  }

  // next delay to wait before reconnecting; grows with repeated failures and
  // is jittered so concurrent clients don't retry in lockstep.
  next(): number {
    const rawMs = Math.min(Math.max(this.initialMs * this.factor ** this.attempt, this.initialMs), this.maxMs);
    this.attempt += 1;

    // full jitter within [initialMs/2, rawMs], so retries spread out immediately
    // rather than only once the backoff has grown.
    const floorMs = this.initialMs / 2;
    return Math.round(floorMs + this.random() * (rawMs - floorMs));
  }

  // call after a successful connection so the next drop starts from scratch.
  reset(): void {
    this.attempt = 0;
  }
}
