import 'package:flutter/material.dart';

import '../../core/navigation/breakpoints.dart';
import '../theme/app_theme.dart';
import 'cc_widgets.dart';

String? Function(String key)? _splitPaneReader;
void Function(String key, String value)? _splitPaneWriter;

void setSplitPaneStorage({
  String? Function(String key)? reader,
  void Function(String key, String value)? writer,
}) {
  _splitPaneReader = reader;
  _splitPaneWriter = writer;
}

class SplitPane extends StatefulWidget {
  const SplitPane({
    super.key,
    required this.first,
    required this.second,
    this.initialFirstFraction = 0.58,
    this.minFirst = 280,
    this.minSecond = 240,
    this.horizontal = true,
    this.storageKey,
    this.mobileShowSecond = false,
    this.onMobileBack,
    this.mobileBackTitle,
  });

  final Widget first;
  final Widget second;
  final double initialFirstFraction;
  final double minFirst;
  final double minSecond;
  final bool horizontal;
  final String? storageKey;

  /// on phone-width screens, a side-by-side (or even stacked-halves) master/detail
  /// split leaves both panes too cramped to use. when [onMobileBack] is provided,
  /// mobile shows exactly one full-screen pane at a time instead: [first] until
  /// [mobileShowSecond] flips true, then [second] behind a back button that calls
  /// [onMobileBack]. views that don't opt in keep the legacy stacked-halves layout.
  final bool mobileShowSecond;
  final VoidCallback? onMobileBack;
  final String? mobileBackTitle;

  @override
  State<SplitPane> createState() => _SplitPaneState();
}

class _SplitPaneState extends State<SplitPane> {
  late double _fraction;

  @override
  void initState() {
    super.initState();
    _fraction = widget.initialFirstFraction;
    final key = widget.storageKey;
    if (key != null) {
      final saved = _splitPaneReader?.call(key);
      final parsed = saved != null ? double.tryParse(saved) : null;
      if (parsed != null) {
        _fraction = parsed.clamp(0.05, 0.95);
      }
    }
  }

  void _persistFraction() {
    final key = widget.storageKey;
    if (key != null) {
      _splitPaneWriter?.call(key, _fraction.toStringAsFixed(4));
    }
  }

  @override
  Widget build(BuildContext context) {
    final onMobileBack = widget.onMobileBack;
    if (widget.horizontal && onMobileBack != null && MediaQuery.sizeOf(context).width <= Breakpoints.mobile) {
      if (!widget.mobileShowSecond) {
        return widget.first;
      }
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          MobileBackBar(onBack: onMobileBack, title: widget.mobileBackTitle),
          Expanded(child: widget.second),
        ],
      );
    }

    // below the tablet breakpoint, stack split panes instead of squeezing them side by side.
    final stacked = widget.horizontal && MediaQuery.sizeOf(context).width <= Breakpoints.tablet;
    final sideBySide = widget.horizontal && !stacked;

    return LayoutBuilder(
      builder: (context, constraints) {
        final total = sideBySide ? constraints.maxWidth : constraints.maxHeight;
        final minSpan = widget.minFirst + widget.minSecond;
        final firstSize = minSpan <= total
            ? (total * _fraction).clamp(widget.minFirst, total - widget.minSecond)
            : total / 2;

        void onDrag(double delta) {
          setState(() {
            _fraction = minSpan <= total
                ? ((firstSize + delta) / total).clamp(
                    widget.minFirst / total,
                    (total - widget.minSecond) / total,
                  )
                : 0.5;
          });
          _persistFraction();
        }

        if (sideBySide) {
          return Row(
            children: [
              SizedBox(width: firstSize, child: widget.first),
              _Divider(isVertical: true, onDrag: onDrag),
              Expanded(child: widget.second),
            ],
          );
        }

        return Column(
          children: [
            SizedBox(height: firstSize, child: widget.first),
            _Divider(isVertical: false, onDrag: onDrag),
            Expanded(child: widget.second),
          ],
        );
      },
    );
  }
}

class _Divider extends StatelessWidget {
  const _Divider({required this.isVertical, required this.onDrag});

  final bool isVertical;
  final ValueChanged<double> onDrag;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onPanUpdate: (details) => onDrag(isVertical ? details.delta.dx : details.delta.dy),
      child: MouseRegion(
        cursor: isVertical ? SystemMouseCursors.resizeColumn : SystemMouseCursors.resizeRow,
        child: Container(
          width: isVertical ? 6 : double.infinity,
          height: isVertical ? double.infinity : 6,
          color: AppColors.border.withValues(alpha: 0.5),
        ),
      ),
    );
  }
}

class DataTableShell extends StatelessWidget {
  const DataTableShell({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: SingleChildScrollView(child: child),
    );
  }
}

class CcDataTable extends StatelessWidget {
  const CcDataTable({
    super.key,
    required this.columns,
    required this.rows,
    this.selectedIndex,
    this.onSelect,
    this.emptyMessage = 'No rows.',
    this.rowColor,
  });

  final List<String> columns;
  final List<List<String>> rows;
  final int? selectedIndex;
  final ValueChanged<int>? onSelect;
  final String emptyMessage;
  final Color Function(int index)? rowColor;

  @override
  Widget build(BuildContext context) {
    if (rows.isEmpty) {
      return EmptyState(message: emptyMessage);
    }

    return DataTableShell(
      child: DataTable(
        headingRowHeight: 36,
        dataRowMinHeight: 34,
        dataRowMaxHeight: 48,
        columnSpacing: 16,
        columns: columns.map((label) => DataColumn(label: Text(label, style: const TextStyle(fontWeight: FontWeight.w700)))).toList(),
        rows: [
          for (var i = 0; i < rows.length; i++)
            DataRow(
              selected: selectedIndex == i,
              color: WidgetStateProperty.resolveWith((_) => rowColor?.call(i)),
              onSelectChanged: onSelect == null ? null : (_) => onSelect!(i),
              cells: rows[i].map((cell) => DataCell(Text(cell, style: const TextStyle(fontSize: 12)))).toList(),
            ),
        ],
      ),
    );
  }
}
