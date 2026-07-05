import 'package:flutter/material.dart';

import '../../core/services/display_preferences_service.dart';

/// a nice, widely-available monospace stack for code/log/id display, in place
/// of the bare 'monospace' generic family (which renders as a fairly ugly
/// fallback on most platforms). ordered by how good each looks; the browser/
/// OS picks the first one it actually has installed.
const String kMonoFontFamily = 'SF Mono';
const List<String> kMonoFontFamilyFallback = [
  'SF Mono',
  'Menlo',
  'Cascadia Code',
  'Consolas',
  'JetBrains Mono',
  'Roboto Mono',
  'Courier New',
  'monospace',
];

/// raw color values for one brightness. kept separate from [AppColors] so
/// [buildAppTheme] can build the light and dark [ThemeData] independent of
/// whichever brightness happens to be currently active — [AppColors]'s
/// getters, by contrast, are meant to track the live brightness.
class _Palette {
  const _Palette({
    required this.surfaceApp,
    required this.surface,
    required this.surfaceSubtle,
    required this.surfaceMuted,
    required this.textPrimary,
    required this.textSubtle,
    required this.textMuted,
    required this.border,
    required this.borderStrong,
    required this.accent,
    required this.accentHover,
    required this.accentSoft,
    required this.accentPulse,
    required this.accentPulseSoft,
    required this.successFg,
    required this.successBg,
    required this.dangerFg,
    required this.dangerBg,
    required this.warningFg,
    required this.warningBg,
    required this.infoFg,
    required this.infoBg,
    required this.workflowCanvasBg,
  });

  final Color surfaceApp;
  final Color surface;
  final Color surfaceSubtle;
  final Color surfaceMuted;
  final Color textPrimary;
  final Color textSubtle;
  final Color textMuted;
  final Color border;
  final Color borderStrong;
  final Color accent;
  final Color accentHover;
  final Color accentSoft;
  final Color accentPulse;
  final Color accentPulseSoft;
  final Color successFg;
  final Color successBg;
  final Color dangerFg;
  final Color dangerBg;
  final Color warningFg;
  final Color warningBg;
  final Color infoFg;
  final Color infoBg;
  final Color workflowCanvasBg;
}

const _lightPalette = _Palette(
  surfaceApp: Color(0xFFF1F3F6),
  surface: Color(0xFFFFFFFF),
  surfaceSubtle: Color(0xFFF7F8FA),
  surfaceMuted: Color(0xFFEBEEF2),
  textPrimary: Color(0xFF181B20),
  textSubtle: Color(0xFF474E56),
  textMuted: Color(0xFF676F79),
  border: Color(0xFFDCE0E6),
  borderStrong: Color(0xFFC5CBD3),
  accent: Color(0xFF0B63C9),
  accentHover: Color(0xFF0A56AE),
  accentSoft: Color(0xFFE6F0FC),
  accentPulse: Color(0xFF11C7B9),
  accentPulseSoft: Color(0xFFDDF8F4),
  successFg: Color(0xFF17875A),
  successBg: Color(0xFFDEF5EA),
  dangerFg: Color(0xFFBE2A3E),
  dangerBg: Color(0xFFFBE2E6),
  warningFg: Color(0xFF8F6200),
  warningBg: Color(0xFFFEF0CE),
  infoFg: Color(0xFF0E76A8),
  infoBg: Color(0xFFE2F2FA),
  workflowCanvasBg: Color(0xFFF7F9FB),
);

const _darkPalette = _Palette(
  surfaceApp: Color(0xFF0B0E12),
  surface: Color(0xFF151B22),
  surfaceSubtle: Color(0xFF1A2129),
  surfaceMuted: Color(0xFF222A33),
  textPrimary: Color(0xFFF5F7FA),
  textSubtle: Color(0xFFD7DEE6),
  textMuted: Color(0xFFB6C0CB),
  border: Color(0xFF2A333D),
  borderStrong: Color(0xFF3A4551),
  accent: Color(0xFF3B82F6),
  accentHover: Color(0xFF2563EB),
  accentSoft: Color(0xFF16233A),
  accentPulse: Color(0xFF2DE0D1),
  accentPulseSoft: Color(0xFF0F2E2C),
  successFg: Color(0xFF34D399),
  successBg: Color(0xFF12301F),
  dangerFg: Color(0xFFF87171),
  dangerBg: Color(0xFF3A1A1F),
  warningFg: Color(0xFFFBBF24),
  warningBg: Color(0xFF352A0D),
  infoFg: Color(0xFF38BDF8),
  infoBg: Color(0xFF102636),
  workflowCanvasBg: Color(0xFF11161C),
);

/// design tokens. field names are the stable contract consumed across every
/// view. most fields track the app's live brightness (see [syncBrightness]);
/// a few are fixed regardless of theme because they belong to chrome that's
/// deliberately always-dark (the sidebar) or a brand solid fill.
class AppColors {
  const AppColors._();

  static bool _isDark = false;
  static _Palette get _p => _isDark ? _darkPalette : _lightPalette;

