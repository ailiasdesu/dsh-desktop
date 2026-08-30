//! M2 核心：DSH 内核子进程全生命周期管理（属主模式）
//! 契约（ARCHITECTURE.md §4/§10）：
//!  - spawn: <node> <kernel>/lib/bin.js web --patch <patch> --no-open --port <0|fixed>
//!    （--patch 等 launcher flags 必须最先，否则 unknown option --patch）
//!  - 就绪: stdout 正则 ^dsh web: http://127.0.0.1:(\d+)
//!  - 停止: GET /quit（patch 挂载，Windows 唯一优雅路径）→ ≤8s → JobObject 树级硬杀
//!  - 崩溃: 相同版本连续崩溃 ≤2 次自动重启（退避 1s/5s），超限 → 错误对话框
use crate::http;
use crate::jobobject::JobObject;
use crate::settings::AppSettings;
use crate::KernelCtl;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::Manager;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

const MAX_AUTO_RESTART: usize = 2; // 同一版本连续崩溃上限（600s 窗口）
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const QUIT_WAIT: Duration = Duration::from_secs(8);
const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(5)];
const FIXED_PORT_PREFIX: &str = "fixed:";

pub enum KernelEvent {
    Ready(u16),
    Line(String),
}

/// 就绪行契约：dsh web: http://127.0.0.1:<port>
/// 低内存滞回状态机（F 纯函数，可单测）：
/// 参数：mem_mb=当前可用提交内存(MB)；warn_mb=预警阈值(0=关)；reset_mb=恢复阈值(滞回)；warned=当前是否已弹过。
/// 返回：(should_warn, next_warned)——should_warn=本次是否弹；next_warned=下一状态。
pub fn memory_warn_transition(mem_mb: u64, warn_mb: u64, reset_mb: u64, warned: bool) -> (bool, bool) {
    if warn_mb == 0 {
        return (false, false);
    }
    if mem_mb < warn_mb {
        if warned {
            (false, true) // 已在提醒状态，不重复弹（直到恢复）
        } else {
            (true, true)
        }
    } else if mem_mb > reset_mb {
        (false, false) // 恢复区：重置
    } else {
        (false, warned) // 滞回区：保持
    }
}

/// 系统可用提交内存（MB）：GlobalMemoryStatusEx；非 Windows 返回 u64::MAX（不预警）
pub fn system_avail_commit_mb() -> u64 {
    #[cfg(windows)]
    {
        use std::mem::MaybeUninit;
        use windows_sys::Win32::System::SystemInformation::{
            GlobalMemoryStatusEx, MEMORYSTATUSEX,
        };
        let mut st: MEMORYSTATUSEX = unsafe { MaybeUninit::zeroed().assume_init() };
        st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        let ok = unsafe { GlobalMemoryStatusEx(&mut st) };
        if ok != 0 {
            return (st.ullAvailPageFile >> 20) as u64;
        }
        u64::MAX
    }
    #[cfg(not(windows))]
    {
        u64::MAX
    }
}

