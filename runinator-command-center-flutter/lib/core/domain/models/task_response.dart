// port of core/domain/models/task-response.ts.

class TaskResponse {
  const TaskResponse({required this.success, required this.message});

  factory TaskResponse.fromJson(Map<String, Object?> json) => TaskResponse(
        success: json['success'] as bool,
        message: json['message'] as String,
      );

  final bool success;
  final String message;

  Map<String, Object?> toJson() => {'success': success, 'message': message};
}
