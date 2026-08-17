import 'package:flutter/material.dart';

import 'app_controller.dart';
import 'core/nexus_core.dart';
import 'screens/nearby_screen.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const NexusBootstrapApp());
}

class NexusBootstrapApp extends StatefulWidget {
  const NexusBootstrapApp({super.key});

  @override
  State<NexusBootstrapApp> createState() => _NexusBootstrapAppState();
}

class _NexusBootstrapAppState extends State<NexusBootstrapApp> {
  late Future<AppController> _controller;

  @override
  void initState() {
    super.initState();
    _controller = _initialize();
  }

  Future<AppController> _initialize() async {
    // Paint a real first frame before invoking the synchronous Rust FFI bridge.
    // This also gives iOS a surface on which to present permission prompts.
    await WidgetsBinding.instance.endOfFrame;
    final core = await NexusCore.open();
    final controller = AppController(core);
    try {
      await controller.initialize();
      return controller;
    } catch (_) {
      core.close();
      rethrow;
    }
  }

  void _retry() {
    setState(() => _controller = _initialize());
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Nexus',
      debugShowCheckedModeBanner: false,
      theme: nexusTheme(),
      home: FutureBuilder<AppController>(
        future: _controller,
        builder: (context, snapshot) {
          if (snapshot.hasData) {
            return NearbyScreen(controller: snapshot.requireData);
          }
          if (snapshot.hasError) {
            return _StartupError(error: snapshot.error!, onRetry: _retry);
          }
          return const _StartupLoading();
        },
      ),
    );
  }
}

ThemeData nexusTheme() => ThemeData(
      colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xff535bf2)),
      useMaterial3: true,
    );

class _StartupLoading extends StatelessWidget {
  const _StartupLoading();

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      body: SafeArea(
        child: Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.hub_outlined, size: 64),
              SizedBox(height: 20),
              Text('正在启动 Nexus'),
              SizedBox(height: 16),
              CircularProgressIndicator(),
            ],
          ),
        ),
      ),
    );
  }
}

class _StartupError extends StatelessWidget {
  const _StartupError({required this.error, required this.onRetry});

  final Object error;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.error_outline,
                    size: 64, color: Theme.of(context).colorScheme.error),
                const SizedBox(height: 20),
                Text('Nexus 启动失败',
                    style: Theme.of(context).textTheme.headlineSmall),
                const SizedBox(height: 12),
                SelectableText(error.toString(), textAlign: TextAlign.center),
                const SizedBox(height: 24),
                FilledButton.icon(
                    onPressed: onRetry,
                    icon: const Icon(Icons.refresh),
                    label: const Text('重试')),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
