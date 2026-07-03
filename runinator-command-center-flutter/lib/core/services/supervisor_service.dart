// port of core/services/supervisor.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;

part 'supervisor_service.g.dart';

class SupervisorService {
  const SupervisorService();

  Future<api.SupervisorStatus> fetchStatus() => api.fetchSupervisorStatus();
}

@riverpod
SupervisorService supervisorService(Ref ref) => const SupervisorService();
