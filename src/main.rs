//! sharSelf 设备发现演示程序

// TUI 模块
mod ui;

use sharSelf::{
    discovery::{discovery_service, DiscoveryEvent, register_service},
    common::config::{DiscoveryConfig, ServiceConfig},
    torrent::{TorrentFile, DEFAULT_BT_PORT},
};
use std::collections::HashMap;
use std::path::Path;
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> sharSelf::Result<()> {
    // 不在这里初始化日志，让各个模式自己初始化
    // run_tui() 会将日志写入文件，其他模式使用默认输出

    println!("🔍 sharSelf 设备发现演示");
    println!("========================================\n");

    // 获取本机设备名称
    let hostname = gethostname::gethostname()
        .into_string()
        .unwrap_or_else(|_| "Unknown".to_string());

    println!("📱 本机信息:");
    println!("   主机名: {}", hostname);
    println!("   服务类型: {}\n", sharSelf::DEFAULT_SERVICE_TYPE);

    // 显示菜单
    println!("请选择功能:");
    println!("  1. 注册自己（让其他设备能发现我）");
    println!("  2. 浏览设备（发现局域网内的设备）");
    println!("  3. 同时运行（既注册又浏览）");
    println!("  4. 快速测试（注册 + 浏览 10 秒）");
    println!("  5. 共享文件（通过 BitTorrent 分享文件）");
    println!("  6. TUI 界面（交互式终端界面）");
    println!("========================================");

    // 读取用户输入
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    let choice = input.trim();

    match choice {
        "1" => run_registrar_only(hostname).await,
        "2" => run_browser_only().await,
        "3" => run_both(hostname).await,
        "4" => run_quick_test(hostname).await,
        "5" => run_share_file().await,
        "6" => run_tui().await,
        _ => {
            println!("❌ 无效选择，运行 TUI 界面...\n");
            run_tui().await
        }
    }
}

