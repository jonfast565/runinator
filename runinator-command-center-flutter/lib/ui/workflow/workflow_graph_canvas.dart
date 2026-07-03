import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../core/domain/icons.dart';
import '../../core/workflow/graph_model.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';

const double nodeWidth = 180;
const double nodeHeight = 72;
const double gridSize = 15;

class WorkflowGraphCanvas extends StatefulWidget {
  const WorkflowGraphCanvas({
    super.key,
    required this.nodes,
    required this.edges,
    this.selectedNodeId,
    this.selectedEdgeId,
    this.readOnly = false,
    this.onNodeClick,
    this.onNodeDoubleClick,
    this.onNodeDragEnd,
    this.onEdgeClick,
    this.onPaneClick,
    this.onNodeContextMenu,
    this.onEdgeContextMenu,
    this.onConnect,
  });

  final List<GraphNodeModel> nodes;
  final List<GraphEdgeModel> edges;
  final String? selectedNodeId;
  final String? selectedEdgeId;
  final bool readOnly;
  final ValueChanged<String>? onNodeClick;
  final ValueChanged<String>? onNodeDoubleClick;
  final void Function(String nodeId, GraphPosition position)? onNodeDragEnd;
  final ValueChanged<String>? onEdgeClick;
  final VoidCallback? onPaneClick;
  final void Function(String nodeId, Offset position, {required bool deletable})? onNodeContextMenu;
  final void Function(String edgeId, Offset position)? onEdgeContextMenu;
  final void Function(String sourceId, String targetId, Offset position)? onConnect;

  @override
  State<WorkflowGraphCanvas> createState() => _WorkflowGraphCanvasState();
}

class _WorkflowGraphCanvasState extends State<WorkflowGraphCanvas> {
  final TransformationController _transform = TransformationController();
  final Map<String, Offset> _dragOffsets = {};
  String? _connectSourceId;
  Offset? _connectPointer;

  @override
  void dispose() {
    _transform.dispose();
    super.dispose();
  }

  Offset _snap(Offset value) {
    return Offset(
      (value.dx / gridSize).round() * gridSize,
      (value.dy / gridSize).round() * gridSize,
    );
  }

  Rect _contentBounds() {
    if (widget.nodes.isEmpty) {
      return const Rect.fromLTWH(0, 0, 800, 600);
    }

    var minX = double.infinity;
    var minY = double.infinity;
    var maxX = -double.infinity;
    var maxY = -double.infinity;

    for (final node in widget.nodes) {
      minX = math.min(minX, node.position.x);
      minY = math.min(minY, node.position.y);
      maxX = math.max(maxX, node.position.x + nodeWidth);
      maxY = math.max(maxY, node.position.y + nodeHeight);
    }

    return Rect.fromLTRB(minX - 120, minY - 120, maxX + 120, maxY + 120);
  }

  GraphNodeModel? _nodeAtCanvasPoint(Offset canvasPoint) {
    for (final node in widget.nodes) {
      final drag = _dragOffsets[node.id] ?? Offset.zero;
      final rect = Rect.fromLTWH(node.position.x + drag.dx, node.position.y + drag.dy, nodeWidth, nodeHeight);
      if (rect.contains(canvasPoint)) return node;
    }
    return null;
  }

  Offset _toCanvas(Offset global, BuildContext context) {
    final box = context.findRenderObject() as RenderBox?;
    if (box == null) return global;
    final local = box.globalToLocal(global);
    return _transform.toScene(local);
  }

  void _finishConnect(Offset global, BuildContext context) {
    final sourceId = _connectSourceId;
    final pointer = _connectPointer;
    setState(() {
      _connectSourceId = null;
      _connectPointer = null;
    });

    if (sourceId == null || pointer == null || widget.onConnect == null) return;

    final target = _nodeAtCanvasPoint(_toCanvas(global, context));
    if (target != null && target.id != sourceId) {
      widget.onConnect!(sourceId, target.id, global);
    }
  }

