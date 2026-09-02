import type { JsonRecord } from "../domain/models";

export type IngressLane = "incoming" | "held" | "applied" | "dropped";

const COMPLETED_LANE_HISTORY_LIMIT = 50;

export function ingressLane(record: JsonRecord): IngressLane {
  if (record.state === "held") {
    return "held";
  }

  if (record.state === "dropped") {
    return "dropped";
  }

  if (["approved", "applying", "applied", "failed"].includes(String(record.state))) {
    return "applied";
  }

  return "incoming";
}

/**
 * Keep completed ingress visible as recent history. The API already returns newest-first records;
 * dwell controls their fresh visual treatment, not whether an operator can see them at all.
 */
export function recordsForIngressLane(records: JsonRecord[], lane: IngressLane): JsonRecord[] {
  const matching = records.filter((record) => ingressLane(record) === lane);

  return lane === "applied" || lane === "dropped"
    ? matching.slice(0, COMPLETED_LANE_HISTORY_LIMIT)
    : matching;
}
