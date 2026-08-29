import type { RunChunk } from "../domain/models";

/** One physical output line, retaining the durable event context that produced it. */
export interface OutputChunkLine {
  id: string;
  timestamp: string;
  stream: string;
  attempt: number;
  effectId: string;
  continuationId: string;
  content: string;
}

/**
 * Splits streamed chunks for display without losing provenance. Providers are free to emit a
 * multi-line chunk; every rendered line keeps its timestamp, stream, and retry attempt so a
 * copied log remains useful after it leaves the command center.
 */
export function outputChunkLines(chunks: RunChunk[]): OutputChunkLine[] {
  return chunks.flatMap((chunk) =>
    chunk.content.replaceAll("\r\n", "\n").split("\n").map((content, index) => ({
      id: `${chunk.id}:${String(index)}`,
      timestamp: chunk.created_at,
      stream: chunk.stream,
      attempt: chunk.attempt,
      effectId: chunk.effect_id,
      continuationId: chunk.continuation_id,
      content,
    })),
  );
}

export function outputChunkTimestamp(value: string): string {
  const date = new Date(value);

  return Number.isNaN(date.getTime()) ? value : date.toISOString();
}
