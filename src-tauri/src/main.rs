#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod http;
mod jobobject;
mod kernel;
mod settings;
mod updater;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// 全局内核控制句柄（跨线程共享，供 Tauri 事件/托盘/更新线程与内核线程互通）
pub struct KernelCtl {
    /// true = 请求停止内核（退出）
    pub stop: Arc<AtomicBool>,
    /// true = 请求重启内核（更新后）——内核停止后可做原子替换再拉起
    pub restart: Arc<AtomicBool>,
    /// 当前内核端口（就绪后写入）
    pub port: Arc<Mutex<Option<u16>>>,
    /// 连续崩溃时间戳（600s 滑动窗口，限次重启判定）
    pub crashes: Arc<Mutex<Vec<std::time::Instant>>>,
    /// 已准备的新内核目录 kernel.new（等待内核停止后交换）
    pub update_path: Arc<Mutex<Option<PathBuf>>>,
    /// 刚更新的版本号（用于崩溃循环自动回滚判定）
    pub updated_version: Arc<Mutex<Option<String>>>,
}

impl Default for KernelCtl {
    fn default() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            restart: Arc::new(AtomicBool::new(false)),
            port: Arc::new(Mutex::new(None)),
            crashes: Arc::new(Mutex::new(Vec::new())),
            update_path: Arc::new(Mutex::new(None)),
            updated_version: Arc::new(Mutex::new(None)),
        }
    }
}

/// 托盘句柄保活（TrayIcon 若 drop 则图标消失）
pub struct TrayHandle(pub tauri::tray::TrayIcon);

fn build_tray(app: &tauri::App, ctl: Arc<KernelCtl>) -> tauri::Result<tauri::tray::TrayIcon> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
    let check = MenuItemBuilder::with_id("check", "检查更新").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出 DSH Desktop").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &check])
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("DSH Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "check" => spawn_update(app.clone(), ctl.clone()),
            "quit" => {
                // 退出：置 stop → 内核优雅停止（/quit）→ 内核线程退出 → process::exit(0)
                ctl.stop.store(true, Ordering::SeqCst);
            }
            _ => {}
        })
        .build(app)
}

/// 托盘「检查更新」：后台线程执行 检查→确认→准备→请求重启
fn spawn_update(app: tauri::AppHandle, ctl: Arc<KernelCtl>) {
    std::thread::spawn(move || {
        let exe_dir = std::env::current_exe()
            .map(|p| p.parent().map(|d| d.to_path_buf()).unwrap_or_default())
            .unwrap_or_default();
        let data_dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"));
        let settings = settings::AppSettings::load(&data_dir);
        let kernel_root = match kernel::resolve_kernel_root(&settings, &exe_dir) {
            Ok(r) => r,
            Err(e) => {
                jobobject::show_error("DSH Desktop - 更新", &format!("无法定位内核：{e}"));
                return;
            }
        };
        let node = kernel::resolve_node_path(&settings, &exe_dir);

        let (msg, prepared) =
            updater::run_update_flow(&kernel_root, &node, |q| jobobject::show_question("DSH Desktop - 更新", q));
        if let Some((kernel_new, ver)) = prepared {
            *ctl.update_path.lock().unwrap() = Some(kernel_new);
            *ctl.updated_version.lock().unwrap() = Some(ver);
            ctl.restart.store(true, Ordering::SeqCst);
        }
        if msg != "已取消" {
            jobobject::show_info("DSH Desktop - 更新", &msg);
        }
    });
}

fn main() {
    let smoke = std::env::args().any(|a| a == "--smoke");
    let update_check = std::env::args().any(|a| a == "--update-check");
    let ctl = Arc::new(KernelCtl::default());

    // 无界面检查（交互证据/CI 用）：打印当前 vs registry latest，不弹窗不下载
    if update_check {
        let exe_dir = std::env::current_exe()
            .map(|p| p.parent().map(|d| d.to_path_buf()).unwrap_or_default())
            .unwrap_or_default();
        let data_dir = std::env::var("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join("com.dshdesktop.app"))
            .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"));
        let settings = settings::AppSettings::load(&data_dir);
        match kernel::resolve_kernel_root(&settings, &exe_dir) {
            Ok(root) => {
                let cur = updater::current_version(&root).unwrap_or_else(|| "?".into());
                match updater::check_latest(&root) {
                    None => println!("UPDATE_CHECK: up-to-date (current={cur} == registry latest)"),
                    Some(info) => println!("UPDATE_CHECK: available version={} (current={cur})", info.version),
                }
            }
            Err(e) => println!("UPDATE_CHECK: error {e}"),
        }
        std::process::exit(0);
    }

    let ctl_tray = ctl.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 第二实例：聚焦/恢复主窗口
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .setup(move |app| {
            // 托盘（M3）
            let tray = build_tray(app, ctl_tray.clone())?;
            app.manage(TrayHandle(tray));
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭按钮 → hide 到托盘（真正退出走托盘「退出」）
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build tauri app");

    // 内核线程先启动；run() 阻塞事件循环直至退出
    let handle = app.handle().clone();
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"));
    let kernel_ctl = KernelCtl {
        stop: ctl.stop.clone(),
        restart: ctl.restart.clone(),
        port: ctl.port.clone(),
        crashes: ctl.crashes.clone(),
        update_path: ctl.update_path.clone(),
        updated_version: ctl.updated_version.clone(),
    };
    std::thread::spawn(move || {
        let code = kernel::run_shell(handle, kernel_ctl, smoke, &data_dir);
        // 内核线程结束 = 壳可退出（--smoke 失败时非零退出码，P1-3）
        std::process::exit(code);
    });

    app.run(move |_app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            // 兜底：任何路径触发退出请求 → 停内核后由内核线程 exit(0)
            ctl.stop.store(true, Ordering::SeqCst);
            api.prevent_exit();
        }
        tauri::RunEvent::Exit => {
            ctl.stop.store(true, Ordering::SeqCst);
        }
        _ => {}
    });
}