// port of core/domain/models/replica.ts.

import '../json.dart';

enum ReplicaKind {
  worker('worker'),
  waker('waker'),
  webservice('webservice'),
  postgres('postgres'),
  archiver('archiver');

  const ReplicaKind(this.wire);

  final String wire;

  static ReplicaKind fromJson(String value) => ReplicaKind.values.firstWhere(
        (kind) => kind.wire == value,
        orElse: () => throw ArgumentError('unknown ReplicaKind: $value'),
      );

  String toJson() => wire;
}

enum ReplicaStatus {
  live('live'),
  stale('stale'),
  offline('offline');

  const ReplicaStatus(this.wire);

  final String wire;

  static ReplicaStatus fromJson(String value) => ReplicaStatus.values.firstWhere(
        (status) => status.wire == value,
        orElse: () => throw ArgumentError('unknown ReplicaStatus: $value'),
      );

  String toJson() => wire;
}

class ReplicaRecord {
  const ReplicaRecord({
    required this.replicaId,
    required this.replicaType,
    required this.instanceId,
    required this.runtimeId,
    required this.status,
    this.displayName,
    this.host,
    this.port,
    this.basePath,
    this.observedIp,
    this.version,
    required this.attributes,
    required this.firstSeenAt,
    required this.lastHeartbeatAt,
    required this.lastSeenAt,
    this.offlineAt,
  });

  factory ReplicaRecord.fromJson(Map<String, Object?> json) => ReplicaRecord(
        replicaId: json['replica_id'] as String,
        replicaType: ReplicaKind.fromJson(json['replica_type'] as String),
        instanceId: json['instance_id'] as String,
        runtimeId: json['runtime_id'] as String,
        status: ReplicaStatus.fromJson(json['status'] as String),
        displayName: json['display_name'] as String?,
        host: json['host'] as String?,
        port: (json['port'] as num?)?.toInt(),
        basePath: json['base_path'] as String?,
        observedIp: json['observed_ip'] as String?,
        version: json['version'] as String?,
        attributes: asJsonObject(json['attributes']),
        firstSeenAt: json['first_seen_at'] as String,
        lastHeartbeatAt: json['last_heartbeat_at'] as String,
        lastSeenAt: json['last_seen_at'] as String,
        offlineAt: json['offline_at'] as String?,
      );

  final String replicaId;
  final ReplicaKind replicaType;
  final String instanceId;
  final String runtimeId;
  final ReplicaStatus status;
  final String? displayName;
  final String? host;
  final int? port;
  final String? basePath;
  final String? observedIp;
  final String? version;
  final JsonObject attributes;
  final String firstSeenAt;
  final String lastHeartbeatAt;
  final String lastSeenAt;
  final String? offlineAt;

  Map<String, Object?> toJson() => {
        'replica_id': replicaId,
        'replica_type': replicaType.toJson(),
        'instance_id': instanceId,
        'runtime_id': runtimeId,
        'status': status.toJson(),
        'display_name': displayName,
        'host': host,
        'port': port,
        'base_path': basePath,
        'observed_ip': observedIp,
        'version': version,
        'attributes': attributes,
        'first_seen_at': firstSeenAt,
        'last_heartbeat_at': lastHeartbeatAt,
        'last_seen_at': lastSeenAt,
        'offline_at': offlineAt,
      };
}

class ReplicaCounts {
  const ReplicaCounts({required this.workers, required this.wakers, required this.webservices});

  factory ReplicaCounts.fromJson(Map<String, Object?> json) => ReplicaCounts(
        workers: (json['workers'] as num).toInt(),
        wakers: (json['wakers'] as num).toInt(),
        webservices: (json['webservices'] as num).toInt(),
      );

  final int workers;
  final int wakers;
  final int webservices;

  Map<String, Object?> toJson() => {'workers': workers, 'wakers': wakers, 'webservices': webservices};
}

class ReplicaListResponse {
  const ReplicaListResponse({required this.counts, required this.replicas});

  factory ReplicaListResponse.fromJson(Map<String, Object?> json) => ReplicaListResponse(
        counts: ReplicaCounts.fromJson(json['counts'] as Map<String, Object?>),
        replicas: (json['replicas'] as List)
            .map((r) => ReplicaRecord.fromJson(r as Map<String, Object?>))
            .toList(),
      );

  final ReplicaCounts counts;
  final List<ReplicaRecord> replicas;

  Map<String, Object?> toJson() => {
        'counts': counts.toJson(),
        'replicas': replicas.map((r) => r.toJson()).toList(),
      };
}
