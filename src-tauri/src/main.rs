#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod http;
mod jobobject;
mod kernel;
mod settings;

use std::sync::{Arc, Mutex};
use tauri::Manager;

/// 全局内核控制句柄（跨线程共享，供 Tauri 事件回调与内核线程互通）
pub struct KernelCtl {
    /// true = 请求停止内核（用户退出/窗口关闭）
    pub stop: Arc<std::sync::atomic::AtomicBool>,
    /// 当前内核端口（就绪后写入）
    pub port: Arc<Mutex<Option<u16>>>,
    /// 连续崩溃时间戳（限次重启判定，600s 滑动窗口）
    pub crashes: Arc<Mutex<Vec<std::time::Instant>>>,
}

impl Default for KernelCtl {
    fn default() -> Self {
        Self {
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            port: Arc::new(Mutex::new(None)),
            crashes: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

fn main() {
    let smoke = std::env::args().any(|a| a == "--smoke");
    let ctl = KernelCtl::default();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 第二实例：聚焦/恢复主窗口
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .build(tauri::generate_context!())
        .expect("failed to build tauri app");

    // 内核线程先启动（封装进 AppHandle / data_dir），run() 阻塞事件循环直至退出
    let handle = app.handle().clone();
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"));
    let ctl2 = KernelCtl {
        stop: ctl.stop.clone(),
        port: ctl.port.clone(),
        crashes: ctl.crashes.clone(),
    };
    std::thread::spawn(move || {
        kernel::run_shell(handle, ctl2, smoke, &data_dir);
        // 内核线程结束 = 壳可退出
        std::process::exit(0);
    });

    app.run(move |_app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            // 窗口全部关闭/用户退出 → 先停内核（优雅 /quit），阻止立即退出
            ctl.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            api.prevent_exit();
        }
        tauri::RunEvent::Exit => {
            ctl.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        _ => {}
    });
}