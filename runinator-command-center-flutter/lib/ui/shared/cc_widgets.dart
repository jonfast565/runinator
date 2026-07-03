import 'package:flutter/material.dart';

import '../../core/domain/icons.dart';
import '../theme/app_theme.dart';

IconData iconDataFor(IconName name) {
  switch (name) {
    case IconName.play:
      return Icons.play_arrow;
    case IconName.pause:
      return Icons.pause;
    case IconName.stop:
      return Icons.stop;
    case IconName.restart:
      return Icons.restart_alt;
    case IconName.replay:
      return Icons.replay;
    case IconName.step:
      return Icons.skip_next;
    case IconName.continue_:
      return Icons.fast_forward;
    case IconName.close:
    case IconName.x:
      return Icons.close;
    case IconName.plus:
      return Icons.add;
    case IconName.minus:
      return Icons.remove;
    case IconName.trash:
      return Icons.delete_outline;
    case IconName.edit:
      return Icons.edit_outlined;
    case IconName.save:
      return Icons.save_outlined;
    case IconName.download:
      return Icons.download_outlined;
    case IconName.upload:
      return Icons.upload_outlined;
    case IconName.check:
      return Icons.check;
    case IconName.alert:
      return Icons.warning_amber_outlined;
    case IconName.info:
      return Icons.info_outline;
    case IconName.search:
      return Icons.search;
    case IconName.file:
      return Icons.description_outlined;
    case IconName.folder:
      return Icons.folder_outlined;
    case IconName.bell:
      return Icons.notifications_outlined;
    case IconName.settings:
      return Icons.settings_outlined;
    case IconName.refresh:
      return Icons.refresh;
    case IconName.debug:
      return Icons.bug_report_outlined;
    case IconName.mail:
      return Icons.mail_outline;
    case IconName.approve:
      return Icons.check_circle_outline;
    case IconName.reject:
      return Icons.cancel_outlined;
    case IconName.arrowUp:
      return Icons.arrow_upward;
    case IconName.arrowDown:
      return Icons.arrow_downward;
    case IconName.chevronLeft:
      return Icons.chevron_left;
    case IconName.chevronRight:
      return Icons.chevron_right;
    case IconName.workflow:
      return Icons.account_tree_outlined;
    case IconName.runs:
      return Icons.play_circle_outline;
    case IconName.list:
      return Icons.list;
    case IconName.key:
      return Icons.key_outlined;
    case IconName.lock:
      return Icons.lock_outline;
    case IconName.box:
      return Icons.inventory_2_outlined;
    case IconName.message:
      return Icons.chat_bubble_outline;
    case IconName.gate:
      return Icons.door_sliding_outlined;
    case IconName.gear:
      return Icons.settings;
    case IconName.flag:
      return Icons.flag_outlined;
    case IconName.tag:
      return Icons.local_offer_outlined;
    case IconName.cursor:
      return Icons.ads_click;
    case IconName.skip:
      return Icons.redo;
    case IconName.circle:
      return Icons.circle_outlined;
    case IconName.dot:
      return Icons.fiber_manual_record;
    case IconName.breakpoint:
      return Icons.radio_button_checked;
    case IconName.bolt:
      return Icons.bolt;
    case IconName.clock:
      return Icons.schedule;
    case IconName.hourglass:
      return Icons.hourglass_empty;
    case IconName.branch:
      return Icons.call_split;
    case IconName.switch_:
      return Icons.swap_horiz;
    case IconName.toggle:
      return Icons.toggle_on_outlined;
    case IconName.percentage:
      return Icons.percent;
    case IconName.loop:
      return Icons.loop;
    case IconName.parallel:
      return Icons.view_column;
    case IconName.join:
      return Icons.merge;
    case IconName.shield:
      return Icons.shield_outlined;
    case IconName.user:
      return Icons.person_outline;
    case IconName.grid:
      return Icons.grid_view;
    case IconName.race:
      return Icons.speed;
    case IconName.emit:
      return Icons.outbound;
    case IconName.output:
      return Icons.output;
  }
}

class CcIcon extends StatelessWidget {
  const CcIcon(this.name, {super.key, this.size = 16, this.color});

  final IconName name;
  final double size;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    return Icon(iconDataFor(name), size: size, color: color);
  }
}

enum CcButtonVariant { normal, primary, danger }

