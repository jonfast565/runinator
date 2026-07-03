// port of core/domain/models/run/run-chunk.ts.

class RunChunk {
  const RunChunk({required this.id, required this.stream, required this.content});

  factory RunChunk.fromJson(Map<String, Object?> json) => RunChunk(
        id: json['id'] as String,
        stream: json['stream'] as String,
        content: json['content'] as String,
      );

  final String id;
  final String stream;
  final String content;

  Map<String, Object?> toJson() => {'id': id, 'stream': stream, 'content': content};
}
