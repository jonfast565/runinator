import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../theme/app_theme.dart';

/// dependency-free svg-style sparkline, mirroring the vue Sparkline.vue widget.
class Sparkline extends StatelessWidget {
  const Sparkline({
    super.key,
    required this.label,
    required this.values,
    this.color,
    this.unit = '',
    this.max,
    this.format,
  });

  final String label;
  final List<double> values;
  final Color? color;
  final String unit;
  final double? max;
  final String Function(double value)? format;

  static const _width = 200.0;
  static const _height = 40.0;

  @override
  Widget build(BuildContext context) {
    final resolvedColor = color ?? AppColors.accent;
    final points = values.where((v) => v.isFinite).toList();
    final latest = points.isEmpty ? null : points.last;
    final latestLabel = latest == null
        ? '—'
        : format != null
            ? format!(latest)
            : '${latest.toStringAsFixed(latest == latest.roundToDouble() ? 0 : 1)}$unit';

    return Container(
      padding: const EdgeInsets.all(8),
      decoration: BoxDecoration(
        color: AppColors.surfaceSubtle,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: AppColors.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(child: Text(label, style: const TextStyle(fontSize: 11, fontWeight: FontWeight.w600))),
              Text(latestLabel, style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
            ],
          ),
          const SizedBox(height: 6),
          if (points.length < 2)
            SizedBox(
              height: _height,
              child: Align(
                alignment: Alignment.centerLeft,
                child: Text('no samples yet', style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
              ),
            )
          else
            SizedBox(
              height: _height,
              child: CustomPaint(
                painter: _SparklinePainter(points: points, color: resolvedColor, max: max),
                size: const Size(_width, _height),
              ),
            ),
        ],
      ),
    );
  }
}

class _SparklinePainter extends CustomPainter {
  _SparklinePainter({required this.points, required this.color, this.max});

  final List<double> points;
  final Color color;
  final double? max;

  @override
  void paint(Canvas canvas, Size size) {
    final upper = max != null && max! > 0 ? max! : math.max(points.reduce(math.max), 0) * 1.1;
    final peak = upper <= 0 ? 1.0 : upper;
    final stepX = size.width / (points.length - 1);
    final coords = <Offset>[];

    for (var i = 0; i < points.length; i++) {
      final ratio = (points[i] / peak).clamp(0.0, 1.0);
      coords.add(Offset(i * stepX, size.height - ratio * size.height));
    }

    final linePath = Path()..moveTo(coords.first.dx, coords.first.dy);
    for (var i = 1; i < coords.length; i++) {
      linePath.lineTo(coords[i].dx, coords[i].dy);
    }

    final areaPath = Path()
      ..moveTo(coords.first.dx, size.height)
      ..addPath(linePath, Offset.zero)
      ..lineTo(coords.last.dx, size.height)
      ..close();

    canvas.drawPath(areaPath, Paint()..color = color.withValues(alpha: 0.12));
    canvas.drawPath(
      linePath,
      Paint()
        ..color = color
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.5,
    );
  }

  @override
  bool shouldRepaint(covariant _SparklinePainter oldDelegate) =>
      oldDelegate.points != points || oldDelegate.color != color || oldDelegate.max != max;
}

String formatRate(double bytesPerSec) {
  if (!bytesPerSec.isFinite || bytesPerSec <= 0) return '0 B/s';
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  var value = bytesPerSec;
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  final text = value < 10 && unit > 0 ? value.toStringAsFixed(1) : value.round().toString();
  return '$text ${units[unit]}';
}
