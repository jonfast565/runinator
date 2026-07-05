import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/workflow_run_extras_service.dart';
import '../../core/utils/format.dart';
import '../../core/utils/status.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';

enum _TimelineFilter { all, running, failed, succeeded }

const _runningStatuses = {'running', 'waiting', 'queued', 'retrying'};
const _failedStatuses = {'failed', 'timed_out'};

class RunTimeline extends ConsumerStatefulWidget {
  const RunTimeline({
    super.key,
    required this.detail,
    this.selectedNodeId,
    this.autoExpandFailed = false,
    this.filterable = false,
    required this.onSelect,
    this.nodeActionsBuilder,
  });

  final WorkflowRunDetail? detail;
  final String? selectedNodeId;
  final bool autoExpandFailed;
  final bool filterable;
  final ValueChanged<String> onSelect;
  final Widget Function(BuildContext context, WorkflowNodeRun node)? nodeActionsBuilder;

  @override
  ConsumerState<RunTimeline> createState() => _RunTimelineState();
}

class _RunTimelineState extends ConsumerState<RunTimeline> {
  _TimelineFilter _filter = _TimelineFilter.all;
  String? _expandedId;
  final _logCache = <String, String>{};
  final _logLoading = <String>{};
  var _copied = false;
  var _now = DateTime.now().millisecondsSinceEpoch;
  Timer? _clock;

  @override
  void initState() {
    super.initState();
    _clock = Timer.periodic(const Duration(seconds: 1), (_) {
      if (_runInFlight && mounted) {
        setState(() => _now = DateTime.now().millisecondsSinceEpoch);
      }
    });
  }

  @override
  void dispose() {
    _clock?.cancel();
    super.dispose();
  }

