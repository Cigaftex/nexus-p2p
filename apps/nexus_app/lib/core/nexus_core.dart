import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:async';

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'models.dart';

typedef _CreateNative = Pointer<Void> Function(Pointer<Utf8>);
typedef _CreateDart = Pointer<Void> Function(Pointer<Utf8>);
typedef _CallNative = Pointer<Utf8> Function(Pointer<Void>, Pointer<Utf8>);
typedef _CallDart = Pointer<Utf8> Function(Pointer<Void>, Pointer<Utf8>);
typedef _FreeNative = Void Function(Pointer<Utf8>);
typedef _FreeDart = void Function(Pointer<Utf8>);
typedef _DestroyNative = Void Function(Pointer<Void>);
typedef _DestroyDart = void Function(Pointer<Void>);

class NexusCore {
  NexusCore._(this._library, this._handle) {
    _callNative = _library.lookupFunction<_CallNative, _CallDart>('nexus_call');
    _freeNative =
        _library.lookupFunction<_FreeNative, _FreeDart>('nexus_string_free');
    _destroyNative =
        _library.lookupFunction<_DestroyNative, _DestroyDart>('nexus_destroy');
  }

  final DynamicLibrary _library;
  final Pointer<Void> _handle;
  late final _CallDart _callNative;
  late final _FreeDart _freeNative;
  late final _DestroyDart _destroyNative;
  _AppleBonjourBridge? _bonjour;
  final _transportEvents = StreamController<Map<String, dynamic>>.broadcast();

  Stream<Map<String, dynamic>> get transportEvents => _transportEvents.stream;

  static Future<NexusCore> open(
      {String displayName = 'Nexus device', int port = 47777}) async {
    final library = _openLibrary();
    final create =
        library.lookupFunction<_CreateNative, _CreateDart>('nexus_create');
    final root = await getApplicationSupportDirectory();
    final config = jsonEncode({
      'data_dir': '${root.path}${Platform.pathSeparator}nexus',
      'display_name': displayName,
      'listen_port': port,
      // Apple requires Bonjour discovery to go through its native APIs.
      'enable_mdns': !(Platform.isIOS || Platform.isMacOS),
    }).toNativeUtf8();
    try {
      final handle = create(config);
      if (handle == nullptr) {
        throw StateError('Unable to initialize Nexus Core');
      }
      final core = NexusCore._(library, handle);
      await core.call({'op': 'start'});
      if (Platform.isIOS || Platform.isMacOS) {
        core._bonjour = _AppleBonjourBridge(core);
        await core._bonjour!.start(await core.identity(), port);
      }
      return core;
    } finally {
      malloc.free(config);
    }
  }

  Future<dynamic> call(Map<String, dynamic> command) async {
    final input = jsonEncode(command).toNativeUtf8();
    try {
      final pointer = _callNative(_handle, input);
      if (pointer == nullptr) {
        throw StateError('Nexus Core returned no response');
      }
      try {
        final response =
            jsonDecode(pointer.toDartString()) as Map<String, dynamic>;
        if (response['ok'] != true) {
          throw NexusException(
              response['error'] as String? ?? 'Unknown core error');
        }
        return response['data'];
      } finally {
        _freeNative(pointer);
      }
    } finally {
      malloc.free(input);
    }
  }

  Future<Map<String, dynamic>> identity() async =>
      (await call({'op': 'identity'}) as Map<String, dynamic>);

  Future<String> identityId() async =>
      (await identity())['device_id'] as String;

  Future<Map<String, dynamic>> setDisplayName(String displayName) async {
    final value = await call({
      'op': 'set_display_name',
      'display_name': displayName,
    }) as Map<String, dynamic>;
    await _bonjour?.restart(value);
    return value;
  }

  Future<List<Peer>> peers() async =>
      (await call({'op': 'peers'}) as List<dynamic>)
          .map((item) => Peer.fromJson(item as Map<String, dynamic>))
          .toList();

  Future<void> pair(String peerId) => call({'op': 'pair', 'peer_id': peerId});
  Future<void> rememberPeer(
          Map<String, dynamic> identity, String host, int port) =>
      call({
        'op': 'remember_peer',
        'identity': identity,
        'host': host,
        'port': port,
      });
  Future<void> sendText(String peerId, String text) =>
      call({'op': 'send_text', 'peer_id': peerId, 'text': text});
  Future<void> sendFile(String peerId, String path) => call({
        'op': 'send_file',
        'peer_id': peerId,
        'path': path,
        'media_type': 'application/octet-stream'
      });
  Future<void> sync(String peerId) => call({'op': 'sync', 'peer_id': peerId});

