import 'package:flutter_test/flutter_test.dart';
import 'package:nexus_app/core/models.dart';

void main() {
  test('parses encrypted-core text projection', () {
    final item = ChatEntry.fromJson({
      'event_id': '01', 'author': 'device', 'created_at_ms': 1000,
      'payload': {'type': 'text', 'data': {'text': 'hello'}},
    });
    expect(item.text, 'hello');
    expect(item.type, 'text');
  });
}
