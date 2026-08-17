class Peer {
  const Peer(
      {required this.id,
      required this.name,
      required this.paired,
      required this.online});

  final String id;
  final String name;
  final bool paired;
  final bool online;

  factory Peer.fromJson(Map<String, dynamic> json) {
    final identity = json['identity'] as Map<String, dynamic>;
    return Peer(
      id: identity['device_id'] as String,
      name: identity['display_name'] as String,
      paired: json['paired'] as bool? ?? false,
      online: json['endpoint'] != null,
    );
  }
}

class ChatEntry {
  const ChatEntry(
      {required this.eventId,
      required this.author,
      required this.sentAt,
      required this.type,
      required this.text,
      this.manifestId});

  final String eventId;
  final String author;
  final DateTime sentAt;
  final String type;
  final String text;
  final String? manifestId;

  factory ChatEntry.fromJson(Map<String, dynamic> json) {
    final payload = json['payload'] as Map<String, dynamic>;
    final type = payload['type'] as String;
    final data = payload['data'];
    String text;
    String? manifestId;
    if (type == 'text') {
      text = (data as Map<String, dynamic>)['text'] as String;
    } else if (type == 'file_manifest') {
      final manifest = data as Map<String, dynamic>;
      text = '${manifest['name']} · ${_formatBytes(manifest['size'] as int)}';
      manifestId = manifest['id'] as String;
    } else {
      text = data.toString();
    }
    return ChatEntry(
      eventId: json['event_id'] as String,
      author: json['author'] as String,
      sentAt: DateTime.fromMillisecondsSinceEpoch(json['created_at_ms'] as int),
      type: type,
      text: text,
      manifestId: manifestId,
    );
  }
}

String _formatBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
  return '${(bytes / 1024 / 1024).toStringAsFixed(1)} MB';
}