  Future<List<ChatEntry>> chat(String peerId) async =>
      (await call({'op': 'chat', 'peer_id': peerId}) as List<dynamic>)
          .map((item) => ChatEntry.fromJson(item as Map<String, dynamic>))
          .toList();

  Future<List<Map<String, dynamic>>> events() async =>
      (await call({'op': 'events'}) as List<dynamic>)
          .cast<Map<String, dynamic>>();

  void close() {
    _bonjour?.stop();
    _transportEvents.close();
    _destroyNative(_handle);
  }

  static DynamicLibrary _openLibrary() {
    if (Platform.isIOS) {
      return DynamicLibrary.process();
    }
    if (Platform.isMacOS) {
      return _openFirst([
        '${Directory.current.path}${Platform.pathSeparator}libnexus_core.dylib',
        '${File(Platform.resolvedExecutable).parent.path}${Platform.pathSeparator}..${Platform.pathSeparator}Frameworks${Platform.pathSeparator}libnexus_core.dylib',
        'libnexus_core.dylib',
      ]);
    }
    if (Platform.isWindows) {
      return _openFirst([
        '${Directory.current.path}${Platform.pathSeparator}nexus_core.dll',
        '${File(Platform.resolvedExecutable).parent.path}${Platform.pathSeparator}nexus_core.dll',
        'nexus_core.dll',
      ]);
    }
    if (Platform.isAndroid || Platform.isLinux) {
      return DynamicLibrary.open('libnexus_core.so');
    }
    throw UnsupportedError('Unsupported platform ${Platform.operatingSystem}');
  }

  static DynamicLibrary _openFirst(List<String> candidates) {
    Object? lastError;
    for (final path in candidates) {
      try {
        return DynamicLibrary.open(path);
      } catch (error) {
        lastError = error;
      }
    }
    throw StateError('Unable to load Nexus Core: $lastError');
  }
}

class _AppleBonjourBridge {
  _AppleBonjourBridge(this.core);

  static const _channel = MethodChannel('app.nexus/transport');
  final NexusCore core;
  int _port = 47777;

  Future<void> start(Map<String, dynamic> identity, int port) async {
    _port = port;
    _channel.setMethodCallHandler((call) async {
      if (call.method == 'bleProximity') {
        core._transportEvents
            .add(Map<String, dynamic>.from(call.arguments as Map));
        return;
      }
      if (call.method != 'bonjourPeer') return;
      final arguments = Map<String, dynamic>.from(call.arguments as Map);
      final peer = Map<String, dynamic>.from(arguments['identity'] as Map);
      peer['signing_public_key'] =
          _decodeHex(peer['signing_public_key'] as String);
      peer['exchange_public_key'] =
          _decodeHex(peer['exchange_public_key'] as String);
      await core.rememberPeer(
          peer, arguments['host'] as String, arguments['port'] as int);
    });

    await _channel.invokeMethod<void>('startBonjour', {
      'deviceId': identity['device_id'],
      'displayName': identity['display_name'],
      'signingPublicKey':
          _encodeHex((identity['signing_public_key'] as List).cast<int>()),
      'exchangePublicKey':
          _encodeHex((identity['exchange_public_key'] as List).cast<int>()),
      'version': 1,
      'port': port,
    });
  }

  Future<void> restart(Map<String, dynamic> identity) => start(identity, _port);

  Future<void> stop() async {
    _channel.setMethodCallHandler(null);
    await _channel.invokeMethod<void>('stopBonjour');
  }

  static String _encodeHex(List<int> bytes) =>
      bytes.map((byte) => byte.toRadixString(16).padLeft(2, '0')).join();

  static List<int> _decodeHex(String value) {
    if (value.length.isOdd) throw const FormatException('Invalid peer key');
    return [
      for (var offset = 0; offset < value.length; offset += 2)
        int.parse(value.substring(offset, offset + 2), radix: 16)
    ];
  }
}

class NexusException implements Exception {
  const NexusException(this.message);
  final String message;
  @override
  String toString() => message;
}