/// 运行 TUI 界面
async fn run_tui() -> sharSelf::Result<()> {
    // 设置日志输出到文件
    use std::fs::OpenOptions;
    use std::sync::Mutex;
    use tracing_subscriber::fmt;

    // 创建日志文件
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/shareself.log")
        .expect("无法创建日志文件");

    // 创建文件日志订阅者
    let subscriber = fmt()
        .with_writer(Mutex::new(log_file))
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();

    // 设置全局订阅者
    tracing::subscriber::set_global_default(subscriber)
        .expect("无法设置日志订阅者");

    tracing::info!("=== sharSelf TUI 启动 ===");
    tracing::info!("时间: {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
    tracing::info!("日志文件: /tmp/shareself.log");
    tracing::info!("提示: 在另一个终端运行 'tail -f /tmp/shareself.log' 查看实时日志");

    ui::run_tui().await
}

/// 仅注册服务
async fn run_registrar_only(hostname: String) -> sharSelf::Result<()> {
    // 初始化日志到控制台
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("\n📡 模式: 仅注册服务");
    println!("----------------------------------------\n");

    let mut txt_records = HashMap::new();
    txt_records.insert("version".to_string(), "0.1.0".to_string());
    txt_records.insert("platform".to_string(), get_platform().to_string());
    txt_records.insert("model".to_string(), "Rust Device".to_string());

    let config = ServiceConfig {
        service_name: hostname.clone(),
        service_type: sharSelf::DEFAULT_SERVICE_TYPE.to_string(),
        port: 8080,
        txt_records,
        hostname: Some(hostname),
        ttl: 120,
        ..Default::default()
    };

    println!("⏳ 正在注册服务...");
    println!("   服务名: {}", config.service_name);
    println!("   端口: {}", config.port);
    println!("   TXT 记录: {:?}", config.txt_records);
    println!("\n💡 提示: 按 Ctrl+C 停止\n");

    match register_service(config) {
        Ok(service) => {
            println!("✅ 服务注册成功！");
            println!("📡 正在广播，等待其他设备发现...\n");

            // 持续运行直到用户中断
            tokio::signal::ctrl_c().await.ok();
            println!("\n🛑 收到停止信号，正在注销服务...");
            let _ = service.unregister().await;
            println!("✅ 已停止");
        }
        Err(e) => {
            println!("❌ 注册失败: {}", e);
        }
    }

    Ok(())
}

/// 仅浏览设备
async fn run_browser_only() -> sharSelf::Result<()> {
    println!("\n🔍 模式: 仅浏览设备");
    println!("----------------------------------------\n");

    let config = DiscoveryConfig::default();

    println!("⏳ 正在启动设备发现...");
    println!("   服务类型: {}", config.service_type);
    println!("\n💡 提示: 按 Ctrl+C 停止\n");

    match discovery_service(config) {
        Ok(mut discovery) => {
            println!("✅ 设备发现已启动！正在扫描...\n");

            let rx = discovery.receiver();
            let mut device_count = 0;

            loop {
                tokio::select! {
                    result = timeout(Duration::from_secs(1), rx.recv()) => {
                        match result {
                            Ok(Some(event)) => {
                                match event {
                                    DiscoveryEvent::DeviceFound(device) => {
                                        device_count += 1;
                                        println!("🎉 发现设备 #{}!", device_count);
                                        print_device_info(&device);
                                    }
                                    DiscoveryEvent::DeviceLost(name) => {
                                        println!("👋 设备离线: {}", name);
                                    }
                                    DiscoveryEvent::DeviceUpdated(device) => {
                                        println!("🔄 设备更新: {}", device.name);
                                    }
                                    DiscoveryEvent::Error(e) => {
                                        println!("⚠️  错误: {}", e);
                                    }
                                }
                            }
                            Ok(None) => {
                                println!("⚠️  事件通道已关闭");
                                break;
                            }
                            Err(_) => {
                                // 超时，继续等待
                            }
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        println!("\n🛑 收到停止信号");
                        break;
                    }
                }
            }

            println!("\n📊 统计: 共发现 {} 台设备", device_count);
            let _ = discovery.shutdown().await;
        }
        Err(e) => {
            println!("❌ 启动失败: {}", e);
        }
    }

    Ok(())
}

/// 同时注册和浏览
async fn run_both(hostname: String) -> sharSelf::Result<()> {
    println!("\n🔄 模式: 注册 + 浏览");
    println!("----------------------------------------\n");

    // 注册服务
    let mut txt_records = HashMap::new();
    txt_records.insert("version".to_string(), "0.1.0".to_string());
    txt_records.insert("platform".to_string(), get_platform().to_string());
    txt_records.insert("model".to_string(), "Rust Device".to_string());

    let service_config = ServiceConfig {
        service_name: hostname.clone(),
        service_type: sharSelf::DEFAULT_SERVICE_TYPE.to_string(),
        port: 8080,
        txt_records,
        hostname: Some(hostname.clone()),
        ttl: 120,
        ..Default::default()
    };

    println!("⏳ 正在注册服务...");
    let service = match register_service(service_config) {
        Ok(s) => {
            println!("✅ 服务注册成功！作为 '{}' 可被发现", s.name());
            s
        }
        Err(e) => {
            println!("❌ 注册失败: {}，继续浏览模式...", e);
            return run_browser_only().await;
        }
    };

    // 启动浏览
    let discovery_config = DiscoveryConfig::default();
    let mut discovery = match discovery_service(discovery_config) {
        Ok(d) => {
            println!("✅ 设备发现已启动！\n");
            d
        }
        Err(e) => {
            println!("❌ 浏览启动失败: {}", e);
            let _ = service.unregister().await;
            return Err(e);
        }
    };

    println!("💡 提示: 按 Ctrl+C 停止\n");

    let rx = discovery.receiver();
    let mut device_count = 0;

    loop {
        tokio::select! {
            result = timeout(Duration::from_secs(1), rx.recv()) => {
                match result {
                    Ok(Some(event)) => {
                        match event {
                            DiscoveryEvent::DeviceFound(device) => {
                                device_count += 1;
                                println!("🎉 发现设备 #{}!", device_count);
                                print_device_info(&device);
                            }
                            DiscoveryEvent::DeviceLost(name) => {
                                println!("👋 设备离线: {}", name);
                            }
                            DiscoveryEvent::DeviceUpdated(device) => {
                                println!("🔄 设备更新: {}", device.name);
                            }
                            DiscoveryEvent::Error(e) => {
                                println!("⚠️  错误: {}", e);
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\n🛑 收到停止信号");
                break;
            }
        }
    }

    println!("\n📊 统计: 共发现 {} 台设备", device_count);
    let _ = discovery.shutdown().await;
    let _ = service.unregister().await;

    Ok(())
}

/// 快速测试（10秒）
async fn run_quick_test(hostname: String) -> sharSelf::Result<()> {
    println!("\n⚡ 模式: 快速测试 (10秒)");
    println!("----------------------------------------\n");

    // 注册服务
    let mut txt_records = HashMap::new();
    txt_records.insert("version".to_string(), "0.1.0".to_string());
    txt_records.insert("platform".to_string(), get_platform().to_string());
    txt_records.insert("model".to_string(), "Rust Device".to_string());

    let service_config = ServiceConfig {
        service_name: hostname.clone(),
        service_type: sharSelf::DEFAULT_SERVICE_TYPE.to_string(),
        port: 8080,
        txt_records,
        hostname: Some(hostname),
        ttl: 120,
        ..Default::default()
    };

    println!("📡 正在注册服务...");
    let _service = register_service(service_config)?;

    println!("✅ 服务已注册！正在扫描设备...\n");

    // 启动浏览
    let discovery_config = DiscoveryConfig {
        timeout_secs: Some(10),
        ..Default::default()
    };

    let mut discovery = discovery_service(discovery_config)?;
    let rx = discovery.receiver();
    let mut device_count = 0;

    println!("⏱️  扫描 10 秒...\n");

    let start = std::time::Instant::now();

    loop {
        let elapsed = start.elapsed();

        if elapsed >= Duration::from_secs(10) {
            break;
        }

        match timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Some(event)) => {
                match event {
                    DiscoveryEvent::DeviceFound(device) => {
                        device_count += 1;
                        println!("🎉 发现设备 #{}!", device_count);
                        print_device_info(&device);
                    }
                    DiscoveryEvent::DeviceLost(name) => {
                        println!("👋 设备离线: {}", name);
                    }
                    DiscoveryEvent::DeviceUpdated(device) => {
                        println!("🔄 设备更新: {}", device.name);
                    }
                    DiscoveryEvent::Error(e) => {
                        println!("⚠️  错误: {}", e);
                    }
                }
            }
            _ => {
                // 显示倒计时
                let remaining = 10 - elapsed.as_secs();
                print!("\r⏳ 剩余 {} 秒... ", remaining);
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
        }
    }

    println!("\n\n📊 测试结果: 共发现 {} 台设备", device_count);

    Ok(())
}

/// 打印设备信息
fn print_device_info(device: &sharSelf::DeviceInfo) {
    println!("   ├─ 名称: {}", device.name);
    println!("   ├─ 主机: {}", device.hostname);
    println!("   ├─ 地址: {:?}", device.addresses);
    println!("   ├─ 端口: {}", device.port);

    if !device.txt_records.is_empty() {
        println!("   ├─ 信息:");
        for (key, value) in &device.txt_records {
            println!("   │   ├─ {}: {}", key, value);
        }
    }

    println!("   └─ 类型: {}\n", device.service_type);
}

/// 获取平台信息
fn get_platform() -> &'static str {
    #[cfg(target_os = "android")]
    return "Android";
    #[cfg(target_os = "ios")]
    return "iOS";
    #[cfg(target_os = "linux")]
    return "Linux";
    #[cfg(target_os = "macos")]
    return "macOS";
    #[cfg(target_os = "windows")]
    return "Windows";
    #[cfg(target_os = "freebsd")]
    return "FreeBSD";
    #[cfg(not(any(
        target_os = "android",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "windows",
        target_os = "freebsd"
    )))]
    return "Unknown";
}

