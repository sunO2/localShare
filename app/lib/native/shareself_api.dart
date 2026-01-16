import 'dart:ffi';
import 'package:ffi/ffi.dart';
import 'shareself_bindings.dart';

/// 设备信息模型
class DeviceInfo {
  final String id;
  final String name;
  final String hostname;
  final String address;
  final int port;
  final String serviceType;
  final String discoverySource; // 新增：发现来源

  DeviceInfo({
    required this.id,
    required this.name,
    required this.hostname,
    required this.address,
    required this.port,
    required this.serviceType,
    required this.discoverySource,
  });

  @override
  String toString() {
    return 'DeviceInfo(name: $name, address: $address:$port, source: $discoverySource)';
  }
}

/// ShareSelf Native API 的 Dart 包装器
class ShareSelfApi {
  late final ShareSelfBindings _bindings;

  /// 单例模式
  static final ShareSelfApi _instance = ShareSelfApi._internal();
  factory ShareSelfApi() => _instance;
  ShareSelfApi._internal() {
    _bindings = ShareSelfBindings();
  }

  /// 获取库版本
  String getVersion() {
    final versionPtr = _bindings.sharSelf_get_version;
    if (versionPtr == nullptr) {
      return 'unknown';
    }
    final version = versionPtr.toDartString();
    _bindings.sharSelf_free_string(versionPtr);
    return version;
  }

  /// 启动设备发现
  /// 返回: (成功标志, 错误消息)
  Future<(bool, String?)> startDiscovery(int port) async {
    final resultOut = calloc<Pointer<FfiResult>>();
    // ignore: unused_local_variable
    final handle = _bindings.sharSelf_start_discovery(port, resultOut);

    final resultPtr = resultOut.value;
    final result = resultPtr.ref;
    final success = result.errorCode == FfiErrorCode.success;

    String? errorMsg;
    if (!success && result.errorMessage != nullptr) {
      errorMsg = result.errorMessage.toDartString();
    }

    _bindings.sharSelf_free_result(resultPtr);
    calloc.free(resultOut);

    // TODO: 存储 handle 用于后续停止发现
    return (success, errorMsg);
  }

  /// 停止设备发现
  (bool, String?) stopDiscovery() {
    // TODO: 使用存储的 handle
    final resultPtr = _bindings.sharSelf_stop_discovery(nullptr);
    final result = resultPtr.ref;
    final success = result.errorCode == FfiErrorCode.success;

    String? errorMsg;
    if (!success && result.errorMessage != nullptr) {
      errorMsg = result.errorMessage.toDartString();
    }

    _bindings.sharSelf_free_result(resultPtr);
    return (success, errorMsg);
  }

  /// 获取已发现的设备列表
  List<DeviceInfo> getDevices() {
    final countOut = calloc<Int32>();
    final devicesPtr = _bindings.sharSelf_get_devices(countOut);

    final count = countOut.value;
    calloc.free(countOut);

    if (count == 0 || devicesPtr == nullptr) {
      return [];
    }

    final devices = <DeviceInfo>[];
    for (int i = 0; i < count; i++) {
      final device = devicesPtr[i];
      devices.add(DeviceInfo(
        id: device.id.toDartString(),
        name: device.name.toDartString(),
        hostname: device.hostname.toDartString(),
        address: device.address.toDartString(),
        port: device.port,
        serviceType: device.serviceType.toDartString(),
        discoverySource: device.discoverySource.toDartString(),
      ));
    }

    // 释放设备列表内存
    _bindings.sharSelf_free_device_list(devicesPtr, count);

    return devices;
  }
}
