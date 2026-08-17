// `:nodes …`, `:orgs …`, `:replicas …`, `:agents …` — the fleet behind the runs.

import {
  createAgentEnrollmentToken,
  createAgentDirective,
  createOrg,
  fetchNodeBackends,
  fetchNodes,
  fetchOrgNodes,
  fetchOrgUsage,
  fetchReplicaProviders,
  fetchReplicas,
  fetchReplicaSamples,
  listAgentDirectives,
  listAgentEnrollmentTokens,
  listMyOrgs,
  revokeAgentEnrollmentToken,
  scaleNodes,
  scaleOrgNodes,
  stopNode,
} from "../../api/commandCenterApi";
import { getCommandRuntime } from "../../api/runtime";
import type { AgentDirectiveKind, ReplicaRecord } from "../../domain/models";
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

export const replicaCommands: ConsoleCommand[] = [
  {
    path: ["replicas", "list"],
    usage: "replicas list [--kind KIND] [--status live|stale|offline] [--live]",
    summary: "list registered replicas and their ids",
    run: async ({ flags, json: raw, print }) => {
      const replicas = await selectReplicas(flags);

      if (raw) {
        print(json(replicas));
        return;
      }

      print(
        table(
          ["id", "kind", "status", "name", "endpoint", "last_heartbeat"],
          replicas.map((replica) => [
            replica.replica_id,
            replica.replica_type,
            replica.status,
            truncate(replicaName(replica), 28),
            truncate(endpoint(replica), 32),
            time(replica.last_heartbeat_at),
          ]),
        ),
      );
    },
  },
  {
    path: ["replicas", "ids"],
    usage: "replicas ids [--kind KIND] [--status live|stale|offline] [--live]",
    summary: "print just the replica ids, one per line",
    run: async ({ flags, json: raw, print }) => {
      const replicas = await selectReplicas(flags);

      if (raw) {
        print(json(replicas.map((replica) => replica.replica_id)));
        return;
      }

      for (const replica of replicas) {
        print(text(replica.replica_id));
      }
    },
  },
  {
    path: ["replicas", "show"],
    usage: "replicas show <replica-id>",
    summary: "show one replica, including the attributes it heartbeats",
    run: async ({ args, json: raw, print }) => {
      // there is no fetch-one endpoint, and adding one for a list this size would be a route that
      // only saves a filter.
      const wanted = requiredArg(args, 0, "replica id");
      const { replicas } = await fetchReplicas();
      const replica = replicas.find((candidate) => candidate.replica_id === wanted);

      if (!replica) {
        throw new Error(`replica ${wanted} not found`);
      }

      if (raw) {
        print(json(replica));
        return;
      }

      print(
        table(
          ["field", "value"],
          [
            ["id", replica.replica_id],
            ["kind", replica.replica_type],
            ["status", replica.status],
            ["name", replicaName(replica)],
            ["instance", replica.instance_id],
            ["runtime", replica.runtime_id],
            ["endpoint", endpoint(replica)],
            ["observed_ip", cell(replica.observed_ip)],
            ["version", cell(replica.version)],
            ["first_seen", time(replica.first_seen_at)],
            ["last_heartbeat", time(replica.last_heartbeat_at)],
            ["offline_at", time(replica.offline_at)],
          ],
        ),
      );
      print(json(replica.attributes));
    },
  },
  {
    path: ["replicas", "providers"],
    usage: "replicas providers <replica-id>",
    summary: "list the providers one replica has registered",
    run: async ({ args, json: raw, print }) => {
      const registrations = await fetchReplicaProviders(requiredArg(args, 0, "replica id"));

      if (raw) {
        print(json(registrations));
        return;
      }

      print(
        table(
          ["provider", "actions", "credential_scopes"],
          registrations.map((registration) => [
            truncate(registration.provider_name, 28),
            String(registration.provider.actions.length),
            truncate(registration.provider.metadata.credential_scopes.join(","), 36),
          ]),
        ),
      );
    },
  },
  {
    path: ["replicas", "samples"],
    usage: "replicas samples <replica-id> [--since-seconds N] [--limit N]",
    summary: "show one replica's recent cpu/memory telemetry",
    run: async ({ args, flags, json: raw, print }) => {
      const series = await fetchReplicaSamples(
        requiredArg(args, 0, "replica id"),
        numberFlag(flags, "since-seconds"),
      );

      if (raw) {
        print(json(series));
        return;
      }

      const limit = numberFlag(flags, "limit") ?? 20;
      print(
        table(
          ["sampled_at", "cpu%", "mem%", "proc_cpu%", "proc_mem", "load1"],
          series.samples.slice(-limit).map((sample) => [
            time(sample.sampled_at),
            sample.cpu_percent.toFixed(1),
            sample.mem_percent.toFixed(1),
            sample.process_cpu_percent.toFixed(1),
            bytes(sample.process_mem_bytes),
            sample.load_one === null || sample.load_one === undefined
              ? "-"
              : sample.load_one.toFixed(2),
          ]),
        ),
      );
    },
  },
];

// the filters `replicas list` and `replicas ids` share. `--live` is the one almost every use of
// this wants, so it is a flag of its own rather than a status to remember the spelling of.
async function selectReplicas(flags: ConsoleFlags): Promise<ReplicaRecord[]> {
  const { replicas } = await fetchReplicas();
  const kind = flag(flags, "kind");
  const status = flag(flags, "status") ?? (flags.live === undefined ? undefined : "live");

  return replicas.filter(
    (replica) =>
      (kind === undefined || replica.replica_type === kind) &&
      (status === undefined || replica.status === status),
  );
}

// a replica names itself when it can; an unnamed one is still addressable by its instance.
function replicaName(replica: ReplicaRecord): string {
  return replica.display_name ?? replica.instance_id;
}

function endpoint(replica: ReplicaRecord): string {
  if (!replica.host) {
    return "-";
  }

  return replica.port ? `${replica.host}:${String(replica.port)}` : replica.host;
}

function bytes(value: number): string {
  const units = ["B", "KiB", "MiB", "GiB"];
  let size = value;
  let unit = 0;

  while (size >= 1024 && unit + 1 < units.length) {
    size /= 1024;
    unit += 1;
  }

  return `${size.toFixed(1)}${units[unit]}`;
}

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
  directive(
    "logs",
    "fetch recent desktop-agent log lines",
    (flags) => ({ type: "tail_logs", lines: numberFlag(flags, "lines") ?? 200 }),
    "[--lines N]",
  ),
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
    usage:
      "agents enroll-token [--ttl 15m] [--label KEY=VALUE] [--org ID] [--service-url URL] [--cluster-id ID] [--spki-pin PIN]",
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

// the four directive verbs differ only in the kind they send, and in whether that kind reads a
// flag — which the usage has to name, since the usage is what a line is checked against.
function directive(
  name: string,
  summary: string,
  kind: (flags: ConsoleFlags) => AgentDirectiveKind,
  options = "",
): ConsoleCommand {
  return {
    path: ["agents", name],
    usage: `agents ${name} <replica-id> ${options}`.trimEnd(),
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
