#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod guard;
mod http;
mod jobobject;
mod kernel;
mod settings;
mod updater;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// 全局内核控制句柄（跨线程共享）
pub struct KernelCtl {
    pub stop: Arc<AtomicBool>,
    pub restart: Arc<AtomicBool>,
    pub port: Arc<Mutex<Option<u16>>>,
    pub crashes: Arc<Mutex<Vec<std::time::Instant>>>,
    pub update_path: Arc<Mutex<Option<PathBuf>>>,
    pub updated_version: Arc<Mutex<Option<String>>>,
    /// D：降级模式（无 desktop patch 运行；/quit 不可用）
    pub degraded: Arc<AtomicBool>,
    /// D：降级警告只弹一次
    pub degraded_warned: Arc<AtomicBool>,
    /// F：低内存警告只弹一次
    pub mem_warned: Arc<AtomicBool>,
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
            degraded: Arc::new(AtomicBool::new(false)),
            degraded_warned: Arc::new(AtomicBool::new(false)),
            mem_warned: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl KernelCtl {
    /// 消费式读取重启请求（原子复位，P0-13 回归保护）
    pub fn consume_restart(&self) -> bool {
        self.restart.swap(false, Ordering::SeqCst)
    }
}

/// 内核失败路径：把错误文本注入 loading 页 #status（E 规格兜底）
pub fn set_loading_status(app: &tauri::AppHandle, text: &str) {
    let app2 = app.clone();
    let t = text.to_string();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window("main") {
            let js = format!("window.setStatus({});", serde_json::to_string(&t).unwrap_or_else(|_| "\"\"".into()));
            let _ = w.eval(&js);
        }
    });
}
pub struct TrayHandle(pub tauri::tray::TrayIcon);
pub struct TrayState(pub tauri::tray::TrayIcon);

// ---------- 注册表助手（自启 C + 深链 C，原生 API 规避 reg.exe 引号解析问题） ----------
#[cfg(windows)]
mod regn {

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn to_str(p: &std::path::Path) -> String {
        p.to_string_lossy().to_string()
    }

    pub fn set_value(key: &str, value: &str, data: &str) -> bool {
        use windows_sys::Win32::System::Registry::{
            RegCreateKeyExW, RegSetValueExW, REG_OPTION_NON_VOLATILE, REG_SZ, HKEY_CURRENT_USER,
            KEY_SET_VALUE,
        };
        let mut hkey: *mut core::ffi::c_void = std::ptr::null_mut();
        let kw = wide(key);
        unsafe {
            let ok = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                kw.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            );
            if ok != 0 {
                return false;
            }
            let vw = wide(value);
            let dw = wide(data);
            let nbytes = (dw.len() - 1) * 2;
            let ok2 = RegSetValueExW(hkey, vw.as_ptr(), 0, REG_SZ, dw.as_ptr() as *const u8, nbytes as u32);
            if ok2 != 0 {
                return false;
            }
        }
        true
    }

    fn open(key: &str) -> Option<*mut core::ffi::c_void> {
        use windows_sys::Win32::System::Registry::{
            RegOpenKeyExW, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
        };
        let kw = wide(key);
        let mut hkey: *mut core::ffi::c_void = std::ptr::null_mut();
        unsafe {
            let ok = RegOpenKeyExW(HKEY_CURRENT_USER, kw.as_ptr(), 0, KEY_QUERY_VALUE, &mut hkey);
            if ok != 0 {
                return None;
            }
            Some(hkey)
        }
    }

    pub fn get_value(key: &str, value: &str) -> Option<String> {
        use windows_sys::Win32::System::Registry::{RegQueryValueExW, RegCloseKey, REG_SZ};
        let h = open(key)?;
        unsafe {
            let vw = wide(value);
            let buf = [0u16; 1024];
            let mut size = (buf.len() * 2) as u32;
            let mut kind = 0u32;
            let ok = RegQueryValueExW(
                h,
                vw.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                buf.as_ptr() as *mut u8,
                &mut size,
            );
            RegCloseKey(h);
            if ok != 0 || kind != REG_SZ {
                return None;
            }
            let len = (size as usize) / 2;
            let s: String = buf[..len].iter().take_while(|c| **c != 0).map(|c| *c as u8 as char).collect();
            Some(s)
        }
    }

    pub fn delete_value(key: &str, value: &str) -> bool {
        use windows_sys::Win32::System::Registry::{RegOpenKeyExW, RegDeleteValueW, RegCloseKey, HKEY_CURRENT_USER, KEY_SET_VALUE};
        let kw = wide(key);
        let mut hkey: *mut core::ffi::c_void = std::ptr::null_mut();
        unsafe {
            let ok = RegOpenKeyExW(HKEY_CURRENT_USER, kw.as_ptr(), 0, KEY_SET_VALUE, &mut hkey);
            if ok != 0 {
                return false;
            }
            let vw = wide(value);
            let r = RegDeleteValueW(hkey, vw.as_ptr());
            RegCloseKey(hkey);
            r == 0
        }
    }

    pub fn reg_set_autostart(exe: &std::path::Path, enable: bool) -> bool {
        let key = r"Software\Microsoft\Windows\CurrentVersion\Run";
        if enable {
            let data = format!("\"{}\"", to_str(exe));
            set_value(key, "DSH Desktop", &data)
        } else {
            delete_value(key, "DSH Desktop")
        }
    }

    pub fn reg_is_autostart() -> bool {
        get_value(r"Software\Microsoft\Windows\CurrentVersion\Run", "DSH Desktop").is_some()
    }

    /// 深链协议幂等注册（启动时执行；卸载不清理——README 已记录）
    pub fn reg_register_deeplink(exe: &std::path::Path) -> bool {
        let cls = r"Software\Classes\dsh-desktop";
        let mut ok = set_value(cls, "", "URL:DSH Desktop");
        ok &= set_value(cls, "URL Protocol", "");
        ok &= set_value(r"Software\Classes\dsh-desktop\shell\open\command", "", &format!("\"{}\" \"%1\"", to_str(exe)));
        ok
    }
}