/// 共享文件
async fn run_share_file() -> sharSelf::Result<()> {
    println!("\n📁 模式: 共享文件");
    println!("----------------------------------------\n");

    // 提示用户输入文件路径
    println!("请输入要共享的文件或目录路径:");
    let mut path_input = String::new();
    std::io::stdin().read_line(&mut path_input).ok();
    let path_str = path_input.trim();

    let path = Path::new(path_str);

    // 检查路径是否存在
    if !path.exists() {
        println!("❌ 路径不存在: {}", path_str);
        return Ok(());
    }

    println!("\n⏳ 正在创建 torrent 文件...");

    // 创建 torrent 文件
    let torrent = match TorrentFile::create(path, None) {
        Ok(t) => t,
        Err(e) => {
            println!("❌ 创建 torrent 失败: {}", e);
            return Err(e.into());
        }
    };

    println!("✅ Torrent 文件创建成功！\n");
    println!("📋 Torrent 信息:");
    println!("   ├─ 名称: {}", torrent.metainfo.info.name);
    println!("   ├─ 大小: {} bytes", torrent.metainfo.total_size());
    println!("   ├─ Piece 数量: {}", torrent.piece_count());
    println!("   ├─ Piece 大小: {} bytes", torrent.metainfo.info.piece_length);

    // 显示 info_hash
    match torrent.info_hash() {
        Ok(hash) => {
            println!("   ├─ Info Hash: {}", hex::encode(hash));
        }
        Err(e) => {
            println!("   ├─ Info Hash: 获取失败 ({})", e);
        }
    }

    println!("   └─ 本地路径: {}\n", path_str);

    // 获取本机 IP 地址
    let local_ip = get_local_ip().unwrap_or_else(|| "0.0.0.0".to_string());
    let listen_addr = format!("{}:{}", local_ip, DEFAULT_BT_PORT);
    let listen_addr = listen_addr.parse::<std::net::SocketAddr>()
        .unwrap_or_else(|_| format!("0.0.0.0:{}", DEFAULT_BT_PORT).parse().unwrap());

    println!("📡 正在启动种子服务...");
    println!("   ├─ 监听地址: {}", listen_addr);
    println!("   └─ 端口: {}\n", DEFAULT_BT_PORT);

    // 创建 PieceManager
    use sharSelf::torrent::PieceManager;
    use std::sync::Arc;

    let storage_path = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };

    let piece_manager = Arc::new(PieceManager::new(
        torrent.metainfo.clone(),
        storage_path,
    ));

    // 创建并启动 Seeder
    use sharSelf::torrent::Seeder;
    let seeder = Seeder::new(
        torrent.metainfo.clone(),
        piece_manager.clone(),
        listen_addr,
    );

    println!("✅ 种子服务已启动！\n");
    println!("💡 其他设备可以通过以下方式连接:");
    println!("   ├─ Info Hash (hex): {}\n", hex::encode(torrent.info_hash().unwrap_or([0u8; 20])));
    println!("💡 提示: 按 Ctrl+C 停止共享\n");

    // 在后台启动 seeder
    let seeder_handle = tokio::spawn(async move {
        let _ = seeder.start().await;
    });

    // 等待用户中断
    tokio::signal::ctrl_c().await.ok();
    println!("\n🛑 收到停止信号，正在关闭种子服务...");

    // 取消 seeder 任务
    seeder_handle.abort();

    println!("✅ 已停止");

    Ok(())
}

/// 获取本机 IP 地址
fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;

    // 通过连接到一个外部地址来获取本机 IP
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    Some(local_addr.ip().to_string())
}
