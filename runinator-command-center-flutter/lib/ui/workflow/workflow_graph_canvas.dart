import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
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
    this.onConnectWithOption,
  });

  final List<GraphNodeModel> nodes;
  final List<GraphEdgeModel> edges;
  final String? selectedNodeId;
  final String? selectedEdgeId;
  final bool readOnly;
  final void Function(String nodeId, {bool shiftKey})? onNodeClick;
  final ValueChanged<String>? onNodeDoubleClick;
  final void Function(String nodeId, GraphPosition position)? onNodeDragEnd;
  final ValueChanged<String>? onEdgeClick;
  final VoidCallback? onPaneClick;
  final void Function(String nodeId, Offset position, {required bool deletable})? onNodeContextMenu;
  final void Function(String edgeId, Offset position)? onEdgeContextMenu;
  /// generic connect from a node's body/fallback handle; the caller resolves the semantic
  /// route (e.g. by opening a "connect as" picker), as [option] is unknown at drop time.
  final void Function(String sourceId, String targetId, Offset position)? onConnect;
  /// connect from a specific per-parameter handle (see [WorkflowSemanticHandle]); the
  /// semantic route is already known, so the caller can apply it directly.
  final void Function(String sourceId, String targetId, String optionId, Offset position)? onConnectWithOption;

  @override
  State<WorkflowGraphCanvas> createState() => _WorkflowGraphCanvasState();
}

class _WorkflowGraphCanvasState extends State<WorkflowGraphCanvas> {
  final TransformationController _transform = TransformationController();
  final Map<String, Offset> _dragOffsets = {};
  String? _connectSourceId;
  String? _connectOptionId;
  Offset? _connectOrigin;
  Offset? _connectPointer;

  // on a phone, a workflow rendered at the default 1:1 scale mostly overflows the
  // viewport, so the user has to pinch-zoom-out before they can even see their
  // graph. auto-fit once on first layout, but never again once they've touched
  // the canvas themselves (that would fight their own zoom/pan).
  var _didAutoFit = false;
  var _userInteracted = false;
  Size _viewportSize = Size.zero;

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

  void _fitToView(Size viewportSize) {
    final bounds = _contentBounds();
    if (viewportSize.width <= 0 || viewportSize.height <= 0 || bounds.width <= 0 || bounds.height <= 0) return;

    final scale = math.min(viewportSize.width / bounds.width, viewportSize.height / bounds.height).clamp(0.2, 1.0);
    setState(() {
      _transform.value = Matrix4.identity()
        ..translateByDouble(viewportSize.width / 2, viewportSize.height / 2, 0, 1)
        ..scaleByDouble(scale, scale, scale, 1)
        ..translateByDouble(-bounds.center.dx, -bounds.center.dy, 0, 1);
    });
  }