fn app_data_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"))
}

fn build_tray(
    app: &tauri::App,
    ctl: Arc<KernelCtl>,
    exe: std::path::PathBuf,
    data_dir: PathBuf,
) -> tauri::Result<tauri::tray::TrayIcon> {
    use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
    let check = MenuItemBuilder::with_id("check", "检查更新").build(app)?;

    let settings = settings::AppSettings::load(&data_dir);
    let independent = CheckMenuItemBuilder::with_id("independent", "独立数据模式")
        .checked(!settings.dsh_home.is_empty())
        .build(app)?;
    let autostart = CheckMenuItemBuilder::with_id("autostart", "开机自启")
        .checked(regn::reg_is_autostart())
        .build(app)?;

    let quit = MenuItemBuilder::with_id("quit", "退出 DSH Desktop").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &check, &independent, &autostart])
        .separator()
        .item(&quit)
        .build()?;

    let ctl2 = ctl.clone();
    let data2 = data_dir.clone();
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
            "check" => spawn_update(app.clone(), ctl2.clone()),
            "quit" => {
                ctl2.stop.store(true, Ordering::SeqCst);
            }
            "independent" => {
                // B：切换数据模式 → 立即重启内核生效
                let cur = settings::AppSettings::load(&data2);
                let enabling = cur.dsh_home.is_empty();
                let mut next = cur.clone();
                if enabling {
                    let home = data2.join("dsh-home");
                    let _ = std::fs::create_dir_all(&home);
                    next.dsh_home = home.display().to_string();
                } else {
                    next.dsh_home = String::new();
                }
                let _ = next.save(&data2);
                let ask = if enabling {
                    "已切换到「独立数据模式」：数据存于 app_data/dsh-home，与浏览器版不共享。\n内核将立即重启。"
                } else {
                    "已切回「共享 ~/.dsh」模式（与浏览器版数据互通）。\n内核将立即重启。"
                };
                jobobject::show_info("DSH Desktop - 数据模式", ask);
                ctl2.restart.store(true, Ordering::SeqCst);
            }
            "autostart" => {
                let enable = !regn::reg_is_autostart();
                let ok = regn::reg_set_autostart(&exe, enable);
                jobobject::show_info(
                    "DSH Desktop - 开机自启",
                    if ok {
                        if enable { "已开启开机自启。" } else { "已关闭开机自启。" }
                    } else {
                        "注册表操作失败（请检查权限/写入失败）。"
                    },
                );
            }
            _ => {}
        })
        .build(app)
}

