import 'dart:async';

import 'package:flutter/foundation.dart';

import 'core/models.dart';
import 'core/nexus_core.dart';

class AppController extends ChangeNotifier {
  AppController(this.core);

  final NexusCore core;
  List<Peer> peers = const [];
  String? localId;
  String? localName;
  String? error;
  bool busy = false;
  Timer? _timer;
  StreamSubscription<Map<String, dynamic>>? _transportSubscription;
  final Map<String, int> _rssiByPrefix = {};
  bool _refreshing = false;

  Future<void> initialize() async {
    final identity = await core.identity();
    localId = identity['device_id'] as String;
    localName = identity['display_name'] as String;
    _transportSubscription = core.transportEvents.listen((event) {
      final prefix = event['deviceIdPrefix'] as String?;
      final rssi = event['rssi'] as int?;
      if (prefix != null && rssi != null) {
        _rssiByPrefix[prefix] = rssi;
        notifyListeners();
      }
    });
    await refresh();
    _timer =
        Timer.periodic(const Duration(milliseconds: 400), (_) => refresh());
  }

  Future<void> refresh() async {
    if (_refreshing) return;
    _refreshing = true;
    try {
      peers = await core.peers();
      await core.events();
      error = null;
      notifyListeners();
    } catch (exception) {
      error = exception.toString();
      notifyListeners();
    } finally {
      _refreshing = false;
    }
  }

  Future<void> rename(String value) async {
    final identity = await core.setDisplayName(value);
    localName = identity['display_name'] as String;
    notifyListeners();
  }

  String proximityFor(Peer peer) {
    final rssi = _rssiByPrefix[peer.id.substring(0, 8)];
    if (rssi == null) return peer.online ? '可连接' : '离线';
    if (rssi >= -55) return '很近';
    if (rssi >= -72) return '附近';
    return '较远';
  }

  Future<void> pair(Peer peer) => _run(() => core.pair(peer.id));
  Future<void> sync(Peer peer) => _run(() => core.sync(peer.id));

  Future<void> _run(Future<void> Function() action) async {
    busy = true;
    notifyListeners();
    try {
      await action();
      await refresh();
    } catch (exception) {
      error = exception.toString();
    } finally {
      busy = false;
      notifyListeners();
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    _transportSubscription?.cancel();
    core.close();
    super.dispose();
  }
}
