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
    BorderSide border = BorderSide(color: AppColors.borderStrong);

    switch (variant) {
      case CcButtonVariant.primary:
        bg = enabled ? AppColors.accent : AppColors.accent.withValues(alpha: 0.4);
        fg = Colors.white;
        border = BorderSide.none;
      case CcButtonVariant.danger:
        bg = enabled ? AppColors.dangerSolid : AppColors.dangerSolid.withValues(alpha: 0.4);
        fg = Colors.white;
        border = BorderSide.none;
      case CcButtonVariant.normal:
        bg = AppColors.surface;
        fg = enabled ? AppColors.textPrimary : AppColors.textMuted;
    }

    final radius = BorderRadius.circular(AppMetrics.radiusSm);

    return Material(
      color: bg,
      borderRadius: radius,
      child: InkWell(
        onTap: onPressed,
        borderRadius: radius,
        child: Container(
          constraints: BoxConstraints(minHeight: dense ? 34 : 42),
          padding: EdgeInsets.symmetric(horizontal: dense ? 12 : 16, vertical: dense ? 6 : 10),
          decoration: BoxDecoration(borderRadius: radius, border: Border.fromBorderSide(border)),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                CcIcon(icon!, size: 15, color: fg),
                const SizedBox(width: 7),
              ],
              Text(label, style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: fg)),
            ],
          ),
        ),
      ),
    );
  }
}

/// small dot used everywhere status is communicated: solid for a settled
/// state, an animated pulse ring reserved for states that mean "happening
/// right now" (a running node, a live replica, a connected stream) so a
/// glance at the dot — not just the label — tells you whether something is
/// actively in flight.
class StatusDot extends StatelessWidget {
  const StatusDot({super.key, required this.color, this.pulse = false, this.size = 7});

  final Color color;
  final bool pulse;
  final double size;

  @override
  Widget build(BuildContext context) {
    if (!pulse) {
      return Container(
        width: size,
        height: size,
        decoration: BoxDecoration(color: color, shape: BoxShape.circle),
      );
    }
    return _PulsingDot(color: color, size: size);
  }
}

class _PulsingDot extends StatefulWidget {
  const _PulsingDot({required this.color, required this.size});

  final Color color;
  final double size;

  @override
  State<_PulsingDot> createState() => _PulsingDotState();
}

class _PulsingDotState extends State<_PulsingDot> with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(vsync: this, duration: const Duration(milliseconds: 1600))..repeat();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: _controller,
      builder: (context, _) {
        final t = _controller.value;
        final haloSize = widget.size + widget.size * 2.2 * t;
        return SizedBox(
          width: widget.size * 3.2,
          height: widget.size * 3.2,
          child: Stack(
            alignment: Alignment.center,
            children: [
              Opacity(
                opacity: (1 - t) * 0.5,
                child: Container(
                  width: haloSize,
                  height: haloSize,
                  decoration: BoxDecoration(color: widget.color, shape: BoxShape.circle),
                ),
              ),
              Container(
                width: widget.size,
                height: widget.size,
                decoration: BoxDecoration(color: widget.color, shape: BoxShape.circle),
              ),
            ],
          ),
        );
      },
    );
  }
}

class _StatusTone {
  const _StatusTone(this.fg, this.bg, {this.pulse = false});

  final Color fg;
  final Color bg;
  final bool pulse;
}

_StatusTone _toneForStatus(String value) {
  if (['running', 'live', 'connected'].any(value.contains)) {
    return _StatusTone(AppColors.accentPulse, AppColors.accentPulseSoft, pulse: true);
  }
  if (['succeeded', 'approved', 'open', 'resolved'].any(value.contains)) {
    return _StatusTone(AppColors.successFg, AppColors.successBg);
  }
  if (['failed', 'rejected', 'closed', 'offline', 'error', 'canceled', 'cancelled'].any(value.contains)) {
    return _StatusTone(AppColors.dangerFg, AppColors.dangerBg);
  }
  if (['waiting', 'pending', 'paused', 'stale', 'warning', 'connecting'].any(value.contains)) {
    return _StatusTone(AppColors.warningFg, AppColors.warningBg);
  }
  if (['debug_paused', 'info'].any(value.contains)) {
    return _StatusTone(AppColors.infoFg, AppColors.infoBg);
  }
  return _StatusTone(AppColors.textMuted, AppColors.surfaceMuted);
}

class StatusBadge extends StatelessWidget {
  const StatusBadge(this.status, {super.key});

  final String? status;

  @override
  Widget build(BuildContext context) {
    final value = (status ?? '').toLowerCase();
    final tone = _toneForStatus(value);

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 5),
      decoration: BoxDecoration(color: tone.bg, borderRadius: BorderRadius.circular(999)),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          StatusDot(color: tone.fg, pulse: tone.pulse, size: 6),
          const SizedBox(width: 6),
          Text(status ?? '—', style: TextStyle(fontSize: 11.5, fontWeight: FontWeight.w600, color: tone.fg)),
        ],
      ),
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
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              width: 52,
              height: 52,
              decoration: BoxDecoration(color: AppColors.surfaceMuted, shape: BoxShape.circle),
              child: Center(child: CcIcon(icon, size: 22, color: AppColors.textMuted)),
            ),
            const SizedBox(height: 14),
            Text(message, style: TextStyle(color: AppColors.textMuted, fontSize: 13.5), textAlign: TextAlign.center),
          ],
        ),
      ),
    );
  }
}

/// header shown when [SplitPane] swaps to a full-screen detail pane on
/// mobile; gives the user a way back to the list without a system back
/// gesture (which, inside a tab, may not exist).
class MobileBackBar extends StatelessWidget {
  const MobileBackBar({super.key, required this.onBack, this.title});

  final VoidCallback onBack;
  final String? title;

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(color: AppColors.surface, border: Border(bottom: BorderSide(color: AppColors.border))),
      child: Row(
        children: [
          IconButton(icon: const Icon(Icons.arrow_back), tooltip: 'Back', onPressed: onBack),
          if (title != null)
            Expanded(
              child: Text(
                title!,
                style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 14.5),
                overflow: TextOverflow.ellipsis,
                maxLines: 1,
              ),
            ),
        ],
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
      padding: const EdgeInsets.fromLTRB(16, 14, 16, 10),
      child: Row(
        children: [
          Expanded(
            child: Text(
              title,
              style: Theme.of(context).textTheme.titleMedium,
              overflow: TextOverflow.ellipsis,
              maxLines: 1,
            ),
          ),
          if (actions.isNotEmpty) ...[
            const SizedBox(width: 8),
            Flexible(
              child: Wrap(
                spacing: 8,
                runSpacing: 8,
                alignment: WrapAlignment.end,
                children: actions,
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class PanelCard extends StatelessWidget {
  const PanelCard({super.key, required this.child, this.padding = const EdgeInsets.all(16)});

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
