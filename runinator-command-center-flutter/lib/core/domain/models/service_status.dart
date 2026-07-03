// port of core/domain/models/service-status.ts.

class ServiceStatus {
  const ServiceStatus({required this.serviceUrl});

  factory ServiceStatus.fromJson(Map<String, Object?> json) =>
      ServiceStatus(serviceUrl: json['service_url'] as String?);

  final String? serviceUrl;

  Map<String, Object?> toJson() => {'service_url': serviceUrl};
}
