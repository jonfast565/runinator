// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'audit_log_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(auditLogService)
final auditLogServiceProvider = AuditLogServiceProvider._();

final class AuditLogServiceProvider
    extends
        $FunctionalProvider<AuditLogService, AuditLogService, AuditLogService>
    with $Provider<AuditLogService> {
  AuditLogServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'auditLogServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$auditLogServiceHash();

  @$internal
  @override
  $ProviderElement<AuditLogService> $createElement($ProviderPointer pointer) =>
      $ProviderElement(pointer);

  @override
  AuditLogService create(Ref ref) {
    return auditLogService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(AuditLogService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<AuditLogService>(value),
    );
  }
}

String _$auditLogServiceHash() => r'489239439f4df5791521f37726f9ab16bfeaf101';