pub fn parse_port_from_line(line: &str) -> Option<u16> {
    if !line.contains("dsh web: http://127.0.0.1:") {
        return None;
    }
    let rest = line.trim_start_matches("dsh web: http://127.0.0.1:");
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[derive(Debug)]
pub struct Resolved {
    pub node: PathBuf,
    pub bin: PathBuf,
    pub patch: PathBuf,
    pub log: PathBuf,
}

/// 内核 lib/bin.js 定位优先级（契约 §2.2/开发兜底）：
/// settings.kernelPath → 捆绑 kernel/ → DSH_DESKTOP_KERNEL → %APPDATA%/npm/node_modules → npm root -g
/// 内核包根（<root>/lib/bin.js 的 <root>）——kernel.rs 与 updater.rs 共用
pub fn resolve_kernel_root(settings: &AppSettings, exe_dir: &Path) -> std::io::Result<PathBuf> {
    let bin = find_kernel_bin(settings, exe_dir)?;
    Ok(kernel_root_of(&bin))
}

pub fn kernel_root_of(bin: &Path) -> PathBuf {
    bin.parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
}

/// node 解析（与 resolve_paths 相同规则），供 updater 冒烟使用
pub fn resolve_node_path(settings: &AppSettings, exe_dir: &Path) -> PathBuf {
    if !settings.node_path.is_empty() {
        PathBuf::from(&settings.node_path)
    } else {
        let bundled = repo_root(exe_dir).join("runtime").join("node.exe");
        if bundled.exists() {
            bundled
        } else {
            PathBuf::from("node")
        }
    }
}

fn find_kernel_bin(settings: &AppSettings, exe_dir: &Path) -> std::io::Result<PathBuf> {
    if !settings.kernel_path.is_empty() {
        return Ok(PathBuf::from(&settings.kernel_path).join("lib").join("bin.js"));
    }
    let bundled = repo_root(exe_dir).join("kernel").join("lib").join("bin.js");
    if bundled.exists() {
        return Ok(bundled);
    }
    if let Ok(p) = std::env::var("DSH_DESKTOP_KERNEL") {
        let cand = PathBuf::from(p).join("lib").join("bin.js");
        if cand.exists() {
            return Ok(cand);
        }
        return Err(std::io::Error::other(
            "DSH_DESKTOP_KERNEL set but lib/bin.js missing",
        ));
    }
    let mut found: Option<PathBuf> = None;
    if let Ok(appdata) = std::env::var("APPDATA") {
        let cand = PathBuf::from(appdata)
            .join("npm")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh")
            .join("lib")
            .join("bin.js");
        if cand.exists() {
            found = Some(cand);
        }
    }
    if found.is_none() {
        // npm.cmd 不能直接 CreateProcess，走 cmd /C（Windows）
        if let Ok(out) = Command::new("cmd").arg("/C").arg("npm root -g").output() {
            let g = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !g.is_empty() {
                let cand =
                    PathBuf::from(g).join("@deepseek-ai").join("dsh").join("lib").join("bin.js");
                if cand.exists() {
                    found = Some(cand);
                }
            }
        }
    }
    match found {
        Some(c) => Ok(c),
        None => Err(std::io::Error::other(
            "kernel not found (bundled kernel/, DSH_DESKTOP_KERNEL, %APPDATA%/npm, npm root -g all exhausted)",
        )),
    }
}

fn resolve_paths(
    settings: &AppSettings,
    exe_dir: &Path,
    data_dir: &Path,
) -> std::io::Result<Resolved> {
    let node = resolve_node_path(settings, exe_dir);

    let bin = find_kernel_bin(settings, exe_dir)?;
    let patch = data_dir.join("desktop.patch.yml");
    let log = data_dir.join("logs").join("kernel.log");

    Ok(Resolved {
        node,
        bin,
        patch,
        log,
    })
}

/// 工程根推断：安装布局=<exe>（kernel/ desktop/ 相邻）；开发布局=<exe>/../../../（src-tauri/target/{debug,release}）
pub fn repo_root(exe_dir: &Path) -> PathBuf {
    for cand in [
        exe_dir.join("..").join("..").join(".."),
        exe_dir.join("..").join(".."),
    ] {
        if cand.join("desktop").join("quit.js").exists() {
            return cand.to_path_buf();
        }
    }
    PathBuf::from(exe_dir)
}

fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    format!("file:///{s}")
}

pub fn write_patch(
    patch_path: &Path,
    quit_js: &Path,
    health_js: &Path,
) -> std::io::Result<()> {
    let lines = vec![
        "# generated by dsh-desktop shell (do not edit)".to_string(),
        "- insert:".to_string(),
        "  - id: desktop-quit".to_string(),
        format!("    name: '{}'", file_url(quit_js)),
        "- insert:".to_string(),
        "  - id: desktop-health".to_string(),
        format!("    name: '{}'", file_url(health_js)),
    ];
    if let Some(parent) = patch_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(patch_path, lines.join("\n"))
}

/// C：安全模式最小 profile 的 package.json 内容（纯函数可单测）——
/// 与官方 web profile 的 dsh.profile.bundles 结构同构，仅官方两件套（禁第三方插件）
pub fn safe_profile_json(profile: &str) -> String {
    let v = serde_json::json!({
        "name": format!("dsh-profile-{profile}"),
        "private": true,
        "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
    });
    serde_json::to_string_pretty(&v).expect("static safe-profile json")
}

