// port of core/utils/resources.ts.

import '../domain/json.dart';
import '../workflow/workflow_helpers.dart' show asRecord;
import 'values.dart';

String genericRecordType(JsonRecord record, String endpoint) {
  final explicit = record['resource_type'] ?? record['approval_type'] ?? record['event_type'];

  if (explicit != null) {
    return displayValue(explicit);
  }

  if (endpoint == 'external_items') {
    return 'external_item';
  }

  if (endpoint == 'automation_events') {
    return 'event';
  }

  return endpoint.replaceFirst(RegExp(r's$'), '');
}

String genericRecordSummary(JsonRecord record) {
  if (record['provider'] == 'jira') {
    final key = record['external_id'] ?? record['key'] ?? '';
    final title = record['title'] ?? record['summary'] ?? '';
    return '${displayValue(key)} ${displayValue(title)}'.trim();
  }

  if (record['provider'] == 'github') {
    final title = record['title'] ?? record['name'] ?? '';
    final url = record['url'] ?? record['html_url'] ?? '';
    return '${displayValue(title)} ${displayValue(url)}'.trim();
  }

  final metadata = asRecord(record['metadata']);
  return displayValue(
    record['title'] ?? record['prompt'] ?? record['message'] ?? record['name'] ?? metadata['summary'] ?? metadata['url'],
  );
}
