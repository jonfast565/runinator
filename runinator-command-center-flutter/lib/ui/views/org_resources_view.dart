import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/api/command_center_api.dart' as api;
import '../../core/domain/icons.dart';
import '../../core/services/org_resources_service.dart';
import '../../core/services/orgs_service.dart';
import '../shared/cc_widgets.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';

const _hoursPerMonth = 730;

class OrgResourcesView extends ConsumerStatefulWidget {
  const OrgResourcesView({super.key});

  @override
  ConsumerState<OrgResourcesView> createState() => _OrgResourcesViewState();
}

class _OrgResourcesViewState extends ConsumerState<OrgResourcesView> {
  var _loading = false;
  var _scaling = false;
  List<api.OrgResourceGroup> _groups = const [];
  var _projectedMonthlyCents = 0;
  api.OrgQuota? _quota;
  api.OrgUsage? _usage;
  api.RateCard _rateCard = const api.RateCard(entries: []);
  var _scaleBackend = 'supervisor';
  var _scaleKind = 'worker';
  var _scaleDesired = 1;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refresh());
  }

  int _rate(String backend, String kind) {
    for (final entry in _rateCard.entries) {
      if (entry.backend == backend && entry.kind == kind) {
        return entry.hourlyCents;
      }
    }
    return 0;
  }

  String _fmtCents(int cents) => '\$${(cents / 100).toStringAsFixed(2)}';

  int? get _budgetPct {
    if (_quota == null || _quota!.maxMonthlyCents <= 0) return null;
    return ((_projectedMonthlyCents / _quota!.maxMonthlyCents) * 100).round();
  }

  Future<void> _refresh() async {
    final orgId = ref.read(orgsProvider).activeOrgId;
    setState(() {
      _groups = const [];
      _projectedMonthlyCents = 0;
      _quota = null;
      _usage = null;
    });

    if (orgId == null) return;

    setState(() => _loading = true);
    try {
      final service = ref.read(orgResourcesServiceProvider);
      _rateCard = await service.fetchRateCard().catchError((_) => const api.RateCard(entries: []));
      final nodes = await service.fetchNodes(orgId).catchError((_) => const api.OrgNodesResponse(groups: [], projectedMonthlyCents: 0));
      api.OrgQuota? quota;
      api.OrgUsage? usage;
      try {
        quota = await service.fetchQuota(orgId);
      } catch (_) {
        quota = null;
      }
      try {
        usage = await service.fetchUsage(orgId);
      } catch (_) {
        usage = null;
      }
      setState(() {
        _groups = nodes.groups;
        _projectedMonthlyCents = nodes.projectedMonthlyCents;
        _quota = quota;
        _usage = usage;
      });
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _scale() async {
    final orgId = ref.read(orgsProvider).activeOrgId;
    if (orgId == null) return;

    setState(() => _scaling = true);
    try {
      await ref.read(orgResourcesServiceProvider).scaleNodes(
            orgId,
            api.ScaleOrgNodesRequest(
              backend: _scaleBackend,
              kind: _scaleKind,
              desired: _scaleDesired < 0 ? 0 : _scaleDesired,
            ),
          );
      await _refresh();
    } finally {
      if (mounted) setState(() => _scaling = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(orgsProvider.select((s) => s.activeOrgId), (prev, next) {
      if (prev != next) _refresh();
    });

    final org = ref.watch(orgsProvider.notifier).activeOrg();
    final isAdmin = ref.watch(orgsProvider.notifier).isActiveOrgAdmin();

    if (org == null) {
      return const Padding(
        padding: EdgeInsets.all(24),
        child: EmptyState(message: 'Select an organization on the Organization tab to manage its resources.'),
      );
    }

    return Padding(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          PanelCard(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                PanelToolbar(
                  title: 'Resources — ${org.name}',
                  actions: [
                    CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: _loading ? null : _refresh),
                  ],
                ),
                if (_loading) const LinearProgressIndicator(minHeight: 2),
                const SizedBox(height: 12),
                Row(
                  children: [
                    Expanded(child: _Stat(label: 'Projected monthly', value: _fmtCents(_projectedMonthlyCents))),
                    Expanded(child: _Stat(label: 'Accrued (30d)', value: _fmtCents(_usage?.accruedCents ?? 0))),
                    Expanded(
                      child: _Stat(
                        label: 'Monthly budget',
                        value: _quota != null && _quota!.maxMonthlyCents > 0
                            ? _fmtCents(_quota!.maxMonthlyCents)
                            : 'unlimited',
                      ),
                    ),
                  ],
                ),
                if (_budgetPct != null) ...[
                  const SizedBox(height: 12),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(999),
                    child: LinearProgressIndicator(
                      minHeight: 8,
                      value: (_budgetPct!.clamp(0, 100)) / 100,
                      color: _budgetPct! >= 100 ? AppColors.dangerFg : AppColors.accent,
                      backgroundColor: AppColors.surfaceSubtle,
                    ),
                  ),
                ],
              ],
            ),
          ),
          const SizedBox(height: 12),
          Expanded(
            child: PanelCard(
              child: ListView(
                children: [
                  Text('Dedicated node pools', style: Theme.of(context).textTheme.titleSmall),
                  const SizedBox(height: 8),
                  if (_groups.isEmpty)
                    const Text('No dedicated node pools. Scale one below.', style: TextStyle(color: AppColors.textMuted))
                  else
                    CcDataTable(
                      columns: const ['Backend', 'Kind', 'Desired', 'Rate', 'Monthly'],
                      rows: [
                        for (final g in _groups)
                          [
                            g.backend,
                            g.kind,
                            g.desired.toString(),
                            '${_fmtCents(_rate(g.backend, g.kind))}/h',
                            _fmtCents(g.desired * _rate(g.backend, g.kind) * _hoursPerMonth),
                          ],
                      ],
                    ),
                  const SizedBox(height: 16),
                  Text('Node-hours (30d)', style: Theme.of(context).textTheme.titleSmall),
                  const SizedBox(height: 8),
                  if (_usage == null || _usage!.nodeHours.isEmpty)
                    const Text('No usage recorded yet.', style: TextStyle(color: AppColors.textMuted))
                  else
                    CcDataTable(
                      columns: const ['Kind', 'Node-hours'],
                      rows: [
                        for (final entry in _usage!.nodeHours.entries) [entry.key, entry.value.toStringAsFixed(2)],
                      ],
                    ),
                  if (isAdmin) ...[
                    const SizedBox(height: 16),
                    Text('Scale a pool', style: Theme.of(context).textTheme.titleSmall),
                    const SizedBox(height: 8),
                    Wrap(
                      spacing: 8,
                      runSpacing: 8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        DropdownButton<String>(
                          value: _scaleBackend,
                          items: const [
                            DropdownMenuItem(value: 'supervisor', child: Text('supervisor')),
                            DropdownMenuItem(value: 'kubernetes', child: Text('kubernetes')),
                          ],
                          onChanged: (v) => setState(() => _scaleBackend = v ?? _scaleBackend),
                        ),
                        DropdownButton<String>(
                          value: _scaleKind,
                          items: const [
                            DropdownMenuItem(value: 'worker', child: Text('worker')),
                            DropdownMenuItem(value: 'waker', child: Text('waker')),
                            DropdownMenuItem(value: 'webservice', child: Text('webservice')),
                          ],
                          onChanged: (v) => setState(() => _scaleKind = v ?? _scaleKind),
                        ),
                        SizedBox(
                          width: 90,
                          child: TextField(
                            keyboardType: TextInputType.number,
                            decoration: const InputDecoration(isDense: true),
                            controller: TextEditingController(text: _scaleDesired.toString()),
                            onChanged: (v) => _scaleDesired = int.tryParse(v) ?? _scaleDesired,
                          ),
                        ),
                        CcButton(
                          label: _scaling ? 'Scaling…' : 'Set desired',
                          variant: CcButtonVariant.primary,
                          dense: true,
                          onPressed: _scaling ? null : _scale,
                        ),
                        Text(
                          '≈ ${_fmtCents(_scaleDesired * _rate(_scaleBackend, _scaleKind) * _hoursPerMonth)}/mo',
                          style: const TextStyle(color: AppColors.textMuted, fontSize: 13),
                        ),
                      ],
                    ),
                  ],
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _Stat extends StatelessWidget {
  const _Stat({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label.toUpperCase(), style: const TextStyle(fontSize: 11, color: AppColors.textMuted, letterSpacing: 0.4)),
        const SizedBox(height: 4),
        Text(value, style: const TextStyle(fontSize: 22, fontWeight: FontWeight.w600)),
      ],
    );
  }
}