  void _zoomAroundCenter(double factor) {
    if (_viewportSize.width <= 0 || _viewportSize.height <= 0) return;
    final center = Offset(_viewportSize.width / 2, _viewportSize.height / 2);
    final scenePoint = _transform.toScene(center);
    final newScale = (_transform.value.getMaxScaleOnAxis() * factor).clamp(0.2, 2.0);
    setState(() {
      _userInteracted = true;
      _transform.value = Matrix4.identity()
        ..translateByDouble(center.dx, center.dy, 0, 1)
        ..scaleByDouble(newScale, newScale, newScale, 1)
        ..translateByDouble(-scenePoint.dx, -scenePoint.dy, 0, 1);
    });
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
    final optionId = _connectOptionId;
    final pointer = _connectPointer;
    setState(() {
      _connectSourceId = null;
      _connectOptionId = null;
      _connectOrigin = null;
      _connectPointer = null;
    });

    if (sourceId == null || pointer == null) return;

    final target = _nodeAtCanvasPoint(_toCanvas(global, context));
    if (target == null || target.id == sourceId) return;

    if (optionId != null && widget.onConnectWithOption != null) {
      widget.onConnectWithOption!(sourceId, target.id, optionId, global);
    } else {
      widget.onConnect?.call(sourceId, target.id, global);
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
        decoration: BoxDecoration(color: AppColors.workflowCanvasBg),
        child: LayoutBuilder(
          builder: (context, constraints) {
            _viewportSize = constraints.biggest;

            if (!_didAutoFit && !_userInteracted && widget.nodes.isNotEmpty && _viewportSize.width > 0 && _viewportSize.height > 0) {
              _didAutoFit = true;
              WidgetsBinding.instance.addPostFrameCallback((_) {
                if (mounted) _fitToView(_viewportSize);
              });
            }

            return Stack(
              children: [
                InteractiveViewer(
                  transformationController: _transform,
                  minScale: 0.2,
                  maxScale: 2,
                  boundaryMargin: const EdgeInsets.all(200),
                  onInteractionStart: (_) => _userInteracted = true,
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
                              connectOrigin: _connectOrigin,
                              connectPointer: _connectPointer,
                            ),
                          ),
                          for (final node in widget.nodes)
                            _WorkflowNodeWidget(
                              node: node,
                              selected: widget.selectedNodeId == node.id,
                              readOnly: widget.readOnly,
                              dragOffset: _dragOffsets[node.id] ?? Offset.zero,
                              onTap: () => widget.onNodeClick?.call(node.id, shiftKey: HardwareKeyboard.instance.isShiftPressed),
                              onDoubleTap: () => widget.onNodeDoubleClick?.call(node.id),
                              onContextMenu: widget.onNodeContextMenu == null
                                  ? null
                                  : (globalPosition) => widget.onNodeContextMenu!(
                                        node.id,
                                        globalPosition,
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
                      onConnectStart: widget.readOnly || (widget.onConnect == null && widget.onConnectWithOption == null)
                          ? null
                          : (optionId, handleDx) {
                              setState(() {
                                _connectSourceId = node.id;
                                _connectOptionId = optionId;
                                _connectOrigin = Offset(node.position.x + handleDx, node.position.y + nodeHeight);
                                _connectPointer = _connectOrigin;
                              });
                            },
                      onConnectUpdate: widget.readOnly || (widget.onConnect == null && widget.onConnectWithOption == null)
                          ? null
                          : (global) {
                              setState(() => _connectPointer = _toCanvas(global, context));
                            },
                              onConnectEnd: widget.readOnly || (widget.onConnect == null && widget.onConnectWithOption == null)
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
                Positioned(
                  right: 10,
                  bottom: 10,
                  child: _CanvasZoomControls(
                    onZoomIn: () => _zoomAroundCenter(1.25),
                    onZoomOut: () => _zoomAroundCenter(0.8),
                    onFit: () => _fitToView(_viewportSize),
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}

class _CanvasZoomControls extends StatelessWidget {
  const _CanvasZoomControls({required this.onZoomIn, required this.onZoomOut, required this.onFit});

  final VoidCallback onZoomIn;
  final VoidCallback onZoomOut;
  final VoidCallback onFit;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: AppColors.surface,
      elevation: 2,
      borderRadius: BorderRadius.circular(AppMetrics.radiusSm),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _ZoomButton(icon: Icons.remove, tooltip: 'Zoom out', onTap: onZoomOut),
          _ZoomButton(icon: Icons.crop_free, tooltip: 'Fit to view', onTap: onFit),
          _ZoomButton(icon: Icons.add, tooltip: 'Zoom in', onTap: onZoomIn),
        ],
      ),
    );
  }
}

/// 44x44 touch target per Material guidance, even though the visible icon is small —
/// this canvas otherwise only exposes zoom via pinch, which is easy to miss on mobile.
class _ZoomButton extends StatelessWidget {
  const _ZoomButton({required this.icon, required this.tooltip, required this.onTap});

  final IconData icon;
  final String tooltip;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: SizedBox(
        width: 44,
        height: 44,
        child: InkWell(
          onTap: onTap,
          child: Icon(icon, size: 18, color: AppColors.textMuted),
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
    this.onContextMenu,
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
  /// opens the node's context menu; wired to both right-click (desktop) and
  /// long-press (touch has no secondary click) at the same position.
  final ValueChanged<Offset>? onContextMenu;
  final ValueChanged<Offset>? onDragUpdate;
  final VoidCallback? onDragEnd;
  /// starts a connection drag; [optionId] is the specific semantic route this handle
  /// represents (null for the generic fallback handle, which leaves route selection to a
  /// "connect as" picker), [handleDx] is the handle's x offset within the node so the
  /// canvas can anchor the preview line precisely.
  final void Function(String? optionId, double handleDx)? onConnectStart;
  final ValueChanged<Offset>? onConnectUpdate;
  final ValueChanged<Offset>? onConnectEnd;

  Widget _buildHandle({required double dx, required String? optionId, required String label, bool small = false}) {
    final size = small ? 12.0 : 16.0;
    // the visible dot stays small so it doesn't clutter a 180px node, but the actual
    // drag target is 40px — a bare 12-16px circle is very hard to grab with a finger.
    const hitSize = 40.0;
    return Positioned(
      left: dx - hitSize / 2,
      bottom: -(hitSize / 2),
      child: Tooltip(
        message: label,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onPanStart: (_) => onConnectStart!(optionId, dx),
          onPanUpdate: (details) => onConnectUpdate?.call(details.globalPosition),
          onPanEnd: (details) => onConnectEnd?.call(details.globalPosition),
          child: SizedBox(
            width: hitSize,
            height: hitSize,
            child: Center(
              child: Container(
                width: size,
                height: size,
                decoration: BoxDecoration(
                  color: AppColors.accent,
                  shape: BoxShape.circle,
                  border: Border.all(color: AppColors.surface, width: small ? 1.5 : 2),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final left = node.position.x + dragOffset.dx;
    final top = node.position.y + dragOffset.dy;
    final data = node.data;
    final borderColor = selected ? AppColors.accent : AppColors.workflowNodeBorder;
    final bg = data.running ? AppColors.accentSoft : AppColors.surface;

    // per-parameter handles: only surface distinct drag points for a node's *routing*
    // options (branch/switch-case/bucket/parallel-branch/etc) — every node also carries
    // five rarely-used direct-transition options (next/on_success/on_failure/...), which
    // would clutter a 180px-wide node if all shown individually; those stay reachable via
    // the single fallback handle's "connect as" picker.
    final routingHandles = data.semanticHandles
        .where((handle) =>
            handle.type == WorkflowSemanticHandleType.source &&
            handle.semanticOptionId != null &&
            !handle.semanticOptionId!.startsWith('direct:'))
        .toList();

    final connectHandles = <Widget>[];
    if (!readOnly && onConnectStart != null) {
      if (routingHandles.isEmpty) {
        connectHandles.add(_buildHandle(dx: nodeWidth / 2, optionId: null, label: 'Connect'));
      } else {
        for (var i = 0; i < routingHandles.length; i++) {
          final dx = nodeWidth * (i + 1) / (routingHandles.length + 1);
          connectHandles.add(_buildHandle(dx: dx, optionId: routingHandles[i].semanticOptionId, label: routingHandles[i].label, small: true));
        }
      }
    }

    return Positioned(
      left: left,
      top: top,
      width: nodeWidth,
      child: GestureDetector(
        onTap: onTap,
        onDoubleTap: onDoubleTap,
        onSecondaryTapDown: onContextMenu == null ? null : (details) => onContextMenu!(details.globalPosition),
        onLongPressStart: onContextMenu == null ? null : (details) => onContextMenu!(details.globalPosition),
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
                          CcIcon(IconName.alert, size: 14, color: AppColors.dangerFg),
                        if (data.debugBreakpoint)
                          const CcIcon(IconName.breakpoint, size: 14, color: AppColors.dangerSolid),
                      ],
                    ),
                    const SizedBox(height: 4),
                    Text(data.kind, style: TextStyle(fontSize: 10, color: AppColors.textMuted)),
                    if (data.summary.isNotEmpty) ...[
                      const SizedBox(height: 4),
                      Text(data.summary, style: TextStyle(fontSize: 11, color: AppColors.textSubtle), maxLines: 2, overflow: TextOverflow.ellipsis),
                    ],
                    if (data.statusLabel != null) ...[
                      const SizedBox(height: 6),
                      StatusBadge(data.statusLabel),
                    ],
                  ],
                ),
              ),
            ),
            // target handle: a visual cue that connections land on this node (mirrors the
            // `target:in` semantic handle); the whole node body remains the actual drop target.
            Positioned(
              left: nodeWidth / 2 - 5,
              top: -5,
              child: IgnorePointer(
                child: SizedBox(
                  width: 10,
                  height: 10,
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: AppColors.surface,
                      border: Border.fromBorderSide(BorderSide(color: AppColors.workflowNodeBorder, width: 1.5)),
                    ),
                  ),
                ),
              ),
            ),
            ...connectHandles,
          ],
        ),
      ),
    );
  }
}

/// builds the edge geometry for a given [style], matching the vue canvas's bezier/
/// straight/step (here "square") edge kinds. shared by [_EdgePainter] (drawing) and
/// [_EdgeTapTarget] (hit-testing + label placement) so both agree on the same path.
Path buildWorkflowEdgePath(Offset start, Offset end, WorkflowEdgeStyle style) {
  final path = Path()..moveTo(start.dx, start.dy);
  final midY = (start.dy + end.dy) / 2;

  switch (style) {
    case WorkflowEdgeStyle.straight:
      path.lineTo(end.dx, end.dy);
    case WorkflowEdgeStyle.square:
      path.lineTo(start.dx, midY);
      path.lineTo(end.dx, midY);
      path.lineTo(end.dx, end.dy);
    case WorkflowEdgeStyle.bezier:
      path.cubicTo(start.dx, midY, end.dx, midY, end.dx, end.dy);
  }

  return path;
}

/// the point at fraction [t] (0..1) along [path]'s total length; falls back to a
/// straight-line lerp between [start]/[end] if the path has no measurable length.
Offset pointAtFraction(Path path, double t, Offset start, Offset end) {
  final metrics = path.computeMetrics().toList();
  final totalLength = metrics.fold<double>(0, (sum, metric) => sum + metric.length);
  if (metrics.isEmpty || totalLength <= 0) {
    return Offset.lerp(start, end, t.clamp(0.0, 1.0))!;
  }

  var target = totalLength * t.clamp(0.0, 1.0);
  for (final metric in metrics) {
    if (target <= metric.length) {
      return metric.getTangentForOffset(target)?.position ?? end;
    }
    target -= metric.length;
  }
  return end;
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
    final style = edge.data.edgeStyle ?? WorkflowEdgeStyle.square;
    final path = buildWorkflowEdgePath(start, end, style);
    final anchorT = edge.data.labelAnchor?.position ?? 0.5;
    var anchor = pointAtFraction(path, anchorT, start, end);
    final labelOffset = edge.data.labelOffset;
    if (labelOffset != null) {
      anchor = anchor.translate(labelOffset.x, labelOffset.y);
    }

    final label = edge.label;

    return Positioned(
      left: anchor.dx,
      top: anchor.dy,
      child: FractionalTranslation(
        translation: const Offset(-0.5, -0.5),
        child: GestureDetector(
          onTap: onTap,
          onSecondaryTapDown: onSecondaryTap == null ? null : (details) => onSecondaryTap!(details.globalPosition),
          onLongPressStart: onSecondaryTap == null ? null : (details) => onSecondaryTap!(details.globalPosition),
          child: label == null || label.isEmpty
              ? const SizedBox(width: 32, height: 32)
              : Container(
                  constraints: const BoxConstraints(minWidth: 32, minHeight: 22),
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
                  decoration: BoxDecoration(
                    color: AppColors.workflowCanvasBg,
                    borderRadius: BorderRadius.circular(4),
                    border: Border.all(color: AppColors.workflowNodeBorder),
                  ),
                  child: Text(
                    label,
                    textAlign: TextAlign.center,
                    style: TextStyle(fontSize: 10, color: AppColors.textMuted),
                  ),
                ),
        ),
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
    this.connectOrigin,
    this.connectPointer,
  });

  final List<GraphNodeModel> nodes;
  final List<GraphEdgeModel> edges;
  final String? selectedEdgeId;
  final Map<String, Offset> dragOffsets;
  final Offset? connectOrigin;
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

      final style = edge.data.edgeStyle ?? WorkflowEdgeStyle.square;
      canvas.drawPath(buildWorkflowEdgePath(start, end, style), paint);

      canvas.drawCircle(end, 3, Paint()..color = paint.color);
    }

    if (connectOrigin != null && connectPointer != null) {
      final paint = Paint()
        ..color = AppColors.accent
        ..strokeWidth = 2
        ..style = PaintingStyle.stroke;
      canvas.drawLine(connectOrigin!, connectPointer!, paint);
    }
  }

  @override
  bool shouldRepaint(covariant _EdgePainter oldDelegate) =>
      oldDelegate.nodes != nodes ||
      oldDelegate.edges != edges ||
      oldDelegate.selectedEdgeId != selectedEdgeId ||
      oldDelegate.dragOffsets != dragOffsets ||
      oldDelegate.connectOrigin != connectOrigin ||
      oldDelegate.connectPointer != connectPointer;
}
