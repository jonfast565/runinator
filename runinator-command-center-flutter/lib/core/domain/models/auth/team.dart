// port of core/domain/models/auth/team.ts.

class Team {
  const Team({required this.id, required this.name, required this.createdAt});

  factory Team.fromJson(Map<String, Object?> json) => Team(
        id: json['id'] as String?,
        name: json['name'] as String,
        createdAt: json['created_at'] as String,
      );

  final String? id;
  final String name;
  final String createdAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'name': name,
        'created_at': createdAt,
      };
}
