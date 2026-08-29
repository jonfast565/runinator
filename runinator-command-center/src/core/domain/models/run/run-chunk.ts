export interface RunChunk {
  id: string;
  /** Durable effect-output event id. It can be used to correlate a rendered line with the API. */
  effect_id: string;
  /** The VM branch that emitted this output. */
  continuation_id: string;
  stream: string;
  content: string;
  /** Attempt that emitted the chunk; retries keep their output distinct. */
  attempt: number;
  /** ISO timestamp projected from the durable output event. */
  created_at: string;
}
