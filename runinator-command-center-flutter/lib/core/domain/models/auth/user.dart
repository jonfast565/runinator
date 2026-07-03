// port of core/domain/models/auth/user.ts.

class User {
  const User({
    required this.id,
    required this.username,
    this.email,
    required this.isAdmin,
    required this.disabled,
    required this.createdAt,
    required this.updatedAt,
  });

  factory User.fromJson(Map<String, Object?> json) => User(
        id: json['id'] as String?,
        username: json['username'] as String,
        email: json['email'] as String?,
        isAdmin: json['is_admin'] as bool,
        disabled: json['disabled'] as bool,
        createdAt: json['created_at'] as String,
        updatedAt: json['updated_at'] as String,
      );

  final String? id;
  final String username;
  final String? email;
  final bool isAdmin;
  final bool disabled;
  final String createdAt;
  final String updatedAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'username': username,
        'email': email,
        'is_admin': isAdmin,
        'disabled': disabled,
        'created_at': createdAt,
        'updated_at': updatedAt,
      };
}