  @override
  Widget build(BuildContext context) {
    final bounds = _contentBounds();
    final canvasWidth = math.max(bounds.width, 800.0);
    final canvasHeight = math.max(bounds.height, 600.0);

    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: DecoratedBox(
        decoration: const BoxDecoration(color: AppColors.workflowCanvasBg),
        child: InteractiveViewer(
          transformationController: _transform,
          minScale: 0.2,
          maxScale: 2,
          boundaryMargin: const EdgeInsets.all(200),
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: widget.onPaneClick,
            child: SizedBox(
              width: canvasWidth,
              height: canvasHeight,
              child: Stack(
                clipBehavior: Clip.none,
                children: [
                  CustomPaint(
                    size: Size(canvasWidth, canvasHeight),
                    painter: _GridPainter(),
                  ),
                  CustomPaint(
                    size: Size(canvasWidth, canvasHeight),
                    painter: _EdgePainter(
                      nodes: widget.nodes,
                      edges: widget.edges,
                      selectedEdgeId: widget.selectedEdgeId,
                      dragOffsets: _dragOffsets,
                      connectSourceId: _connectSourceId,
                      connectPointer: _connectPointer,
                    ),
                  ),
                  for (final node in widget.nodes)
                    _WorkflowNodeWidget(
                      node: node,
                      selected: widget.selectedNodeId == node.id,
                      readOnly: widget.readOnly,
                      dragOffset: _dragOffsets[node.id] ?? Offset.zero,
                      onTap: () => widget.onNodeClick?.call(node.id),
                      onDoubleTap: () => widget.onNodeDoubleClick?.call(node.id),
                      onSecondaryTapDown: widget.onNodeContextMenu == null
                          ? null
                          : (details) => widget.onNodeContextMenu!(
                                node.id,
                                details.globalPosition,
                                deletable: !node.data.locked,
                              ),
                      onDragUpdate: widget.readOnly
                          ? null
                          : (delta) {
                              setState(() {
                                _dragOffsets[node.id] = (_dragOffsets[node.id] ?? Offset.zero) + delta;
                              });
                            },
                      onDragEnd: widget.readOnly
                          ? null
                          : () {
                              final base = Offset(node.position.x, node.position.y);
                              final snapped = _snap(base + (_dragOffsets[node.id] ?? Offset.zero));
                              _dragOffsets.remove(node.id);
                              widget.onNodeDragEnd?.call(
                                node.id,
                                GraphPosition(x: snapped.dx, y: snapped.dy),
                              );
                              setState(() {});
                            },
                      onConnectStart: widget.readOnly || widget.onConnect == null
                          ? null
                          : () {
                              setState(() {
                                _connectSourceId = node.id;
                                _connectPointer = Offset(node.position.x + nodeWidth / 2, node.position.y + nodeHeight);
                              });
                            },
                      onConnectUpdate: widget.readOnly || widget.onConnect == null
                          ? null
                          : (global) {
                              setState(() => _connectPointer = _toCanvas(global, context));
                            },
                      onConnectEnd: widget.readOnly || widget.onConnect == null
                          ? null
                          : (global) => _finishConnect(global, context),
                    ),
                  for (final edge in widget.edges)
                    _EdgeTapTarget(
                      edge: edge,
                      nodes: widget.nodes,
                      dragOffsets: _dragOffsets,
                      onTap: () => widget.onEdgeClick?.call(edge.id!),
                      onSecondaryTap: widget.onEdgeContextMenu == null
                          ? null
                          : (position) => widget.onEdgeContextMenu!(edge.id!, position),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _WorkflowNodeWidget extends StatelessWidget {
  const _WorkflowNodeWidget({
    required this.node,
    required this.selected,
    required this.readOnly,
    required this.dragOffset,
    this.onTap,
    this.onDoubleTap,
    this.onSecondaryTapDown,
    this.onDragUpdate,
    this.onDragEnd,
    this.onConnectStart,
    this.onConnectUpdate,
    this.onConnectEnd,
  });

  final GraphNodeModel node;
  final bool selected;
  final bool readOnly;
  final Offset dragOffset;
  final VoidCallback? onTap;
  final VoidCallback? onDoubleTap;
  final void Function(TapDownDetails details)? onSecondaryTapDown;
  final ValueChanged<Offset>? onDragUpdate;
  final VoidCallback? onDragEnd;
  final VoidCallback? onConnectStart;
  final ValueChanged<Offset>? onConnectUpdate;
  final ValueChanged<Offset>? onConnectEnd;

  @override
  Widget build(BuildContext context) {
    final left = node.position.x + dragOffset.dx;
    final top = node.position.y + dragOffset.dy;
    final data = node.data;
    final borderColor = selected ? AppColors.accent : AppColors.workflowNodeBorder;
    final bg = data.running ? AppColors.accentSoft : AppColors.surface;

    return Positioned(
      left: left,
      top: top,
      width: nodeWidth,
      child: GestureDetector(
        onTap: onTap,
        onDoubleTap: onDoubleTap,
        onSecondaryTapDown: onSecondaryTapDown,
        onPanUpdate: onDragUpdate == null ? null : (details) => onDragUpdate!(details.delta),
        onPanEnd: onDragEnd == null ? null : (_) => onDragEnd!(),
        child: Stack(
          clipBehavior: Clip.none,
          children: [
            Material(
              elevation: selected ? 3 : 1,
              borderRadius: BorderRadius.circular(8),
              color: bg,
              child: Container(
                padding: const EdgeInsets.all(10),
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: borderColor, width: selected ? 2 : 1),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Expanded(
                          child: Text(
                            data.title,
                            style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 12),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        if (data.validationCount > 0)
                          const CcIcon(IconName.alert, size: 14, color: AppColors.dangerFg),
                        if (data.debugBreakpoint)
                          const CcIcon(IconName.breakpoint, size: 14, color: AppColors.dangerSolid),
                      ],
                    ),
                    const SizedBox(height: 4),
                    Text(data.kind, style: const TextStyle(fontSize: 10, color: AppColors.textMuted)),
                    if (data.summary.isNotEmpty) ...[
                      const SizedBox(height: 4),
                      Text(data.summary, style: const TextStyle(fontSize: 11, color: AppColors.textSubtle), maxLines: 2, overflow: TextOverflow.ellipsis),
                    ],
                    if (data.statusLabel != null) ...[
                      const SizedBox(height: 6),
                      StatusBadge(data.statusLabel),
                    ],
                  ],
                ),
              ),
            ),
            if (!readOnly && onConnectStart != null)
              Positioned(
                left: nodeWidth / 2 - 8,
                bottom: -8,
                child: GestureDetector(
                  onPanStart: (_) => onConnectStart!(),
                  onPanUpdate: (details) => onConnectUpdate?.call(details.globalPosition),
                  onPanEnd: (details) => onConnectEnd?.call(details.globalPosition),
                  child: Container(
                    width: 16,
                    height: 16,
                    decoration: BoxDecoration(
                      color: AppColors.accent,
                      shape: BoxShape.circle,
                      border: Border.all(color: AppColors.surface, width: 2),
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _EdgeTapTarget extends StatelessWidget {
  const _EdgeTapTarget({
    required this.edge,
    required this.nodes,
    required this.dragOffsets,
    this.onTap,
    this.onSecondaryTap,
  });

  final GraphEdgeModel edge;
  final List<GraphNodeModel> nodes;
  final Map<String, Offset> dragOffsets;
  final VoidCallback? onTap;
  final ValueChanged<Offset>? onSecondaryTap;

  @override
  Widget build(BuildContext context) {
    final nodeById = {for (final node in nodes) node.id: node};
    final source = nodeById[edge.source];
    final target = nodeById[edge.target];
    if (source == null || target == null || edge.id == null) return const SizedBox.shrink();

    final sourceDrag = dragOffsets[source.id] ?? Offset.zero;
    final targetDrag = dragOffsets[target.id] ?? Offset.zero;
    final start = Offset(source.position.x + sourceDrag.dx + nodeWidth / 2, source.position.y + sourceDrag.dy + nodeHeight);
    final end = Offset(target.position.x + targetDrag.dx + nodeWidth / 2, target.position.y + targetDrag.dy);
    final mid = Offset((start.dx + end.dx) / 2, (start.dy + end.dy) / 2);

    return Positioned(
      left: mid.dx - 16,
      top: mid.dy - 16,
      child: GestureDetector(
        onTap: onTap,
        onSecondaryTapDown: onSecondaryTap == null ? null : (details) => onSecondaryTap!(details.globalPosition),
        child: const SizedBox(width: 32, height: 32),
      ),
    );
  }
}

class _GridPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = AppColors.border.withValues(alpha: 0.35)
      ..strokeWidth = 1;

    for (var x = 0.0; x < size.width; x += gridSize) {
      canvas.drawLine(Offset(x, 0), Offset(x, size.height), paint);
    }
    for (var y = 0.0; y < size.height; y += gridSize) {
      canvas.drawLine(Offset(0, y), Offset(size.width, y), paint);
    }
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

class _EdgePainter extends CustomPainter {
  _EdgePainter({
    required this.nodes,
    required this.edges,
    this.selectedEdgeId,
    this.dragOffsets = const {},
    this.connectSourceId,
    this.connectPointer,
  });

  final List<GraphNodeModel> nodes;
  final List<GraphEdgeModel> edges;
  final String? selectedEdgeId;
  final Map<String, Offset> dragOffsets;
  final String? connectSourceId;
  final Offset? connectPointer;

  @override
  void paint(Canvas canvas, Size size) {
    final nodeById = {for (final node in nodes) node.id: node};

    for (final edge in edges) {
      final source = nodeById[edge.source];
      final target = nodeById[edge.target];
      if (source == null || target == null) continue;

      final sourceDrag = dragOffsets[source.id] ?? Offset.zero;
      final targetDrag = dragOffsets[target.id] ?? Offset.zero;
      final start = Offset(source.position.x + sourceDrag.dx + nodeWidth / 2, source.position.y + sourceDrag.dy + nodeHeight);
      final end = Offset(target.position.x + targetDrag.dx + nodeWidth / 2, target.position.y + targetDrag.dy);

      final paint = Paint()
        ..color = edge.id == selectedEdgeId ? AppColors.accent : AppColors.workflowNodeBorder
        ..strokeWidth = edge.id == selectedEdgeId ? 2.5 : 1.5
        ..style = PaintingStyle.stroke;

      final path = Path();
      path.moveTo(start.dx, start.dy);
      final midY = (start.dy + end.dy) / 2;
      path.cubicTo(start.dx, midY, end.dx, midY, end.dx, end.dy);
      canvas.drawPath(path, paint);

      canvas.drawCircle(end, 3, Paint()..color = paint.color);
    }

    if (connectSourceId != null && connectPointer != null) {
      final source = nodeById[connectSourceId];
      if (source != null) {
        final sourceDrag = dragOffsets[source.id] ?? Offset.zero;
        final start = Offset(source.position.x + sourceDrag.dx + nodeWidth / 2, source.position.y + sourceDrag.dy + nodeHeight);
        final paint = Paint()
          ..color = AppColors.accent
          ..strokeWidth = 2
          ..style = PaintingStyle.stroke;
        canvas.drawLine(start, connectPointer!, paint);
      }
    }
  }

  @override
  bool shouldRepaint(covariant _EdgePainter oldDelegate) =>
      oldDelegate.nodes != nodes ||
      oldDelegate.edges != edges ||
      oldDelegate.selectedEdgeId != selectedEdgeId ||
      oldDelegate.dragOffsets != dragOffsets ||
      oldDelegate.connectSourceId != connectSourceId ||
      oldDelegate.connectPointer != connectPointer;
}