class CcButton extends StatelessWidget {
  const CcButton({
    super.key,
    required this.label,
    this.icon,
    this.onPressed,
    this.variant = CcButtonVariant.normal,
    this.dense = false,
  });

  final String label;
  final IconName? icon;
  final VoidCallback? onPressed;
  final CcButtonVariant variant;
  final bool dense;

  @override
  Widget build(BuildContext context) {
    final enabled = onPressed != null;
    Color? bg;
    Color fg = AppColors.textPrimary;
    BorderSide border = const BorderSide(color: AppColors.borderStrong);

    switch (variant) {
      case CcButtonVariant.primary:
        bg = enabled ? AppColors.accent : AppColors.accent.withValues(alpha: 0.5);
        fg = Colors.white;
        border = BorderSide.none;
      case CcButtonVariant.danger:
        bg = enabled ? AppColors.dangerSolid : AppColors.dangerSolid.withValues(alpha: 0.5);
        fg = Colors.white;
        border = BorderSide.none;
      case CcButtonVariant.normal:
        bg = AppColors.surface;
    }

    return Material(
      color: bg,
      borderRadius: BorderRadius.circular(6),
      child: InkWell(
        onTap: onPressed,
        borderRadius: BorderRadius.circular(6),
        child: Container(
          padding: EdgeInsets.symmetric(horizontal: dense ? 8 : 10, vertical: dense ? 6 : 8),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(6),
            border: Border.fromBorderSide(border),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                CcIcon(icon!, size: 14, color: fg),
                const SizedBox(width: 6),
              ],
              Text(label, style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: fg)),
            ],
          ),
        ),
      ),
    );
  }
}

class StatusBadge extends StatelessWidget {
  const StatusBadge(this.status, {super.key});

  final String? status;

  @override
  Widget build(BuildContext context) {
    final value = (status ?? '').toLowerCase();
    Color fg = AppColors.textMuted;
    Color bg = AppColors.surfaceMuted;

    if (['succeeded', 'approved', 'open', 'live', 'running', 'resolved'].any(value.contains)) {
      fg = AppColors.successFg;
      bg = AppColors.successBg;
    } else if (['failed', 'rejected', 'closed', 'offline', 'error', 'canceled', 'cancelled'].any(value.contains)) {
      fg = AppColors.dangerFg;
      bg = AppColors.dangerBg;
    } else if (['waiting', 'pending', 'paused', 'stale', 'warning'].any(value.contains)) {
      fg = AppColors.warningFg;
      bg = AppColors.warningBg;
    } else if (['debug_paused', 'info'].any(value.contains)) {
      fg = AppColors.infoFg;
      bg = AppColors.infoBg;
    }

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(color: bg, borderRadius: BorderRadius.circular(999)),
      child: Text(status ?? '—', style: TextStyle(fontSize: 11, fontWeight: FontWeight.w600, color: fg)),
    );
  }
}

class EmptyState extends StatelessWidget {
  const EmptyState({super.key, required this.message, this.icon = IconName.box});

  final String message;
  final IconName icon;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            CcIcon(icon, size: 28, color: AppColors.textMuted),
            const SizedBox(height: 8),
            Text(message, style: const TextStyle(color: AppColors.textMuted, fontSize: 13), textAlign: TextAlign.center),
          ],
        ),
      ),
    );
  }
}

class PanelToolbar extends StatelessWidget {
  const PanelToolbar({super.key, required this.title, this.actions = const []});

  final String title;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 8),
      child: Row(
        children: [
          Text(title, style: Theme.of(context).textTheme.titleMedium),
          const Spacer(),
          Wrap(spacing: 8, runSpacing: 8, children: actions),
        ],
      ),
    );
  }
}

class PanelCard extends StatelessWidget {
  const PanelCard({super.key, required this.child, this.padding = const EdgeInsets.all(12)});

  final Widget child;
  final EdgeInsets padding;

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: EdgeInsets.zero,
      child: Padding(padding: padding, child: child),
    );
  }
}

Future<T?> showCcDialog<T>({
  required BuildContext context,
  required String title,
  required Widget body,
  List<Widget>? actions,
}) {
  return showDialog<T>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(title),
      content: SizedBox(width: 480, child: body),
      actions: actions ??
          [
            TextButton(onPressed: () => Navigator.pop(context), child: const Text('Close')),
          ],
    ),
  );
}
