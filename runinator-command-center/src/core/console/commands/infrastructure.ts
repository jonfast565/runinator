// `:nodes …`, `:orgs …`, `:agents …` — the fleet behind the runs.

import {
  createAgentEnrollmentToken,
  createAgentDirective,
  createOrg,
  fetchNodeBackends,
  fetchNodes,
  fetchOrgNodes,
  fetchOrgUsage,
  listAgentDirectives,
  listAgentEnrollmentTokens,
  listMyOrgs,
  revokeAgentEnrollmentToken,
  scaleNodes,
  scaleOrgNodes,
  stopNode,
} from "../../api/commandCenterApi";
import { getCommandRuntime } from "../../api/runtime";
import type { AgentDirectiveKind } from "../../domain/models";
import { cell, done, json, table, text, time, truncate } from "../format";
import { flag, flagList, numberFlag, requiredArg, requiredFlag } from "../options";
import type { ConsoleCommand, ConsoleFlags } from "../types";

export const nodeCommands: ConsoleCommand[] = [
  {
    path: ["nodes", "list"],
    usage: "nodes list",
    summary: "list provisioning backends and current node group sizing",
    run: async ({ json: raw, print }) => {
      const [backends, groups] = await Promise.all([fetchNodeBackends(), fetchNodes()]);

      if (raw) {
        print(json({ backends: backends as unknown as Record<string, unknown>, groups }));
        return;
      }

      print(
        text(
          `backends: ${backends.backends
            .map((backend) => `${backend.backend}${backend.available ? "" : " (unavailable)"}`)
            .join(", ")}`,
        ),
      );
      print(
        table(
          ["backend", "kind", "name", "desired", "available"],
          groups.map((group) => [
            group.backend,
            group.kind,
            truncate(group.name, 28),
            String(group.desired),
            String(group.available),
          ]),
        ),
      );
    },
  },
  {
    path: ["nodes", "spin-up"],
    usage: "nodes spin-up --backend B --kind K [--count N] [--label KEY=VALUE]",
    summary: "add nodes of a kind, raising the desired count",
    run: async ({ flags, print }) => {
      const backend = requiredFlag(flags, "backend");
      const kind = requiredFlag(flags, "kind");
      const count = numberFlag(flags, "count") ?? 1;
      // the api sets an absolute desired count, so "add N" is read-then-set — the same thing
      // runinatorctl does, and the same race it accepts.
      const groups = await fetchNodes();
      const current =
        groups.find((group) => group.backend === backend && group.kind === kind)?.desired ?? 0;
      const group = await scaleNodes({
        backend,
        kind,
        desired: current + count,
        spec: { labels: labels(flags) },
      });
      print(done(`${backend}/${kind} desired ${String(group.desired)}`));
    },
  },
  {
    path: ["nodes", "scale"],
    usage: "nodes scale --backend B --kind K --desired N",
    summary: "set the exact desired node count for a kind on a backend",
    run: async ({ flags, print }) => {
      const group = await scaleNodes({
        backend: requiredFlag(flags, "backend"),
        kind: requiredFlag(flags, "kind"),
        desired: Number(requiredFlag(flags, "desired")),
      });
      print(done(`${group.backend}/${group.kind} desired ${String(group.desired)}`));
    },
  },
  {
    path: ["nodes", "stop"],
    usage: "nodes stop --backend B --node ID",
    summary: "stop and remove a single node instance",
    run: async ({ flags, print }) => {
      const response = await stopNode({
        backend: requiredFlag(flags, "backend"),
        node_id: requiredFlag(flags, "node"),
      });
      print(json(response));
    },
  },
];

export const orgCommands: ConsoleCommand[] = [
  {
    path: ["orgs", "list"],
    usage: "orgs list",
    summary: "list the organizations you belong to, with your role in each",
    run: async ({ json: raw, print }) => {
      const memberships = await listMyOrgs();

      if (raw) {
        print(json(memberships));
        return;
      }

      print(
        table(
          ["id", "name", "role"],
          memberships.map((membership) => [
            cell(membership.org.id),
            truncate(membership.org.name, 32),
            cell(membership.role),
          ]),
        ),
      );
    },
  },
  {
    path: ["orgs", "create"],
    usage: "orgs create <name>",
    summary: "create an organization; you become its owner",
    run: async ({ args, print }) => {
      const org = await createOrg(requiredArg(args, 0, "name"));
      print(done(`created org ${org.name} (${org.id})`));
    },
  },
  {
    path: ["orgs", "nodes"],
    usage: "orgs nodes <org-id>",
    summary: "show an org's dedicated node allocation and projected monthly cost",
    run: async ({ args, json: raw, print }) => {
      const nodes = await fetchOrgNodes(requiredArg(args, 0, "org id"));

      if (raw) {
        print(json(nodes));
        return;
      }

      print(text(`projected monthly: ${money(nodes.projected_monthly_cents)}`));
      print(
        table(
          ["backend", "kind", "desired"],
          nodes.groups.map((group) => [group.backend, group.kind, String(group.desired)]),
        ),
      );
    },
  },
  {
    path: ["orgs", "scale"],
    usage: "orgs scale <org-id> --backend B --kind K --desired N",
    summary: "set an org's dedicated node count for a kind on a backend",
    run: async ({ args, flags, print }) => {
      const group = await scaleOrgNodes(requiredArg(args, 0, "org id"), {
        backend: requiredFlag(flags, "backend"),
        kind: requiredFlag(flags, "kind"),
        desired: Number(requiredFlag(flags, "desired")),
      });
      print(json(group));
    },
  },
  {
    path: ["orgs", "usage"],
    usage: "orgs usage <org-id>",
    summary: "show an org's accrued usage and cost over the trailing 30 days",
    run: async ({ args, json: raw, print }) => {
      const usage = await fetchOrgUsage(requiredArg(args, 0, "org id"));

      if (raw) {
        print(json(usage));
        return;
      }

      print(text(`since ${cell(usage.since)}, accrued ${money(usage.accrued_cents)}`));
      print(
        table(
          ["kind", "node_hours"],
          Object.entries(usage.node_hours).map(([kind, hours]) => [kind, hours.toFixed(2)]),
        ),
      );
    },
  },
];

