import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';

import '../app_controller.dart';
import '../core/models.dart';

class ChatScreen extends StatefulWidget {
  const ChatScreen({super.key, required this.controller, required this.peer});
  final AppController controller;
  final Peer peer;

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> {
  final _input = TextEditingController();
  List<ChatEntry> _items = const [];
  bool _sending = false;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _load();
    _timer = Timer.periodic(const Duration(milliseconds: 350), (_) => _load());
  }

  @override
  void dispose() {
    _timer?.cancel();
    _input.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final values = await widget.controller.core.chat(widget.peer.id);
    if (mounted) setState(() => _items = values);
  }

  Future<void> _send() async {
    final text = _input.text.trim();
    if (text.isEmpty) return;
    setState(() => _sending = true);
    try {
      await widget.controller.core.sendText(widget.peer.id, text);
      _input.clear();
      await _load();
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<void> _file() async {
    final picked = await FilePicker.platform.pickFiles();
    final path = picked?.files.single.path;
    if (path == null) return;
    setState(() => _sending = true);
    try {
      await widget.controller.core.sendFile(widget.peer.id, path);
      await _load();
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final localId = widget.controller.localId;
    return Scaffold(
      appBar: AppBar(title: Text(widget.peer.name), actions: [
        IconButton(
            onPressed: () async {
              await widget.controller.sync(widget.peer);
              await _load();
            },
            icon: const Icon(Icons.sync))
      ]),
      body: Column(children: [
        Expanded(
            child: RefreshIndicator(
                onRefresh: _load,
                child: ListView.builder(
                  padding: const EdgeInsets.all(16),
                  itemCount: _items.length,
                  itemBuilder: (context, index) {
                    final item = _items[index];
                    final mine = item.author == localId;
                    return Align(
                      alignment:
                          mine ? Alignment.centerRight : Alignment.centerLeft,
                      child: Card(
                          color: mine
                              ? Theme.of(context).colorScheme.primaryContainer
                              : null,
                          child: Padding(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 14, vertical: 10),
                            child:
                                Row(mainAxisSize: MainAxisSize.min, children: [
                              if (item.type == 'file_manifest')
                                const Padding(
                                    padding: EdgeInsets.only(right: 8),
                                    child:
                                        Icon(Icons.insert_drive_file_outlined)),
                              Flexible(child: Text(item.text))
                            ]),
                          )),
                    );
                  },
                ))),
        SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(8, 6, 8, 8),
              child: Row(children: [
                IconButton(
                    onPressed: _sending ? null : _file,
                    icon: const Icon(Icons.attach_file)),
                Expanded(
                    child: TextField(
                        controller: _input,
                        onSubmitted: (_) => _send(),
                        decoration: const InputDecoration(
                            hintText: '加密消息', border: OutlineInputBorder()))),
                IconButton(
                    onPressed: _sending ? null : _send,
                    icon: const Icon(Icons.send)),
              ]),
            )),
      ]),
    );
  }
}
