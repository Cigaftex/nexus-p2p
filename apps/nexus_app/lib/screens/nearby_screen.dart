import 'package:flutter/material.dart';

import '../app_controller.dart';
import '../core/models.dart';
import 'chat_screen.dart';

class NearbyScreen extends StatelessWidget {
  const NearbyScreen({super.key, required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) => Scaffold(
        appBar: AppBar(title: const Text('附近设备'), actions: [
          IconButton(
              onPressed: () => _rename(context),
              icon: const Icon(Icons.edit_outlined),
              tooltip: '修改设备名'),
          IconButton(
              onPressed: controller.refresh, icon: const Icon(Icons.refresh))
        ]),
        body: Column(children: [
          if (controller.error != null)
            MaterialBanner(content: Text(controller.error!), actions: [
              TextButton(onPressed: controller.refresh, child: const Text('重试'))
            ]),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
            child: Row(children: [
              const Icon(Icons.security, size: 18),
              const SizedBox(width: 8),
              Expanded(
                  child: Text(
                      '${controller.localName ?? 'Nexus device'} · ${_short(controller.localId)} · 端到端加密',
                      style: Theme.of(context).textTheme.bodySmall)),
            ]),
          ),
          Expanded(
              child: controller.peers.isEmpty
                  ? const _EmptyState()
                  : ListView.separated(
                      itemCount: controller.peers.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, index) => _PeerTile(
                          peer: controller.peers[index],
                          controller: controller),
                    )),
        ]),
      ),
    );
  }

  Future<void> _rename(BuildContext context) async {
    final input = TextEditingController(text: controller.localName);
    final value = await showDialog<String>(
        context: context,
        builder: (context) => AlertDialog(
              title: const Text('设备名称'),
              content: TextField(
                  controller: input,
                  autofocus: true,
                  maxLength: 40,
                  decoration: const InputDecoration(hintText: '例如：小明的 iPhone')),
              actions: [
                TextButton(
                    onPressed: () => Navigator.pop(context),
                    child: const Text('取消')),
                FilledButton(
                    onPressed: () => Navigator.pop(context, input.text.trim()),
                    child: const Text('保存'))
              ],
            ));
    input.dispose();
    if (value != null && value.isNotEmpty && context.mounted) {
      await controller.rename(value);
    }
  }
}

class _PeerTile extends StatelessWidget {
  const _PeerTile({required this.peer, required this.controller});
  final Peer peer;
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      leading:
          CircleAvatar(child: Text(peer.name.characters.first.toUpperCase())),
      title: Text(peer.name),
      subtitle: Text('${_short(peer.id)} · ${controller.proximityFor(peer)}'),
      trailing: peer.paired
          ? const Icon(Icons.chevron_right)
          : FilledButton.tonal(
              onPressed: controller.busy || !peer.online
                  ? null
                  : () => controller.pair(peer),
              child: const Text('配对')),
      onTap: peer.paired
          ? () => Navigator.of(context).push(MaterialPageRoute(
              builder: (_) => ChatScreen(controller: controller, peer: peer)))
          : null,
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();
  @override
  Widget build(BuildContext context) => const Center(
          child: Padding(
        padding: EdgeInsets.all(32),
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          Icon(Icons.radar, size: 52),
          SizedBox(height: 16),
          Text('正在寻找附近设备'),
          SizedBox(height: 6),
          Text('保持 Wi-Fi 或蓝牙开启；无需连接同一个路由器。', textAlign: TextAlign.center)
        ]),
      ));
}

String _short(String? id) => id == null ? '…' : '${id.substring(0, 8)}…';
