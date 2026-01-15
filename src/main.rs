//! sharSelf 设备发现演示程序

// TUI 模块
mod ui;

use sharSelf::{
    discovery::{discovery_service, DiscoveryEvent, register_service},
    common::config::{DiscoveryConfig, ServiceConfig},
};
use std::collections::HashMap;
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> sharSelf::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

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
    println!("  5. TUI 界面（交互式终端界面）");
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
        "5" => run_tui().await,
        _ => {
            println!("❌ 无效选择，运行 TUI 界面...\n");
            run_tui().await
        }
    }
}

/// 运行 TUI 界面
async fn run_tui() -> sharSelf::Result<()> {
    // 关闭日志输出，避免干扰 TUI
    tracing::info!("Starting TUI interface...");

    ui::run_tui().await
}

/// 仅注册服务
async fn run_registrar_only(hostname: String) -> sharSelf::Result<()> {
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
