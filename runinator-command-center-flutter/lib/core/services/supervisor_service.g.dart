// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'supervisor_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(supervisorService)
final supervisorServiceProvider = SupervisorServiceProvider._();

final class SupervisorServiceProvider
    extends
        $FunctionalProvider<
          SupervisorService,
          SupervisorService,
          SupervisorService
        >
    with $Provider<SupervisorService> {
  SupervisorServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'supervisorServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$supervisorServiceHash();

  @$internal
  @override
  $ProviderElement<SupervisorService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  SupervisorService create(Ref ref) {
    return supervisorService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(SupervisorService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<SupervisorService>(value),
    );
  }
}

String _$supervisorServiceHash() => r'316fb0b93836bcf3a3bc70978a648535d67356e1';