/// C：确保 <DSH_HOME>/profiles/<safe_profile>/package.json 存在（缺则生成最小 profile）。
/// 只创建 safe profile 自己的目录；已存在则原样保留（幂等）。绝不触碰 profiles/web。
pub fn ensure_safe_profile(dsh_home: &Path, profile: &str) -> std::io::Result<PathBuf> {
    let dir = dsh_home.join("profiles").join(profile);
    let pkg = dir.join("package.json");
    if !pkg.exists() {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&pkg, safe_profile_json(profile))?;
    }
    Ok(pkg)
}

pub fn spawn_kernel(
    res: &Resolved,
    settings: &AppSettings,
    port_arg: String,
    patch: Option<&std::path::Path>,
) -> std::io::Result<Child> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // C：安全模式经 --profile <name> 旗标启动最小 profile。实测（v0.2.1 probe）：
    // 位置参数 web 只是 --profile web 的别名；任意 profile 名用位置参数会报
    // "error: --profile <name> is required"，必须旗标形式；就绪行 "dsh web:" 来自
    // dsh-web-app bundle、与 profile 名无关，解析器无需变化。
    let profile: &str = if settings.safe_mode && !settings.safe_profile.is_empty() {
        settings.safe_profile.as_str()
    } else {
        "web"
    };
    let mut cmd = Command::new(&res.node);
    cmd.arg(&res.bin);
    if profile == "web" {
        cmd.arg("web");
    } else {
        cmd.arg("--profile").arg(profile);
    }
    if let Some(p) = patch {
        cmd.arg("--patch").arg(p); // ⚠ launcher flags 必须先于应用 flags（契约 §10.1）
    }
    cmd.arg("--no-open").arg("--port").arg(&port_arg).stdout(Stdio::piped());

    if let Some(parent) = res.log.parent() {
        let _ = std::fs::create_dir_all(parent);
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&res.log)?;
        cmd.stderr(Stdio::from(f));
    } else {
        cmd.stderr(Stdio::null());
    }

    // 环境：DSH_HOME 空=默认 ~/.dsh（D4 共享）；遥测默认关；NODE_OPTIONS（E）
    if !settings.dsh_home.is_empty() {
        cmd.env("DSH_HOME", &settings.dsh_home);
    }
    if settings.telemetry_disabled {
        cmd.env("DSH_TELEMETRY_DISABLED", "1");
    }
    if !settings.node_options.is_empty() {
        cmd.env("NODE_OPTIONS", &settings.node_options);
    }

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW); // 无控制台（§3.2）

    let degraded_flag = patch.is_none();
    eprintln!(
        "[kernel] spawn (degraded={degraded_flag}, profile={profile}): {} {} {}",
        res.node.display(),
        res.bin.display(),
        patch.map(|p| format!("--patch {}", p.display())).unwrap_or_else(|| "(no-patch)".into())
    );
    let child = cmd.spawn()?;

    // E：子进程优先级提升（ABOVE_NORMAL）
    if settings.boost_priority {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{
                SetPriorityClass, ABOVE_NORMAL_PRIORITY_CLASS,
            };
            let h = child.as_raw_handle();
            unsafe {
                let _ = SetPriorityClass(h, ABOVE_NORMAL_PRIORITY_CLASS);
            }
        }
    }
    Ok(child)
}