fn spawn_update(app: tauri::AppHandle, ctl: Arc<KernelCtl>) {
    std::thread::spawn(move || {
        let exe_dir = std::env::current_exe()
            .map(|p| p.parent().map(|d| d.to_path_buf()).unwrap_or_default())
            .unwrap_or_default();
        let data_dir = app_data_dir(&app);
        let settings = settings::AppSettings::load(&data_dir);
        let kernel_root = match kernel::resolve_kernel_root(&settings, &exe_dir) {
            Ok(r) => r,
            Err(e) => {
                jobobject::show_error("DSH Desktop - 更新", &format!("无法定位内核：{e}"));
                return;
            }
        };
        let node = kernel::resolve_node_path(&settings, &exe_dir);
        let exe2 = exe_dir.clone();
        let (msg, prepared) = updater::run_update_flow(&kernel_root, &node, &exe2, |q| {
            jobobject::show_question("DSH Desktop - 更新", q)
        });
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
    let indep_flag: Option<bool> = std::env::args()
        .position(|a| a == "--set-independent")
        .and_then(|i| std::env::args().nth(i + 1))
        .map(|v| v == "1");
    let autostart_flag: Option<bool> = std::env::args()
        .position(|a| a == "--set-autostart")
        .and_then(|i| std::env::args().nth(i + 1))
        .map(|v| v == "1");
    let ctl = Arc::new(KernelCtl::default());

    let exe_path = std::env::current_exe().unwrap_or_default();

    if update_check {
        let exe_dir = exe_path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let data_dir = std::env::var("APPDATA")
            .map(|a| PathBuf::from(a).join("com.dshdesktop.app"))
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

    // CLI 钩子（自测/运维）：--set-independent 0|1 / --set-autostart 0|1
    if let Some(flag) = indep_flag {
        let data_dir = std::env::var("APPDATA")
            .map(|a| PathBuf::from(a).join("com.dshdesktop.app"))
            .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"));
        let mut st = settings::AppSettings::load(&data_dir);
        if flag {
            let home = data_dir.join("dsh-home");
            let _ = std::fs::create_dir_all(&home);
            st.dsh_home = home.display().to_string();
        } else {
            st.dsh_home = String::new();
        }
        let _ = st.save(&data_dir);
        println!("INDEPENDENT_SET={}", if flag { "isolation" } else { "shared" });
        std::process::exit(0);
    }
    if let Some(flag) = autostart_flag {
        let ok = regn::reg_set_autostart(&exe_path, flag);
        println!("AUTOSTART_SET={} ok={ok}", if flag { "on" } else { "off" });
        std::process::exit(0);
    }

    // C：深链协议幂等注册（启动时）
    let _ = regn::reg_register_deeplink(&exe_path);

    // A：并发守卫（仅共享默认 DSH_HOME 时检测；--smoke 跳过）
    if !smoke {
        let data_dir = std::env::var("APPDATA")
            .map(|a| PathBuf::from(a).join("com.dshdesktop.app"))
            .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"));
        let settings = settings::AppSettings::load(&data_dir);
        let shared_default = settings.dsh_home.is_empty() && std::env::var("DSH_HOME").is_err();
        if shared_default {
            let own = exe_path
                .parent()
                .map(|d| d.join("kernel").display().to_string())
                .unwrap_or_default();
            let found = guard::detect_foreign_kernel(&own);
            if !found.is_empty() {
                let ask = format!(
                    "检测到另一个 DSH 内核正在共享数据目录 ~/.dsh 运行。
双实例并发会写坏会话日志（torn JSONL）。
建议先关闭另一实例（浏览器版 dsh web）。

仍要继续启动吗？

命中：
{}",
                    found.join("
    ")
                );
                // 测试钩子：DSH_DESKTOP_GUARD_ANSWER=no/yes（绕过对话框自动应答）
                let answer = std::env::var("DSH_DESKTOP_GUARD_ANSWER").ok();
                let allow = match answer.as_deref() {
                    Some("no") => false,
                    Some("yes") => true,
                    _ => jobobject::show_question("DSH Desktop - 并发检测", &ask),
                };
                if !allow {
                    eprintln!("[guard] foreign kernel detected, user declined");
                    std::process::exit(0);
                }
            }
        }
    }

    let ctl_tray = ctl.clone();

    let data_dir_for_tray = std::env::var("APPDATA")
        .map(|a| PathBuf::from(a).join("com.dshdesktop.app"))
        .unwrap_or_else(|_| std::env::temp_dir().join("dsh-desktop"));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            let _ = args; // v1 不解析深链参数（C）
        }))
        .setup(move |app| {
            // C：自启菜单初始状态由注册表实读（build_tray 内）
            let tray = build_tray(app, ctl_tray.clone(), exe_path.clone(), data_dir_for_tray.clone())?;
            app.manage(TrayHandle(tray));

            // E：立即窗口——setup 阶段即建 loading 页（内核就绪后导航到真实 UI）
            let hw = settings::AppSettings::load(&data_dir_for_tray).hardware_acceleration;
            let mut args: Vec<&str> = vec![
                // 保留 wry 默认 + 防节流（托盘常驻不被 background-throttle 卡顿）
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
                "--disable-background-timer-throttling",
                "--disable-renderer-backgrounding",
                "--disable-backgrounding-occluded-windows",
            ];
            if !hw {
                args.push("--disable-gpu");
            }
            let builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("loading.html".into()),
            )
            .title("DSH Desktop")
            .inner_size(1280.0, 820.0)
            .additional_browser_args(&args.join(" "));
            if let Err(e) = builder.build() {
                eprintln!("[shell] loading window build failed: {e}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build tauri app");

    let handle = app.handle().clone();
    let data_dir = app_data_dir(&handle);
    let kernel_ctl = KernelCtl {
        stop: ctl.stop.clone(),
        restart: ctl.restart.clone(),
        port: ctl.port.clone(),
        crashes: ctl.crashes.clone(),
        update_path: ctl.update_path.clone(),
        updated_version: ctl.updated_version.clone(),
        degraded: ctl.degraded.clone(),
        degraded_warned: ctl.degraded_warned.clone(),
        mem_warned: ctl.mem_warned.clone(),
    };
    std::thread::spawn(move || {
        let code = kernel::run_shell(handle, kernel_ctl, smoke, &data_dir);
        std::process::exit(code);
    });

    app.run(move |_app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            ctl.stop.store(true, Ordering::SeqCst);
            api.prevent_exit();
        }
        tauri::RunEvent::Exit => {
            ctl.stop.store(true, Ordering::SeqCst);
        }
        _ => {}
    });
}