  @override
  void didUpdateWidget(RunTimeline oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.detail?.run.id != widget.detail?.run.id) {
      _logCache.clear();
      _logLoading.clear();
      _expandedId = null;
    }
    if (widget.autoExpandFailed) {
      final failure = _failure;
      if (failure != null) {
        final node = _orderedNodes.where((n) => n.nodeId == failure.nodeId).firstOrNull;
        if (node != null) {
          _expandedId = node.id;
          _loadLogs(node.id);
        }
      }
    }
  }

  bool get _runInFlight {
    final status = widget.detail?.run.status;
    return status != null && !isTerminalWorkflowRunStatus(status);
  }

  List<WorkflowNodeRun> get _orderedNodes {
    final nodes = [...?widget.detail?.nodes];
    nodes.sort((left, right) {
      final leftCreated = DateTime.tryParse(left.createdAt ?? '')?.millisecondsSinceEpoch;
      final rightCreated = DateTime.tryParse(right.createdAt ?? '')?.millisecondsSinceEpoch;
      if (leftCreated != null && rightCreated != null && leftCreated != rightCreated) {
        return leftCreated.compareTo(rightCreated);
      }
      return left.id.compareTo(right.id);
    });
    return nodes;
  }

  bool _matchesFilter(WorkflowNodeRun node, [_TimelineFilter? active]) {
    switch (active ?? _filter) {
      case _TimelineFilter.all:
        return true;
      case _TimelineFilter.failed:
        return _failedStatuses.contains(node.status);
      case _TimelineFilter.succeeded:
        return node.status == 'succeeded';
      case _TimelineFilter.running:
        return _runningStatuses.contains(node.status) || _isActive(node);
    }
  }

  int _count(_TimelineFilter filter) => _orderedNodes.where((node) => _matchesFilter(node, filter)).length;

  bool _isActive(WorkflowNodeRun node) {
    final active = widget.detail?.run.activeNodeId;
    if (active != null && active == node.nodeId) {
      return !_failedStatuses.contains(node.status) && node.status != 'succeeded';
    }
    return _runningStatuses.contains(node.status);
  }

  _FailureInfo? get _failure {
    final detail = widget.detail;
    if (detail == null) return null;

    final runFailed = _failedStatuses.contains(detail.run.status);
    WorkflowNodeRun? failedNode;
    for (final node in _orderedNodes.reversed) {
      if (_failedStatuses.contains(node.status)) {
        failedNode = node;
        break;
      }
    }

    if (!runFailed && failedNode == null) return null;

    if (failedNode != null) {
      return _FailureInfo(
        nodeId: failedNode.nodeId,
        status: failedNode.status ?? 'failed',
        message: formatErrorMessage(failedNode.message ?? detail.run.message).isEmpty
            ? 'Run failed.'
            : formatErrorMessage(failedNode.message ?? detail.run.message),
      );
    }

    return _FailureInfo(
      nodeId: detail.run.activeNodeId ?? 'run',
      status: detail.run.status ?? 'failed',
      message: formatErrorMessage(detail.run.message).isEmpty ? 'Run failed.' : formatErrorMessage(detail.run.message),
    );
  }

  Future<void> _loadLogs(String nodeRunId) async {
    if (_logCache.containsKey(nodeRunId) || _logLoading.contains(nodeRunId)) return;
    _logLoading.add(nodeRunId);
    setState(() {});

    try {
      final chunks = await ref.read(workflowRunExtrasServiceProvider).fetchNodeRunChunks(nodeRunId);
      _logCache[nodeRunId] = chunks.map((c) => c.content).join('');
    } catch (_) {
      _logCache[nodeRunId] = '';
    } finally {
      _logLoading.remove(nodeRunId);
      if (mounted) setState(() {});
    }
  }

  Future<void> _copyFailure(String message) async {
    await Clipboard.setData(ClipboardData(text: message));
    setState(() => _copied = true);
    Future.delayed(const Duration(milliseconds: 1200), () {
      if (mounted) setState(() => _copied = false);
    });
  }

  String _previewOf(WorkflowNodeRun node) {
    final output = node.outputJson;
    if (output == null) return '';
    final text = output is String ? output : jsonEncode(output);
    final oneLine = text.replaceAll(RegExp(r'\s+'), ' ').trim();
    if (oneLine.isEmpty || oneLine == '{}' || oneLine == '""') return '';
    return oneLine.length > 140 ? '${oneLine.substring(0, 140)}…' : oneLine;
  }

  String _outputText(WorkflowNodeRun node) {
    final output = node.outputJson;
    if (output == null) return '';
    if (output is Map && output.isEmpty) return '';
    return const JsonEncoder.withIndent('  ').convert(output);
  }

  String _nodeTiming(WorkflowNodeRun node) {
    final started = DateTime.tryParse(node.startedAt ?? '');
    if (started == null) return '';

    if (node.finishedAt != null) {
      final finished = DateTime.tryParse(node.finishedAt!);
      if (finished == null) return '';
      return _formatMs(finished.difference(started).inMilliseconds);
    }

    if (_isActive(node)) {
      return _formatMs(DateTime.fromMillisecondsSinceEpoch(_now).difference(started).inMilliseconds);
    }

    return '';
  }

  String _formatMs(int ms) {
    if (ms < 1000) return '${ms}ms';
    final seconds = ms / 1000;
    if (seconds < 60) return '${seconds.toStringAsFixed(seconds < 10 ? 1 : 0)}s';
    final minutes = seconds ~/ 60;
    final remSec = (seconds % 60).round();
    return remSec == 0 ? '${minutes}m' : '${minutes}m ${remSec}s';
  }

  @override
  Widget build(BuildContext context) {
    final detail = widget.detail;
    if (detail == null) {
      return Text('No run selected.', style: TextStyle(color: AppColors.textMuted, fontSize: 13));
    }

    final visible = _orderedNodes.where((node) => _matchesFilter(node)).toList();
    final failure = _failure;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (failure != null) ...[
          Container(
            padding: const EdgeInsets.all(10),
            decoration: BoxDecoration(
              color: AppColors.dangerBg,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: AppColors.dangerFg.withValues(alpha: 0.4)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    CcIcon(IconName.alert, size: 14, color: AppColors.dangerFg),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        'Failed at ${failure.nodeId}',
                        style: TextStyle(color: AppColors.dangerFg, fontWeight: FontWeight.w600, fontSize: 13),
                      ),
                    ),
                    StatusBadge(failure.status),
                    if (failure.message.isNotEmpty)
                      TextButton(
                        onPressed: () => _copyFailure(failure.message),
                        child: Text(_copied ? 'Copied' : 'Copy'),
                      ),
                  ],
                ),
                if (failure.message.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  SelectableText(
                    failure.message,
                    style: TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 11, color: AppColors.dangerFg),
                  ),
                ],
              ],
            ),
          ),
          const SizedBox(height: 8),
        ],
        if (widget.filterable && _orderedNodes.isNotEmpty)
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (final option in [
                (_TimelineFilter.all, 'All'),
                (_TimelineFilter.running, 'Active'),
                (_TimelineFilter.failed, 'Failed'),
                (_TimelineFilter.succeeded, 'OK'),
              ])
                FilterChip(
                  label: Text('${option.$2} ${_count(option.$1)}'),
                  selected: _filter == option.$1,
                  onSelected: (_) => setState(() => _filter = option.$1),
                ),
            ],
          ),
        if (visible.isEmpty)
          Text(
            _orderedNodes.isEmpty ? 'No steps recorded yet.' : 'No steps match this filter.',
            style: TextStyle(color: AppColors.textMuted, fontSize: 13),
          )
        else
          Expanded(
            child: ListView.builder(
              itemCount: visible.length,
              itemBuilder: (context, index) {
                final node = visible[index];
                final selected = node.nodeId == widget.selectedNodeId;
                final expanded = _expandedId == node.id;
                final timing = _nodeTiming(node);
                final preview = _previewOf(node);

                return Padding(
                  padding: const EdgeInsets.only(bottom: 6),
                  child: Material(
                    color: selected ? AppColors.accentSoft : Colors.transparent,
                    borderRadius: BorderRadius.circular(6),
                    child: InkWell(
                      borderRadius: BorderRadius.circular(6),
                      onTap: () {
                        widget.onSelect(node.nodeId);
                        setState(() {
                          _expandedId = expanded ? null : node.id;
                        });
                        if (!expanded) _loadLogs(node.id);
                      },
                      child: Padding(
                        padding: const EdgeInsets.all(8),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Row(
                              children: [
                                StatusBadge(node.status),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: Text(node.nodeId, style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 12)),
                                ),
                                if (_isActive(node))
                                  Container(
                                    padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
                                    decoration: BoxDecoration(color: AppColors.accentSoft, borderRadius: BorderRadius.circular(999)),
                                    child: Text('active', style: TextStyle(fontSize: 10, color: AppColors.accent)),
                                  ),
                                if (timing.isNotEmpty) ...[
                                  const SizedBox(width: 8),
                                  Text(timing, style: TextStyle(fontSize: 11, color: _isActive(node) ? AppColors.accent : AppColors.textMuted)),
                                ],
                                Icon(expanded ? Icons.expand_more : Icons.chevron_right, size: 16, color: AppColors.textMuted),
                              ],
                            ),
                            if (preview.isNotEmpty)
                              Padding(
                                padding: const EdgeInsets.only(top: 4),
                                child: Text(
                                  preview,
                                  style: TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 11, color: AppColors.textSubtle),
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                ),
                              ),
                            if (selected && widget.nodeActionsBuilder != null)
                              Padding(padding: const EdgeInsets.only(top: 6), child: widget.nodeActionsBuilder!(context, node)),
                            if (expanded) ...[
                              const SizedBox(height: 8),
                              if (node.message != null && node.message!.isNotEmpty && !_failedStatuses.contains(node.status)) ...[
                                Text('MESSAGE', style: TextStyle(fontSize: 10, color: AppColors.textMuted)),
                                Text(formatErrorMessage(node.message) ?? node.message!, style: const TextStyle(fontSize: 12)),
                                const SizedBox(height: 8),
                              ],
                              if (_outputText(node).isNotEmpty) ...[
                                Text('OUTPUT', style: TextStyle(fontSize: 10, color: AppColors.textMuted)),
                                SelectableText(_outputText(node), style: const TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 11)),
                                const SizedBox(height: 8),
                              ],
                              Text('LOGS', style: TextStyle(fontSize: 10, color: AppColors.textMuted)),
                              SelectableText(
                                _logLoading.contains(node.id)
                                    ? 'Loading logs…'
                                    : (_logCache[node.id]?.isNotEmpty == true ? _logCache[node.id]! : 'No logs for this step.'),
                                style: const TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 11),
                              ),
                            ],
                          ],
                        ),
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
      ],
    );
  }
}

class _FailureInfo {
  const _FailureInfo({required this.nodeId, required this.status, required this.message});

  final String nodeId;
  final String status;
  final String message;
}

extension _FirstOrNull<E> on Iterable<E> {
  E? get firstOrNull {
    final iterator = this.iterator;
    if (!iterator.moveNext()) return null;
    return iterator.current;
  }
}
