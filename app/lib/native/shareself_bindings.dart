import 'dart:ffi';
import 'dart:io';
import 'package:ffi/ffi.dart';

/// FFI 绑定到 sharSelf Rust 库
class ShareSelfBindings {
  /// 动态库文件名 (根据平台不同)
  static const String _libName = 'sharSelf';

  /// 加载动态库
  static DynamicLibrary _openLibrary() {
    if (Platform.isAndroid || Platform.isLinux) {
      return DynamicLibrary.open('lib$_libName.so');
    }
    if (Platform.isIOS || Platform.isMacOS) {
      return DynamicLibrary.open('$_libName.framework/$_libName');
    }
    if (Platform.isWindows) {
      return DynamicLibrary.open('$_libName.dll');
    }
    throw UnsupportedError('Unknown platform: ${Platform.operatingSystem}');
  }

  static final DynamicLibrary _dylib = _openLibrary();

  // 获取版本信息
  late final Pointer<Utf8> Function() _sharSelfGetVersion;
  late final Pointer<Void> Function(int port, Pointer<Pointer<FfiResult>> resultOut) _sharSelfStartDiscovery;
  late final Pointer<FfiResult> Function(Pointer<Void> handle) _sharSelfStopDiscovery;
  late final Pointer<FfiDeviceInfo> Function(Pointer<Int32> countOut) _sharSelfGetDevices;
  late final void Function(Pointer<Utf8> s) _sharSelfFreeString;
  late final void Function(Pointer<FfiResult> result) _sharSelfFreeResult;
  late final void Function(Pointer<FfiDeviceInfo> devices, int count) _sharSelfFreeDeviceList;

  // 构造函数 - 加载所有符号
  ShareSelfBindings() {
    // 版本信息
    _sharSelfGetVersion = _dylib
        .lookupFunction<Pointer<Utf8> Function(), Pointer<Utf8> Function()>(
      'shareself_get_version',
    );

    // 启动发现
    _sharSelfStartDiscovery = _dylib.lookupFunction<
        Pointer<Void> Function(Uint32 port, Pointer<Pointer<FfiResult>> resultOut),
        Pointer<Void> Function(int port, Pointer<Pointer<FfiResult>> resultOut)
      >('shareself_start_discovery');

    // 停止发现
    _sharSelfStopDiscovery = _dylib.lookupFunction<
        Pointer<FfiResult> Function(Pointer<Void> handle),
        Pointer<FfiResult> Function(Pointer<Void> handle)
      >('shareself_stop_discovery');

    // 获取设备列表
    _sharSelfGetDevices = _dylib.lookupFunction<
        Pointer<FfiDeviceInfo> Function(Pointer<Int32> countOut),
        Pointer<FfiDeviceInfo> Function(Pointer<Int32> countOut)
      >('shareself_get_devices');

    // 内存释放函数
    _sharSelfFreeString = _dylib.lookupFunction<
        Void Function(Pointer<Utf8>),
        void Function(Pointer<Utf8>)
      >('shareself_free_string');

    _sharSelfFreeResult = _dylib.lookupFunction<
        Void Function(Pointer<FfiResult>),
        void Function(Pointer<FfiResult>)
      >('shareself_free_result');

    _sharSelfFreeDeviceList = _dylib.lookupFunction<
        Void Function(Pointer<FfiDeviceInfo>, Int32),
        void Function(Pointer<FfiDeviceInfo>, int)
      >('shareself_free_device_list');
  }

  /// 获取版本信息
  // ignore: non_constant_identifier_names
  Pointer<Utf8> get sharSelf_get_version => _sharSelfGetVersion();

  /// 启动设备发现
  // ignore: non_constant_identifier_names
  Pointer<Void> sharSelf_start_discovery(
    int port,
    Pointer<Pointer<FfiResult>> resultOut,
  ) {
    return _sharSelfStartDiscovery(port, resultOut);
  }

  /// 停止设备发现
  // ignore: non_constant_identifier_names
  Pointer<FfiResult> sharSelf_stop_discovery(
    Pointer<Void> handle,
  ) {
    return _sharSelfStopDiscovery(handle);
  }

  /// 获取设备列表
  // ignore: non_constant_identifier_names
  Pointer<FfiDeviceInfo> sharSelf_get_devices(
    Pointer<Int32> countOut,
  ) {
    return _sharSelfGetDevices(countOut);
  }

  /// 释放字符串内存
  // ignore: non_constant_identifier_names
  void sharSelf_free_string(Pointer<Utf8> s) {
    _sharSelfFreeString(s);
  }

  /// 释放结果内存
  // ignore: non_constant_identifier_names
  void sharSelf_free_result(Pointer<FfiResult> result) {
    _sharSelfFreeResult(result);
  }

  /// 释放设备列表内存
  // ignore: non_constant_identifier_names
  void sharSelf_free_device_list(
    Pointer<FfiDeviceInfo> devices,
    int count,
  ) {
    _sharSelfFreeDeviceList(devices, count);
  }
}

/// FFI 错误码
class FfiErrorCode {
  static const int success = 0;
  static const int unknownError = -1;
  static const int invalidArgument = -2;
  static const int nullPointer = -3;
  static const int utf8Error = -4;
  static const int discoveryError = -5;
}

/// FFI 结果结构体
base class FfiResult extends Struct {
  @Int32()
  external int errorCode;

  external Pointer<Utf8> errorMessage;
}

/// 设备信息结构体
base class FfiDeviceInfo extends Struct {
  external Pointer<Utf8> id;
  external Pointer<Utf8> name;
  external Pointer<Utf8> hostname;
  external Pointer<Utf8> address;
  @Uint32()
  external int port;
  external Pointer<Utf8> serviceType;
  external Pointer<Utf8> discoverySource; // 新增：发现来源
}
