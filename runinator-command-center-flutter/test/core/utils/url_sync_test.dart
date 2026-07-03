import 'package:runinator_command_center_flutter/core/utils/url_sync.dart';
import 'package:test/test.dart';

bool known(String tab) => ['Workflows', 'Runs', 'Providers'].contains(tab);

void main() {
  group('url-sync route mapping', () {
    test('parses an empty hash to no route', () {
      expect(parseRoute('', known).tab, isNull);
      expect(parseRoute('', known).id, isNull);
      expect(parseRoute('#', known).tab, isNull);
      expect(parseRoute('#', known).id, isNull);
    });

    test('parses a tab-only hash', () {
      expect(parseRoute('#/Runs', known).tab, 'Runs');
      expect(parseRoute('#/Runs', known).id, isNull);
      expect(parseRoute('#Runs', known).tab, 'Runs');
      expect(parseRoute('#Runs', known).id, isNull);
    });

    test('parses a tab + id hash and decodes the id', () {
      expect(parseRoute('#/Workflows/abc-123', known).tab, 'Workflows');
      expect(parseRoute('#/Workflows/abc-123', known).id, 'abc-123');
      expect(parseRoute('#/Runs/a%2Fb', known).tab, 'Runs');
      expect(parseRoute('#/Runs/a%2Fb', known).id, 'a/b');
    });

    test('rejects unknown tabs', () {
      expect(parseRoute('#/Nope/1', known).tab, isNull);
      expect(parseRoute('#/Nope/1', known).id, '1');
    });

    test('formats routes with and without an id', () {
      expect(formatRoute('Runs', null), '#/Runs');
      expect(formatRoute('Workflows', 'abc-123'), '#/Workflows/abc-123');
      expect(formatRoute('Runs', 'a/b'), '#/Runs/a%2Fb');
    });

    test('round-trips a tab + id', () {
      final hash = formatRoute('Workflows', 'wf 1');
      final route = parseRoute(hash, known);
      expect(route.tab, 'Workflows');
      expect(route.id, 'wf 1');
    });
  });
}
