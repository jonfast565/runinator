import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/navigation/app_tab.dart';
import '../../core/navigation/breakpoints.dart';
import '../../core/navigation/nav_config.dart';
import '../../core/services/app_service.dart';
import '../../core/services/auth_service.dart';
import '../../core/services/orgs_service.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';
import 'connection_strip.dart';
import 'keyboard_shortcuts.dart';
import 'outage_banner.dart';
import 'toast_host.dart';
import 'top_toolbar.dart';

class SidebarNav extends ConsumerWidget {
  const SidebarNav({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final app = ref.watch(appProvider);
    final auth = ref.watch(authProvider);
    final width = MediaQuery.sizeOf(context).width;
    final isDesktop = width > Breakpoints.mobile;
    final isAdmin = _isAdmin(auth);
    final sections = visibleNavSections(canSeeAdmin: isAdmin, isDesktop: isDesktop);
    final collapsed = app.sidebarCollapsed && isDesktop;

    return AnimatedContainer(
      duration: const Duration(milliseconds: 180),
      width: collapsed ? 56 : 220,
      color: AppColors.surfaceInverse,
      child: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 12, 12, 8),
              child: Row(
                children: [
                  if (!collapsed) ...[
                    Container(
                      width: 28,
                      height: 28,
                      alignment: Alignment.center,
                      decoration: BoxDecoration(color: const Color(0xFF4EA1FF), borderRadius: BorderRadius.circular(7)),
                      child: const Text('R', style: TextStyle(color: Colors.white, fontWeight: FontWeight.w800)),
                    ),
                    const SizedBox(width: 10),
                    const Expanded(child: Text('Runinator', style: TextStyle(color: AppColors.textInverse, fontWeight: FontWeight.w700))),
                  ],
                  IconButton(
                    icon: Icon(collapsed ? Icons.chevron_right : Icons.chevron_left, color: AppColors.textInverseMuted, size: 18),
                    onPressed: () => ref.read(appProvider.notifier).toggleSidebar(),
                  ),
                ],
              ),
            ),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                children: [
                  for (final section in sections) ...[
                    if (!collapsed)
                      Padding(
                        padding: const EdgeInsets.fromLTRB(8, 12, 8, 4),
                        child: Text(section.label.toUpperCase(), style: const TextStyle(color: AppColors.textInverseFaint, fontSize: 10, letterSpacing: 0.8)),
                      ),
                    for (final item in section.items)
                      _NavButton(
                        item: item,
                        active: app.activeTab == item.tab,
                        collapsed: collapsed,
                        onTap: () => ref.read(appProvider.notifier).setActiveTab(item.tab),
                      ),
                  ],
                ],
              ),
            ),
            if (!collapsed)
              Padding(
                padding: const EdgeInsets.all(12),
                child: Text(
                  ref.read(orgsProvider.notifier).activeOrg()?.name ?? '',
                  style: const TextStyle(color: AppColors.textInverseMuted, fontSize: 11),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
          ],
        ),
      ),
    );
  }

  bool _isAdmin(AuthState auth) {
    final user = auth.user;
    if (user == null) return true;
    return user['is_admin'] == true;
  }
}

class _NavButton extends StatelessWidget {
  const _NavButton({required this.item, required this.active, required this.collapsed, required this.onTap});

  final NavItem item;
  final bool active;
  final bool collapsed;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: active ? AppColors.surfaceInverseHover : Colors.transparent,
      borderRadius: BorderRadius.circular(6),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(6),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
          child: Row(
            children: [
              CcIcon(item.icon, size: 16, color: active ? AppColors.textInverse : AppColors.textInverseMuted),
              if (!collapsed) ...[
                const SizedBox(width: 10),
                Expanded(child: Text(item.label, style: TextStyle(color: active ? AppColors.textInverse : AppColors.textInverseMuted, fontSize: 13))),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class AppShell extends ConsumerWidget {
  const AppShell({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final app = ref.watch(appProvider);
    final width = MediaQuery.sizeOf(context).width;
    final isMobile = width <= Breakpoints.mobile;

    return CommandCenterKeyboardShortcuts(
      child: Focus(
        autofocus: true,
        onKeyEvent: (_, event) {
          if (event.logicalKey == LogicalKeyboardKey.escape && app.mobileNavOpen) {
            ref.read(appProvider.notifier).closeMobileNav();
          }
          return KeyEventResult.ignored;
        },
        child: Scaffold(
          // a real Drawer gives edge-swipe-to-open, the standard scrim, and back-gesture
          // dismissal for free, instead of hand-rolling an overlay + FAB-as-menu-button.
          drawer: isMobile ? const Drawer(width: 272, child: SidebarNav()) : null,
          onDrawerChanged: (isOpen) {
            final notifier = ref.read(appProvider.notifier);
            isOpen ? notifier.openMobileNav() : notifier.closeMobileNav();
          },
          body: Row(
            children: [
              if (!isMobile) const SidebarNav(),
              Expanded(
                child: Column(
                  children: [
                    const TopToolbar(),
                    const ConnectionStrip(),
                    const OutageBanner(),
                    Expanded(child: child),
                    if (app.loading)
                      LinearProgressIndicator(minHeight: 2, color: AppColors.accent, backgroundColor: AppColors.border),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class AppShellWithToasts extends ConsumerWidget {
  const AppShellWithToasts({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Stack(
      children: [
        AppShell(child: child),
        const ToastHostOverlay(),
      ],
    );
  }
}
