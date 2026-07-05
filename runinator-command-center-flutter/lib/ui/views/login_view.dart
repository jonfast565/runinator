import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/services/auth_service.dart';
import '../theme/app_theme.dart';

class LoginView extends ConsumerStatefulWidget {
  const LoginView({super.key});

  @override
  ConsumerState<LoginView> createState() => _LoginViewState();
}

class _LoginViewState extends ConsumerState<LoginView> {
  final _username = TextEditingController();
  final _password = TextEditingController();
  var _submitting = false;

  @override
  void initState() {
    super.initState();
    _username.addListener(_handleFieldsChanged);
    _password.addListener(_handleFieldsChanged);
  }

  void _handleFieldsChanged() => setState(() {});

  @override
  void dispose() {
    _username.removeListener(_handleFieldsChanged);
    _password.removeListener(_handleFieldsChanged);
    _username.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    setState(() => _submitting = true);
    try {
      await ref.read(authProvider.notifier).signIn(_username.text, _password.text);
    } finally {
      if (mounted) setState(() => _submitting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final auth = ref.watch(authProvider);
    final canSubmit = !_submitting && _username.text.isNotEmpty && _password.text.isNotEmpty;

    return Scaffold(
      backgroundColor: AppColors.surfaceApp,
      body: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(vertical: 40, horizontal: 24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 380),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  width: 56,
                  height: 56,
                  decoration: BoxDecoration(
                    gradient: LinearGradient(
                      colors: [AppColors.accent, AppColors.accentPulse],
                      begin: Alignment.topLeft,
                      end: Alignment.bottomRight,
                    ),
                    borderRadius: BorderRadius.circular(AppMetrics.radiusLg),
                    boxShadow: [BoxShadow(color: AppColors.accent.withValues(alpha: 0.28), blurRadius: 28, offset: const Offset(0, 12))],
                  ),
                  alignment: Alignment.center,
                  child: const Text('R', style: TextStyle(color: Colors.white, fontWeight: FontWeight.w800, fontSize: 26)),
                ),
                const SizedBox(height: 20),
                Text('Runinator', style: Theme.of(context).textTheme.titleLarge),
                const SizedBox(height: 4),
                Text('Sign in to your command center', style: TextStyle(color: AppColors.textMuted, fontSize: 14)),
                const SizedBox(height: 28),
                Card(
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        TextField(
                          controller: _username,
                          textInputAction: TextInputAction.next,
                          decoration: const InputDecoration(labelText: 'Username'),
                        ),
                        const SizedBox(height: 14),
                        TextField(
                          controller: _password,
                          decoration: const InputDecoration(labelText: 'Password'),
                          obscureText: true,
                          onSubmitted: (_) => canSubmit ? _submit() : null,
                        ),
                        if (auth.error.isNotEmpty) ...[
                          const SizedBox(height: 14),
                          Container(
                            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                            decoration: BoxDecoration(color: AppColors.dangerBg, borderRadius: BorderRadius.circular(AppMetrics.radiusSm)),
                            child: Text(auth.error, style: TextStyle(color: AppColors.dangerFg, fontSize: 12.5)),
                          ),
                        ],
                        const SizedBox(height: 22),
                        SizedBox(
                          height: 46,
                          child: FilledButton(
                            style: FilledButton.styleFrom(
                              backgroundColor: AppColors.accent,
                              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(AppMetrics.radiusSm)),
                            ),
                            onPressed: canSubmit ? _submit : null,
                            child: _submitting
                                ? const SizedBox(
                                    width: 18,
                                    height: 18,
                                    child: CircularProgressIndicator(strokeWidth: 2, color: Colors.white),
                                  )
                                : const Text('Sign in', style: TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600)),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