export const agentCommands: ConsoleCommand[] = [
  directive("diagnostics", "collect runtime diagnostics from one agent", () => ({
    type: "diagnostics",
  })),
  directive("drain", "stop one agent from accepting new actions", () => ({ type: "drain" })),
  directive("restart", "restart one agent's broker worker loop", () => ({ type: "restart" })),
  directive("logs", "fetch recent desktop-agent log lines", (flags) => ({
    type: "tail_logs",
    lines: numberFlag(flags, "lines") ?? 200,
  })),
  {
    path: ["agents", "directives"],
    usage: "agents directives <replica-id> [--limit N]",
    summary: "list recent directive state for one agent",
    run: async ({ args, flags, json: raw, print }) => {
      const directives = await listAgentDirectives(
        requiredArg(args, 0, "replica id"),
        numberFlag(flags, "limit") ?? 50,
      );

      if (raw) {
        print(json(directives));
        return;
      }

      print(
        table(
          ["directive", "state", "issued", "completed", "message"],
          directives.map((record) => [
            truncate(record.directive_id, 14),
            cell(record.state),
            time(record.issued_at),
            time(record.completed_at),
            truncate(record.message, 40),
          ]),
        ),
      );
    },
  },
  {
    path: ["agents", "enroll-token"],
    usage: "agents enroll-token [--ttl 15m] [--label KEY=VALUE] [--org ID] [--service-url URL]",
    summary: "create a single-use enrollment token, shown only once",
    run: async ({ flags, print }) => {
      const response = await createAgentEnrollmentToken({
        ttl_seconds: parseTtl(flag(flags, "ttl") ?? "15m"),
        org_id: flag(flags, "org") ?? null,
        labels: labels(flags),
        // the token embeds the url the agent will call back on, which is the service this console
        // is already talking to unless the operator names another.
        service_url: flag(flags, "service-url") ?? getCommandRuntime().apiBaseUrl(),
        cluster_id: flag(flags, "cluster-id") ?? null,
        spki_pin: flag(flags, "spki-pin") ?? null,
      });
      // shown once and never returned again, so it is printed on its own line rather than buried in
      // the record.
      print(text(response.token));
      print(json(response.enrollment_token));
    },
  },
  {
    path: ["agents", "enrollment-tokens"],
    usage: "agents enrollment-tokens",
    summary: "list enrollment-token metadata; secrets are never returned",
    run: async ({ json: raw, print }) => {
      const tokens = await listAgentEnrollmentTokens();

      if (raw) {
        print(json(tokens));
        return;
      }

      print(
        table(
          ["token", "org", "expires", "consumed", "labels"],
          tokens.map((token) => [
            truncate(token.token_id, 14),
            truncate(token.org_id, 14),
            time(token.expires_at),
            time(token.consumed_at),
            Object.entries(token.labels)
              .map(([key, value]) => `${key}=${value}`)
              .join(","),
          ]),
        ),
      );
    },
  },
  {
    path: ["agents", "revoke-token"],
    usage: "agents revoke-token <token-id>",
    summary: "revoke an unused enrollment token",
    run: async ({ args, print }) => {
      const tokenId = requiredArg(args, 0, "token id");
      await revokeAgentEnrollmentToken(tokenId);
      print(done(`revoked ${tokenId}`));
    },
  },
];

// the four directive verbs differ only in the kind they send.
function directive(
  name: string,
  summary: string,
  kind: (flags: ConsoleFlags) => AgentDirectiveKind,
): ConsoleCommand {
  return {
    path: ["agents", name],
    usage: `agents ${name} <replica-id>`,
    summary,
    run: async ({ args, flags, print }) => {
      const record = await createAgentDirective(requiredArg(args, 0, "replica id"), kind(flags));
      print(json(record));
    },
  };
}

// `KEY=VALUE` labels from a repeatable `--label`.
function labels(flags: ConsoleFlags): Record<string, string> {
  const labels: Record<string, string> = {};

  for (const entry of flagList(flags, "label")) {
    const separator = entry.indexOf("=");

    if (separator <= 0) {
      throw new Error(`--label expects KEY=VALUE, got '${entry}'`);
    }

    labels[entry.slice(0, separator)] = entry.slice(separator + 1);
  }

  return labels;
}

// `30s`, `15m`, `2h`, `1d`, or a bare number of seconds — the units runinatorctl documents.
function parseTtl(value: string): number {
  const match = /^(\d+)([smhd]?)$/.exec(value.trim());

  if (!match) {
    throw new Error("--ttl takes a duration such as 30s, 15m, 2h, or 1d");
  }

  const scale = { "": 1, s: 1, m: 60, h: 3600, d: 86400 }[match[2]] ?? 1;
  return Number(match[1]) * scale;
}

function money(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}
