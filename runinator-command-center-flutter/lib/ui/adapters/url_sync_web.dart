import 'dart:async';

// ignore: avoid_web_libraries_in_flutter, deprecated_member_use
import 'dart:html' as html;

/// web hash navigation binding for deep links (#/Tab/id).
class UrlSyncBinding {
  UrlSyncBinding._() {
    _controller = StreamController<void>.broadcast(
      onListen: () => html.window.onPopState.listen((_) => _controller.add(null)),
    );
  }

  static final UrlSyncBinding instance = UrlSyncBinding._();

  late final StreamController<void> _controller;

  String get hash => html.window.location.hash;

  set hash(String value) {
    if (html.window.location.hash != value) {
      html.window.history.pushState(null, '', value);
    }
  }

  Stream<void> get onPopState => _controller.stream;
}
