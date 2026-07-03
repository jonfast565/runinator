// port of core/domain/models/auth/grant.ts.

import 'permission.dart';

class Grant {
  const Grant({
    required this.id,
    required this.resourceType,
    required this.resourceId,
    required this.principalType,
    required this.principalId,
    required this.permission,
    required this.createdAt,
  });

  factory Grant.fromJson(Map<String, Object?> json) => Grant(
        id: json['id'] as String?,
        resourceType: json['resource_type'] as String,
        resourceId: json['resource_id'] as String,
        principalType: PrincipalType.fromJson(json['principal_type'] as String),
        principalId: json['principal_id'] as String,
        permission: PermissionLevel.fromJson(json['permission'] as String),
        createdAt: json['created_at'] as String,
      );

  final String? id;
  final String resourceType;
  final String resourceId;
  final PrincipalType principalType;
  final String principalId;
  final PermissionLevel permission;
  final String createdAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'resource_type': resourceType,
        'resource_id': resourceId,
        'principal_type': principalType.toJson(),
        'principal_id': principalId,
        'permission': permission.toJson(),
        'created_at': createdAt,
      };
}
