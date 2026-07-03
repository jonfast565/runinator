/// no-op url sync for non-web targets.
class UrlSyncBinding {
  UrlSyncBinding._();

  static final UrlSyncBinding instance = UrlSyncBinding._();

  String get hash => '';

  set hash(String value) {}

  Stream<void> get onPopState => const Stream.empty();
}
