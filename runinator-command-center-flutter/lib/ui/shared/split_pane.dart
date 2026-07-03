import 'package:flutter/material.dart';

import '../theme/app_theme.dart';
import 'cc_widgets.dart';

class SplitPane extends StatefulWidget {
  const SplitPane({
    super.key,
    required this.first,
    required this.second,
    this.initialFirstFraction = 0.58,
    this.minFirst = 280,
    this.minSecond = 240,
    this.horizontal = true,
  });

  final Widget first;
  final Widget second;
  final double initialFirstFraction;
  final double minFirst;
  final double minSecond;
  final bool horizontal;

  @override
  State<SplitPane> createState() => _SplitPaneState();
}

class _SplitPaneState extends State<SplitPane> {
  late double _fraction;

  @override
  void initState() {
    super.initState();
    _fraction = widget.initialFirstFraction;
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final total = widget.horizontal ? constraints.maxWidth : constraints.maxHeight;
        final firstSize = (total * _fraction).clamp(widget.minFirst, total - widget.minSecond);

        if (widget.horizontal) {
          return Row(
            children: [
              SizedBox(width: firstSize, child: widget.first),
              _Divider(isVertical: true, onDrag: (delta) {
                setState(() {
                  _fraction = ((firstSize + delta) / total).clamp(
                    widget.minFirst / total,
                    (total - widget.minSecond) / total,
                  );
                });
              }),
              Expanded(child: widget.second),
            ],
          );
        }

        return Column(
          children: [
            SizedBox(height: firstSize, child: widget.first),
            _Divider(isVertical: false, onDrag: (delta) {
              setState(() {
                _fraction = ((firstSize + delta) / total).clamp(
                  widget.minFirst / total,
                  (total - widget.minSecond) / total,
                );
              });
            }),
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
