import 'package:runinator_command_center_flutter/core/utils/wdl_expression.dart';
import 'package:test/test.dart';

void main() {
  group('WDL expression conversion', () {
    test('renders lowered references and operators as WDL surface expressions', () {
      expect(expressionJsonToWdl({r'$ref': {'params': ['ticket_id']}}), 'params.ticket_id');
      expect(expressionJsonToWdl({r'$ref': {'input': ['ticket_id']}}), '{ input: ["ticket_id"] }');
      expect(expressionJsonToWdl({r'$ref': {'workflow': ['attempt']}}), 'run.attempt');
      expect(
        expressionJsonToWdl({r'$ref': {'node': 'create_ticket', 'output': ['id']}}),
        'create_ticket.id',
      );
      expect(
        expressionJsonToWdl({r'$concat': ['ticket ', {r'$ref': {'params': ['ticket_id']}}]}),
        '"ticket " ++ params.ticket_id',
      );
      expect(
        expressionJsonToWdl({r'$coalesce': [{r'$ref': {'prev': ['name']}}, 'unknown']}),
        'prev.name ?? "unknown"',
      );
      expect(
        expressionJsonToWdl({r'$to_string': {r'$ref': {'prev': ['count']}}}),
        'string(prev.count)',
      );
    });

    test('parses WDL surface expressions back into lowered JSON values', () {
      expect(parseWdlExpression('params.ticket_id'), {r'$ref': {'params': ['ticket_id']}});
      expect(parseWdlExpression('"ticket " ++ params.ticket_id'), {
        r'$concat': ['ticket ', {r'$ref': {'params': ['ticket_id']}}],
      });
      expect(parseWdlExpression('input.ticket_id'), {
        r'$ref': {'node': 'input', 'output': ['ticket_id']},
      });
      expect(parseWdlExpression('prev.name ?? "unknown"'), {
        r'$coalesce': [{r'$ref': {'prev': ['name']}}, 'unknown'],
      });
      expect(parseWdlExpression('string(prev.count)'), {
        r'$to_string': {r'$ref': {'prev': ['count']}},
      });
      expect(parseWdlExpression('{ message: string(prev.count), tags: [params.tag] }'), {
        'message': {r'$to_string': {r'$ref': {'prev': ['count']}}},
        'tags': [{r'$ref': {'params': ['tag']}}],
      });
    });
  });
}
