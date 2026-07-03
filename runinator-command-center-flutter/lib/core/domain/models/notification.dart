// port of core/domain/models/notification.ts.

import '../json.dart';

enum NotificationChannel {
  inApp('in_app'),
  email('email'),
  slack('slack');

  const NotificationChannel(this.wire);

  final String wire;

  static NotificationChannel fromJson(String value) => NotificationChannel.values.firstWhere(
        (channel) => channel.wire == value,
        orElse: () => throw ArgumentError('unknown NotificationChannel: $value'),
      );

  String toJson() => wire;
}

enum NotificationSeverity {
  info('info'),
  success('success'),
  warning('warning'),
  error('error');

  const NotificationSeverity(this.wire);

  final String wire;

  static NotificationSeverity fromJson(String value) => NotificationSeverity.values.firstWhere(
        (severity) => severity.wire == value,
        orElse: () => throw ArgumentError('unknown NotificationSeverity: $value'),
      );

  String toJson() => wire;
}

class Notification {
  const Notification({
    required this.id,
    this.workflowRunId,
    this.workflowNodeId,
    required this.channel,
    required this.severity,
    required this.title,
    this.body,
    this.target,
    this.metadata,
    this.readAt,
    required this.createdAt,
  });

  factory Notification.fromJson(Map<String, Object?> json) => Notification(
        id: json['id'] as String,
        workflowRunId: json['workflow_run_id'] as String?,
        workflowNodeId: json['workflow_node_id'] as String?,
        channel: NotificationChannel.fromJson(json['channel'] as String),
        severity: NotificationSeverity.fromJson(json['severity'] as String),
        title: json['title'] as String,
        body: json['body'] as String?,
        target: json['target'] as String?,
        metadata: json['metadata'] != null ? asJsonObject(json['metadata']) : null,
        readAt: json['read_at'] as String?,
        createdAt: json['created_at'] as String,
      );

  final String id;
  final String? workflowRunId;
  final String? workflowNodeId;
  final NotificationChannel channel;
  final NotificationSeverity severity;
  final String title;
  final String? body;
  final String? target;
  final JsonObject? metadata;
  final String? readAt;
  final String createdAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'workflow_run_id': workflowRunId,
        'workflow_node_id': workflowNodeId,
        'channel': channel.toJson(),
        'severity': severity.toJson(),
        'title': title,
        'body': body,
        'target': target,
        'metadata': metadata,
        'read_at': readAt,
        'created_at': createdAt,
      };
}
