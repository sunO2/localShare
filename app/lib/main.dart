import 'dart:io';
import 'package:flutter/material.dart';
import 'native/shareself_api.dart';

void main() {
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'ShareSelf',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple),
        useMaterial3: true,
      ),
      home: const ShareSelfHomePage(),
    );
  }
}

class ShareSelfHomePage extends StatefulWidget {
  const ShareSelfHomePage({super.key});

  @override
  State<ShareSelfHomePage> createState() => _ShareSelfHomePageState();
}

class _ShareSelfHomePageState extends State<ShareSelfHomePage> {
  final _api = ShareSelfApi();
  String _version = 'Loading...';
  String _status = 'Ready';
  bool _isDiscovering = false;
  List<DeviceInfo> _devices = [];

  @override
  void initState() {
    super.initState();
    _loadVersion();
  }

  Future<void> _loadVersion() async {
    try {
      final version = _api.getVersion();
      setState(() {
        _version = version;
      });
    } catch (e) {
      setState(() {
        _version = 'Error: $e';
      });
    }
  }

  Future<void> _toggleDiscovery() async {
    if (_isDiscovering) {
      final (success, error) = _api.stopDiscovery();
      setState(() {
        _isDiscovering = false;
        _status = success ? 'Discovery stopped' : 'Error: $error';
        _devices = [];
      });
    } else {
      setState(() {
        _status = 'Starting discovery...';
        _devices = [];
      });
      final (success, error) = await _api.startDiscovery(8080);
      setState(() {
        _isDiscovering = success;
        _status = success ? 'Discovering devices on port 8080' : 'Error: $error';
      });

      // 开始轮询设备列表
      if (success) {
        _startDevicePolling();
      }
    }
  }

  void _startDevicePolling() {
    Future.doWhile(() async {
      if (!_isDiscovering) return false;

      await Future.delayed(const Duration(seconds: 2));
      if (!_isDiscovering) return false;

      final devices = _api.getDevices();
      if (mounted) {
        setState(() {
          _devices = devices;
        });
      }
      return _isDiscovering;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
        title: const Text('ShareSelf'),
        actions: [
          if (_devices.isNotEmpty)
            Center(
              child: Padding(
                padding: const EdgeInsets.only(right: 16),
                child: Text(
                  '${_devices.length} device${_devices.length > 1 ? 's' : ''} found',
                  style: const TextStyle(fontWeight: FontWeight.bold),
                ),
              ),
            ),
        ],
      ),
      body: Padding(
        padding: const EdgeInsets.all(16.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // 版本信息
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16.0),
                child: Row(
                  children: [
                    const Icon(Icons.info_outline),
                    const SizedBox(width: 16),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          const Text('Library Version',
                              style: TextStyle(fontWeight: FontWeight.bold)),
                          const SizedBox(height: 4),
                          Text(_version),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 16),

            // 状态信息
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16.0),
                child: Row(
                  children: [
                    Icon(
                      _isDiscovering ? Icons.wifi : Icons.wifi_off,
                      color: _isDiscovering ? Colors.green : Colors.grey,
                    ),
                    const SizedBox(width: 16),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          const Text('Discovery Status',
                              style: TextStyle(fontWeight: FontWeight.bold)),
                          const SizedBox(height: 4),
                          Text(_status),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 16),

            // 设备列表
            Expanded(
              child: _devices.isEmpty
                  ? Center(
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.center,
                        children: [
                          Icon(
                            _isDiscovering ? Icons.search : Icons.devices_other,
                            size: 64,
                            color: Colors.grey,
                          ),
                          const SizedBox(height: 16),
                          Text(
                            _isDiscovering ? 'Searching for devices...' : 'No devices found',
                            style: const TextStyle(fontSize: 16, color: Colors.grey),
                          ),
                        ],
                      ),
                    )
                  : ListView.builder(
                      itemCount: _devices.length,
                      itemBuilder: (context, index) {
                        final device = _devices[index];
                        return Card(
                          margin: const EdgeInsets.only(bottom: 8),
                          child: ListTile(
                            leading: const CircleAvatar(
                            child: Icon(Icons.phone_android),
                          ),
                          title: Text(device.name),
                          subtitle: Text('${device.address}:${device.port}'),
                          trailing: const Icon(Icons.chevron_right),
                            onTap: () {
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(content: Text('Selected: ${device.name}')),
                              );
                            },
                          ),
                        );
                      },
                    ),
            ),

            const SizedBox(height: 16),

            // 控制按钮
            Center(
              child: ElevatedButton.icon(
                onPressed: _toggleDiscovery,
                icon: Icon(_isDiscovering ? Icons.stop : Icons.play_arrow),
                label: Text(_isDiscovering ? 'Stop Discovery' : 'Start Discovery'),
                style: ElevatedButton.styleFrom(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 32,
                    vertical: 16,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  String _getPlatformName() {
    if (Platform.isAndroid) return 'Android';
    if (Platform.isIOS) return 'iOS';
    if (Platform.isMacOS) return 'macOS';
    if (Platform.isWindows) return 'Windows';
    if (Platform.isLinux) return 'Linux';
    return 'Unknown';
  }
}