  /// called once per frame near the widget tree's root (see
  /// `CommandCenterRoot.build`), before any descendant that reads [AppColors]
  /// builds — Flutter builds a tree top-down within a single frame, so this
  /// is safe without threading BuildContext through every call site.
  static void syncBrightness(bool isDark) => _isDark = isDark;

  static Color get surfaceApp => _p.surfaceApp;
  static Color get surface => _p.surface;
  static Color get surfaceSubtle => _p.surfaceSubtle;
  static Color get surfaceMuted => _p.surfaceMuted;
  static Color get textPrimary => _p.textPrimary;
  static Color get textSubtle => _p.textSubtle;
  static Color get textMuted => _p.textMuted;
  static Color get border => _p.border;
  static Color get borderStrong => _p.borderStrong;

  /// interactive accent: buttons, links, focus rings, selection.
  static Color get accent => _p.accent;
  static Color get accentHover => _p.accentHover;
  static Color get accentSoft => _p.accentSoft;

  /// reserved for "this is happening right now": live streams, running runs,
  /// connected replicas. never used for static/interactive chrome, so a cyan
  /// pulse always means "live", not "clickable".
  static Color get accentPulse => _p.accentPulse;
  static Color get accentPulseSoft => _p.accentPulseSoft;

  static Color get successFg => _p.successFg;
  static Color get successBg => _p.successBg;
  static Color get dangerFg => _p.dangerFg;
  static Color get dangerBg => _p.dangerBg;
  static Color get warningFg => _p.warningFg;
  static Color get warningBg => _p.warningBg;
  static Color get infoFg => _p.infoFg;
  static Color get infoBg => _p.infoBg;
  static Color get workflowCanvasBg => _p.workflowCanvasBg;

  // fixed regardless of app brightness: the sidebar nav is deliberately dark-always.
  static const surfaceInverse = Color(0xFF11151B);
  static const surfaceInverseHover = Color(0xFF1D242C);
  static const textInverse = Color(0xFFF5F7FA);
  static const textInverseMuted = Color(0xFFB6C0CB);
  static const textInverseFaint = Color(0xFF6E7885);
  static const workflowNodeBorder = Color(0xFF32404E);
  static const dangerSolid = Color(0xFFCF2B41);
}

/// shared spacing/radius scale so widgets agree on the same rhythm instead of
/// each picking its own magic numbers.
class AppMetrics {
  const AppMetrics._();

  static const radiusSm = 8.0;
  static const radiusMd = 12.0;
  static const radiusLg = 16.0;
}

ThemeData buildAppTheme({required Brightness brightness}) {
  final isDark = brightness == Brightness.dark;
  final p = isDark ? _darkPalette : _lightPalette;
  final scheme = ColorScheme.fromSeed(seedColor: p.accent, brightness: brightness, surface: p.surface);

  return ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: p.surfaceApp,
    dividerColor: p.border,
    splashFactory: InkSparkle.splashFactory,
    textTheme: TextTheme(
      titleLarge: TextStyle(fontSize: 21, fontWeight: FontWeight.w800, letterSpacing: -0.3, color: p.textPrimary),
      titleMedium: TextStyle(fontSize: 17, fontWeight: FontWeight.w700, letterSpacing: -0.1, color: p.textPrimary),
      titleSmall: TextStyle(fontSize: 14, fontWeight: FontWeight.w600, color: p.textPrimary),
      bodyMedium: TextStyle(fontSize: 14, height: 1.35, color: p.textPrimary),
      bodySmall: TextStyle(fontSize: 12.5, height: 1.3, color: p.textMuted),
      labelSmall: TextStyle(fontSize: 11, fontWeight: FontWeight.w600, letterSpacing: 0.4, color: p.textMuted),
    ),
    inputDecorationTheme: InputDecorationTheme(
      isDense: true,
      contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(AppMetrics.radiusSm), borderSide: BorderSide(color: p.borderStrong)),
      enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(AppMetrics.radiusSm), borderSide: BorderSide(color: p.borderStrong)),
      focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(AppMetrics.radiusSm), borderSide: BorderSide(color: p.accent, width: 1.6)),
      filled: true,
      fillColor: isDark ? p.surfaceSubtle : p.surface,
      hintStyle: TextStyle(color: p.textMuted, fontSize: 13.5),
    ),
    cardTheme: CardThemeData(
      color: p.surface,
      elevation: 1,
      shadowColor: Colors.black.withValues(alpha: isDark ? 0.5 : 0.06),
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(AppMetrics.radiusMd), side: BorderSide(color: p.border)),
    ),
    popupMenuTheme: PopupMenuThemeData(
      color: p.surface,
      elevation: 6,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(AppMetrics.radiusMd)),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: p.surface,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(AppMetrics.radiusLg)),
    ),
    iconTheme: IconThemeData(color: p.textMuted),
    dividerTheme: DividerThemeData(color: p.border, space: 1, thickness: 1),
  );
}

ThemeMode themeModeFor(AppTheme theme) {
  switch (theme) {
    case AppTheme.light:
      return ThemeMode.light;
    case AppTheme.dark:
      return ThemeMode.dark;
    case AppTheme.system:
      return ThemeMode.system;
  }
}
