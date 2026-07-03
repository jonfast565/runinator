// port of core/domain/models/auth/api-key.ts.

class ApiKey {
  const ApiKey({
    required this.id,
    required this.name,
    this.userId,
    required this.isService,
    required this.keyPrefix,
    this.lastUsedAt,
    this.expiresAt,
    required this.disabled,
    required this.createdAt,
  });

  factory ApiKey.fromJson(Map<String, Object?> json) => ApiKey(
        id: json['id'] as String?,
        name: json['name'] as String,
        userId: json['user_id'] as String?,
        isService: json['is_service'] as bool,
        keyPrefix: json['key_prefix'] as String,
        lastUsedAt: json['last_used_at'] as String?,
        expiresAt: json['expires_at'] as String?,
        disabled: json['disabled'] as bool,
        createdAt: json['created_at'] as String,
      );

  final String? id;
  final String name;
  final String? userId;
  final bool isService;
  final String keyPrefix;
  final String? lastUsedAt;
  final String? expiresAt;
  final bool disabled;
  final String createdAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'name': name,
        'user_id': userId,
        'is_service': isService,
        'key_prefix': keyPrefix,
        'last_used_at': lastUsedAt,
        'expires_at': expiresAt,
        'disabled': disabled,
        'created_at': createdAt,
      };
}

class CreateApiKeyResponse {
  const CreateApiKeyResponse({required this.apiKey, required this.secret});

  factory CreateApiKeyResponse.fromJson(Map<String, Object?> json) => CreateApiKeyResponse(
        apiKey: ApiKey.fromJson(json['api_key'] as Map<String, Object?>),
        secret: json['secret'] as String,
      );

  final ApiKey apiKey;
  final String secret;

  Map<String, Object?> toJson() => {
        'api_key': apiKey.toJson(),
        'secret': secret,
      };
}