/// 启动 → 就绪 → 窗口 → 崩溃重启 → 停止（阻塞至退出）
pub fn run_shell(app: tauri::AppHandle, ctl: KernelCtl, smoke: bool, data_dir: &Path) -> i32 {
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().map(|d| d.to_path_buf()).unwrap_or_default())
        .unwrap_or_default();
    let mut exit_code = 0;
    let mut degraded_done = false;
    loop {
        if ctl.stop.load(Ordering::SeqCst) {
            break;
        }
        // B：每轮重载 settings/resolve（设置切换 Restart 即生效；更新交换每轮重取内核根）
        let settings = AppSettings::load(data_dir);
        let _ = settings.save(data_dir);
        let res = match resolve_paths(&settings, &exe_dir, data_dir) {
            Ok(r) => {
                eprintln!("[kernel] resolved node={} bin={}", r.node.display(), r.bin.display());
                r
            }
            Err(e) => {
                eprintln!("[kernel] resolve failed: {e}");
                if !smoke {
                    crate::jobobject::show_error(
                        "DSH Desktop - 内核未找到",
                        &format!(
                            "无法定位 DSH 内核（lib/bin.js）：\n{e}\n\n请在 settings.json 指定 kernelPath，或安装官方内核后重试。"
                        ),
                    );
                }
                return if smoke { 2 } else { 0 };
            }
        };
        let root = repo_root(&exe_dir);
        let quit_js = root.join("desktop").join("quit.js");
        let health_js = root.join("desktop").join("health.js");
        if let Err(e) = write_patch(&res.patch, &quit_js, &health_js) {
            eprintln!("[kernel] write_patch failed: {e}");
        }
        // C：安全模式——启动前确保最小 profile 存在（绝不修改用户 profiles/web 任何文件）
        if settings.safe_mode {
            match ensure_safe_profile(&settings.real_dsh_home(), &settings.safe_profile) {
                Ok(p) => eprintln!("[kernel] safe mode ON, profile ready: {}", p.display()),
                Err(e) => eprintln!("[kernel] safe profile prepare failed: {e}"),
            }
        }
        let degraded = ctl.degraded.load(Ordering::SeqCst);
        match launch_once(&app, &ctl, &res, &settings, smoke, degraded) {
            LaunchOutcome::SmokeFail => {
                exit_code = 2;
                break;
            }
            LaunchOutcome::Stopped => break,
            LaunchOutcome::Restart => {
                // 更新流：内核已停止，此刻执行原子替换（§5.2 步骤④）
                // P0-13：restart 标志必须先复位——否则新内核就绪后阶段2立读 restart=true，
                // 再次 graceful_stop → 无限 spawn→杀→spawn 环路（更新端到端必死）。
                ctl.restart.store(false, Ordering::SeqCst);
                let pending = ctl.update_path.lock().unwrap().take();
                if let Some(_kernel_new) = pending {
                    let root = kernel_root_of(&res.bin);
                    let newver = ctl.updated_version.lock().unwrap().clone().unwrap_or_default();
                    match crate::updater::apply_swap(&root, &newver) {
                        Ok(()) => {
                            eprintln!("[kernel] updated to {newver}, relaunching");
                        }
                        Err(e) => {
                            eprintln!("[kernel] apply_swap failed: {e}");
                            let _ = crate::updater::rollback(&root, false);
                            if !smoke {
                                crate::jobobject::show_error(
                                    "DSH Desktop - 更新失败",
                                    &format!("内核替换失败，已回滚：{e}"),
                                );
                            }
                        }
                    }
                }
                continue; // 重新 spawn（新版本文件）
            }
            LaunchOutcome::Exited => {
                // 记录崩溃并判限次（600s 滑动窗口）
                let crash_count = {
                    let mut v = ctl.crashes.lock().unwrap();
                    let now = Instant::now();
                    v.push(now);
                    v.retain(|t| t.elapsed() < Duration::from_secs(600));
                    v.len()
                };
                if ctl.stop.load(Ordering::SeqCst) {
                    break;
                }
                // 更新后崩溃循环自动回滚（§6.2：新版本 10 分钟内 ≥2 次）
                let upd_ver = ctl.updated_version.lock().unwrap().clone();
                if let Some(uv) = upd_ver {
                    let root = kernel_root_of(&res.bin);
                    let cur = crate::updater::current_version(&root).unwrap_or_default();
                    if cur == uv && crash_count >= 2 {
                        let _ = crate::updater::rollback(&root, false);
                        *ctl.updated_version.lock().unwrap() = None;
                        eprintln!("[kernel] auto-rollback from {uv}");
                        if !smoke {
                            crate::jobobject::show_error(
                                "DSH Desktop - 已自动回滚",
                                &format!("新版本 {uv} 启动后连续崩溃，已自动回滚到上一版本。"),
                            );
                        }
                        continue;
                    }
                }
                if crash_count <= MAX_AUTO_RESTART {
                    let idx = (crash_count - 1).min(BACKOFF.len() - 1);
                    eprintln!(
                        "[kernel] exited ({crash_count}/{}), auto-restart in {:?}",
                        MAX_AUTO_RESTART, BACKOFF[idx]
                    );
                    std::thread::sleep(BACKOFF[idx]);
                    continue;
                }
                eprintln!("[kernel] crashed too many times, giving up");
                if !smoke {
                    crate::set_loading_status(&app, "内核反复崩溃，已停止自动重启：请检查更新或查看日志后重试；也可从托盘开启「安全模式（禁用第三方插件）」排查。");
                    crate::jobobject::show_error(
                        "DSH Desktop - 内核反复崩溃",
                        "DSH 内核短时间内连续崩溃，已停止自动重启。\n请检查更新或查看日志后重试。\n\n若怀疑第三方插件损坏，可在托盘菜单勾选「安全模式（禁用第三方插件）」后重新启动内核排查。",
                    );
                } else {
                    exit_code = 2; // P1-3：冒烟失败必须非零退出
                }
                // D：降级兜底——尝试一次无 patch 启动（桌面插件与官方新版不兼容）；smoke 保持非零语义
                if !smoke && !degraded_done {
                    degraded_done = true;
                    ctl.degraded.store(true, Ordering::SeqCst);
                    eprintln!("[kernel] degraded mode: retry without desktop patch");
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                break;
            }
        }
    }
    eprintln!("[kernel] shell loop exit (code {exit_code})");
    exit_code
}

