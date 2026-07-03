import 'package:flutter/material.dart';

import '../../core/services/display_preferences_service.dart';

class AppColors {
  const AppColors._();

  static const surfaceApp = Color(0xFFE9EEF4);
  static const surface = Color(0xFFFFFFFF);
  static const surfaceSubtle = Color(0xFFF8FAFC);
  static const surfaceMuted = Color(0xFFEEF2F6);
  static const surfaceInverse = Color(0xFF15202B);
  static const surfaceInverseHover = Color(0xFF223142);
  static const textPrimary = Color(0xFF17202A);
  static const textSubtle = Color(0xFF4B5663);
  static const textMuted = Color(0xFF66717E);
  static const textInverse = Color(0xFFF7FAFC);
  static const textInverseMuted = Color(0xFFD8E0E8);
  static const textInverseFaint = Color(0xFF778392);
  static const border = Color(0xFFD4DDE7);
  static const borderStrong = Color(0xFFC4CCD6);
  static const accent = Color(0xFF1F6FEB);
  static const accentHover = Color(0xFF1864D6);
  static const accentSoft = Color(0xFFEEF5FF);
  static const dangerSolid = Color(0xFFD6402B);
  static const successFg = Color(0xFF1F6F49);
  static const successBg = Color(0xFFDFF5E7);
  static const dangerFg = Color(0xFFA33A2F);
  static const dangerBg = Color(0xFFFDE2DF);
  static const warningFg = Color(0xFF84620D);
  static const warningBg = Color(0xFFFFF2CC);
  static const infoFg = Color(0xFF1D5B9F);
  static const infoBg = Color(0xFFE1EFFF);
  static const workflowCanvasBg = Color(0xFFF8FAFC);
  static const workflowNodeBorder = Color(0xFF34495E);
}

ThemeData buildAppTheme({required Brightness brightness}) {
  final isDark = brightness == Brightness.dark;
  final scheme = ColorScheme.fromSeed(
    seedColor: AppColors.accent,
    brightness: brightness,
    surface: isDark ? const Color(0xFF1A2332) : AppColors.surface,
  );

  return ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: isDark ? const Color(0xFF121820) : AppColors.surfaceApp,
    dividerColor: AppColors.border,
    fontFamily: 'system-ui',
    textTheme: const TextTheme(
      bodyMedium: TextStyle(fontSize: 13, color: AppColors.textPrimary),
      bodySmall: TextStyle(fontSize: 12, color: AppColors.textMuted),
      titleMedium: TextStyle(fontSize: 15, fontWeight: FontWeight.w700, color: AppColors.textPrimary),
      titleSmall: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: AppColors.textPrimary),
    ),
    inputDecorationTheme: InputDecorationTheme(
      isDense: true,
      contentPadding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(6), borderSide: const BorderSide(color: AppColors.borderStrong)),
      enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(6), borderSide: const BorderSide(color: AppColors.borderStrong)),
      focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(6), borderSide: const BorderSide(color: AppColors.accent, width: 1.5)),
      filled: true,
      fillColor: isDark ? const Color(0xFF1E2836) : AppColors.surface,
    ),
    cardTheme: CardThemeData(
      color: isDark ? const Color(0xFF1A2332) : AppColors.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8), side: const BorderSide(color: AppColors.border)),
    ),
    popupMenuTheme: PopupMenuThemeData(
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
    ),
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