enum LaunchOutcome {
    Exited,
    Stopped,
    Restart,
    SmokeFail,
}

fn launch_once(
    app: &tauri::AppHandle,
    ctl: &KernelCtl,
    res: &Resolved,
    settings: &AppSettings,
    smoke: bool,
    degraded: bool,
) -> LaunchOutcome {
    // 端口策略（D2）：fixed:<p> 预检绑定，失败回退 0
    let port_arg = if let Some(fixed) = settings.port_mode.strip_prefix(FIXED_PORT_PREFIX) {
        match fixed.parse::<u16>() {
            Ok(p) => match std::net::TcpListener::bind(("127.0.0.1", p)) {
                Ok(_) => p.to_string(),
                Err(_) => "0".to_string(),
            },
            Err(_) => "0".to_string(),
        }
    } else {
        "0".to_string()
    };

    let mut child = match spawn_kernel(res, settings, port_arg, (!degraded).then_some(res.patch.as_path())) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[kernel] spawn failed: {e}");
            if smoke {
                return LaunchOutcome::SmokeFail;
            }
            crate::jobobject::show_error(
                "DSH Desktop - 内核启动失败",
                &format!("{e}\n\n请检查 node/kernel 路径配置（settings.json）。"),
            );
            crate::set_loading_status(app, "内核启动失败：请检查设置或日志后重试。");
            return LaunchOutcome::Stopped;
        }
    };

    // Job Object：KILL_ON_JOB_CLOSE（防孤儿，§3.2）
    #[cfg(windows)]
    let job = match JobObject::new().and_then(|j| j.assign_child(&child).map(|_| j)) {
        Ok(j) => Some(j),
        Err(e) => {
            eprintln!("[kernel] job object assign failed: {e}");
            None
        }
    };

    // 就绪监听：stdout 行 → Ready(port)/Line(text)
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel::<KernelEvent>();
    if let Some(out) = stdout {
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        let _ = tx.send(KernelEvent::Line(l.clone()));
                        if let Some(p) = parse_port_from_line(&l) {
                            let _ = tx.send(KernelEvent::Ready(p));
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // 阶段 1：等就绪（≤30s，期间响应停止请求/早退）
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut port: Option<u16> = None;
    loop {
        if ctl.stop.load(Ordering::SeqCst) {
            graceful_stop(&mut child, job.as_ref(), None, degraded);
            return LaunchOutcome::Stopped;
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(KernelEvent::Ready(p)) => {
                port = Some(p);
                break;
            }
            Ok(KernelEvent::Line(l)) => eprintln!("[kernel] {l}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    eprintln!("[kernel] ready timeout(30s)");
                    graceful_stop(&mut child, job.as_ref(), None, degraded);
                    if !smoke {
                        crate::set_loading_status(app, "内核启动超时（30s）：请查看日志后重试。");
                    }
                    return if smoke {
                        LaunchOutcome::SmokeFail
                    } else {
                        LaunchOutcome::Exited
                    };
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if child.try_wait().ok().flatten().is_some() {
            eprintln!("[kernel] exited before ready");
            return if smoke {
                LaunchOutcome::SmokeFail
            } else {
                LaunchOutcome::Exited
            };
        }
    }
    let port = match port {
        Some(p) => p,
        None => {
            eprintln!("[kernel] no ready port");
            return if smoke {
                LaunchOutcome::SmokeFail
            } else {
                LaunchOutcome::Exited
            };
        }
    };
    *ctl.port.lock().unwrap() = Some(port);
    eprintln!("[kernel] ready on port {port}");
    if degraded && !smoke && !ctl.degraded_warned.swap(true, Ordering::SeqCst) {
        crate::jobobject::show_error(
            "DSH Desktop - 已降级运行",
            "桌面集成插件（quit/health）加载失败，内核已降级启动：\n- 退出将使用强制结束（跳过优雅 /quit）\n- 更新功能不可用\n请检查更新或重新安装修复。",
        );
    }

    if smoke {
        // 冒烟模式：就绪 → 校验 /health → /quit → 打印 SMOKE_OK → 正常退出
        std::thread::sleep(Duration::from_millis(500));
        let health = http::http_get("/health", port, Duration::from_secs(2)).unwrap_or_default();
        let health_ok = http::is_ok(&health);
        println!("SMOKE health={health_ok} port={port}");
        graceful_stop(&mut child, job.as_ref(), Some(port), degraded);
        if !health_ok {
            // P1-3：health 失败 = 冒烟失败，非零退出
            println!("SMOKE_FAILED health={health_ok}");
            return LaunchOutcome::SmokeFail;
        }
        println!("SMOKE_OK port={port}");
        return LaunchOutcome::Stopped;
    }

    // 创建/导航窗口（主线程调度）
    let url = format!("http://127.0.0.1:{port}/");
    show_or_redirect(app, &url);

    // 阶段 2：运行中监测（停止请求 / 更新重启请求 / 崩溃检测 / F 低内存看门狗）
    let mut next_mem_check = Instant::now() + Duration::from_secs(30);
    loop {
        if ctl.stop.load(Ordering::SeqCst) {
            graceful_stop(&mut child, job.as_ref(), Some(port), degraded);
            return LaunchOutcome::Stopped;
        }
        if ctl.consume_restart() {
            eprintln!("[kernel] update restart requested");
            graceful_stop(&mut child, job.as_ref(), Some(port), degraded);
            return LaunchOutcome::Restart;
        }
        // F：低内存预警看门狗（每 30s；弹窗独立线程，不阻塞监测）
        if Instant::now() >= next_mem_check {
            next_mem_check = Instant::now() + Duration::from_secs(30);
            let mem = system_avail_commit_mb();
            let warn_mb = settings.memory_warn_mb;
            let reset_mb = if warn_mb == 0 { 0 } else { (warn_mb * 3).max(2500) };
            let (should, next) =
                memory_warn_transition(mem, warn_mb, reset_mb, ctl.mem_warned.load(Ordering::SeqCst));
            ctl.mem_warned.store(next, Ordering::SeqCst);
            if should {
                let msg = format!(
                    "系统可用内存严重不足（提交内存剩 {mem} MB），DSH 内核可能被系统终止。\n请关闭占用内存的程序或重启电脑。"
                );
                eprintln!("[kernel] LOW_MEMORY: {mem} MB");
                if !smoke {
                    let m = msg.clone();
                    std::thread::spawn(move || crate::jobobject::show_error("DSH Desktop - 内存不足", &m));
                }
            }
        }
        match child.try_wait() {
            Ok(Some(_st)) => {
                eprintln!("[kernel] exited while running");
                return LaunchOutcome::Exited;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => {
                eprintln!("[kernel] wait error: {e}");
                return LaunchOutcome::Exited;
            }
        }
    }
}

/// /quit → ≤8s 等待 → JobObject 树级硬杀（§4.3）
fn graceful_stop(child: &mut Child, job: Option<&JobObject>, port: Option<u16>, degraded: bool) {
    if let Some(p) = port {
        if !degraded {
            let _ = http::http_get("/quit", p, Duration::from_secs(2));
        }
    }
    let deadline = Instant::now() + QUIT_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if Instant::now() >= deadline {
                    eprintln!("[kernel] quit timeout, tree-kill");
                    if let Some(j) = job {
                        let _ = j.terminate();
                    } else {
                        let _ = child.kill();
                    }
                    while child.try_wait().ok().flatten().is_none() {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return,
        }
    }
}

fn show_or_redirect(app: &tauri::AppHandle, url: &str) {
    use tauri::WebviewUrl;
    let app2 = app.clone();
    let u = url.to_string();
    let (tx, rx) = mpsc::channel::<bool>();
    let _ = app.run_on_main_thread(move || {
        let ok = match app2.get_webview_window("main") {
            Some(w) => {
                let js = format!(
                    "window.location.href = {};",
                    serde_json::to_string(&u).unwrap_or_else(|_| "\"\"".into())
                );
                w.eval(&js).is_ok()
            }
            None => {
                tauri::WebviewWindowBuilder::new(
                    &app2,
                    "main",
                    WebviewUrl::External(tauri::Url::parse(&u).unwrap_or_else(|_| {
                        tauri::Url::parse("http://127.0.0.1/").unwrap()
                    })),
                )
                .title("DSH Desktop")
                .inner_size(1280.0, 820.0)
                .build()
                .is_ok()
            }
        };
        let _ = tx.send(ok);
    });
    let _ = rx.recv_timeout(Duration::from_secs(5));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_warn_hysteresis() {
        // 关闭
        assert_eq!(memory_warn_transition(100, 0, 0, false), (false, false));
        // 首次触发（<1536）
        assert_eq!(memory_warn_transition(1200, 1536, 3072, false), (true, true));
        // 已提醒再低：不重复弹
        assert_eq!(memory_warn_transition(900, 1536, 3072, true), (false, true));
        // 滞回区（1536..3072 之间）保持 true
        assert_eq!(memory_warn_transition(2000, 1536, 3072, true), (false, true));
        // 恢复区（>3072）重置
        assert_eq!(memory_warn_transition(3600, 1536, 3072, true), (false, false));
        // 恢复后再降：重新可弹
        assert_eq!(memory_warn_transition(1000, 1536, 3072, false), (true, true));
    }

    #[test]
    fn parse_ready_line() {
        assert_eq!(parse_port_from_line("dsh web: http://127.0.0.1:2085"), Some(2085));
        assert_eq!(parse_port_from_line("dsh web: http://127.0.0.1:3456 extra"), Some(3456));
        assert_eq!(parse_port_from_line("nothing here"), None);
        assert_eq!(parse_port_from_line(""), None);
        assert_eq!(parse_port_from_line("dsh web: http://127.0.0.1:"), None);
    }

    #[test]
    fn safe_profile_json_shape() {
        let s = safe_profile_json("dsh-safe");
        let v: serde_json::Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["name"], "dsh-profile-dsh-safe");
        let names: Vec<&str> = v["dsh"]["profile"]["bundles"]
            .as_array()
            .expect("bundles array")
            .iter()
            .filter_map(|b| b.as_str())
            .collect();
        assert_eq!(names, ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"]);
    }

    #[test]
    fn ensure_safe_profile_creates_and_keeps_existing() {
        let home = std::env::temp_dir().join(format!("dsh-safe-ut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let p = ensure_safe_profile(&home, "dsh-safe").expect("create");
        assert!(p.exists());
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).expect("json on disk");
        assert_eq!(v["dsh"]["profile"]["bundles"].as_array().map(|a| a.len()), Some(2));
        // 幂等：已存在不覆盖（用户手工定制的 safe profile 不被冲掉）
        std::fs::write(&p, r#"{"user":"edited"}"#).unwrap();
        let p2 = ensure_safe_profile(&home, "dsh-safe").expect("idempotent");
        assert_eq!(std::fs::read_to_string(&p2).unwrap(), r#"{"user":"edited"}"#);
        let _ = std::fs::remove_dir_all(&home);
    }
}