//! M4 版本跟随与回滚（ARCHITECTURE.md §5/§6 + captain 裁定 2026-08-29）
//! 裁定要点：更新单位 = 完整自足树（package.json + node_modules 全套，npm install 产物）。
//! 官方 npm tgz(33KB) 只含 lib/config/package.json；dsh-* 家族子包 co-release ^0.1.1-rc.2，
//! 仅替换主包永远不满足 → 准备阶段必须用捆绑 npm 对 tgz 重放安装：
//!   node <runtime>/node_modules/npm/bin/npm-cli.js install --prefix <kernel.new> <tgz>
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha512};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const REGISTRY_URL: &str = "https://registry.npmjs.org/@deepseek-ai/dsh";

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub tarball: String,
    pub integrity: String,
}

/// 读取当前内核版本：<kernel_root>/package.json → version
pub fn current_version(kernel_root: &Path) -> Option<String> {
    let p = kernel_root.join("package.json");
    let text = std::fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("version")?.as_str().map(|s| s.to_string())
}

fn fetch_registry() -> Option<Value> {
    let out = Command::new("curl").args(["-sSLf", REGISTRY_URL]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// 检查更新：None=已是最新（或网络失败）；Some(info)=可更新（容忍 -rc.x，dist-tag 直比）
pub fn check_latest(kernel_root: &Path) -> Option<UpdateInfo> {
    let cur = current_version(kernel_root)?;
    let reg = fetch_registry()?;
    let latest = reg.get("dist-tags")?.get("latest")?.as_str()?.to_string();
    if latest == cur {
        return None;
    }
    let v = reg.get("versions")?.get(&latest)?;
    let tarball = v.get("dist")?.get("tarball")?.as_str()?.to_string();
    let integrity = v.get("dist")?.get("integrity")?.as_str()?.to_string();
    Some(UpdateInfo { version: latest, tarball, integrity })
}

/// sha512 校验：npm integrity=sha512-<base64>
pub fn verify_tarball(path: &Path, integrity: &str) -> std::io::Result<()> {
    let encoded = integrity
        .strip_prefix("sha512-")
        .ok_or_else(|| std::io::Error::other("unsupported integrity algorithm"))?;
    let expected: Vec<u8> = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    if digest.as_slice() != expected.as_slice() {
        return Err(std::io::Error::other("integrity mismatch"));
    }
    Ok(())
}

/// 重放安装（裁定核心）：node <runtime>/node_modules/npm/bin/npm-cli.js install --prefix <kernel.new> <tgz>
/// 优先捆绑 npm；release 无捆绑 npm 即报错（方案 B 自包含 P0-A）；仅 debug 构建允许系统 npm 兜底。
/// 实测（npm 11）：空目录 --prefix install <tgz> → npm 将 tgz 作为根工程安装：
/// kernel.new/{package.json,lib,config,…} + kernel.new/node_modules（完整依赖树）= 与 kernel/ 同构。
/// 兜底：主包落在 node_modules/@deepseek-ai/dsh/ 则执行平铺逻辑。
pub fn materialize_dependencies(
    kernel_new: &Path,
    node: &Path,
    tgz: &Path,
) -> std::io::Result<()> {
    let npm_cli = node
        .parent()
        .map(|p| p.join("node_modules").join("npm").join("bin").join("npm-cli.js"));
    let has_bundled_npm = npm_cli.as_ref().map(|c| c.exists()).unwrap_or(false);
    let mut cmd = if has_bundled_npm {
        let mut c = Command::new(node);
        c.arg(npm_cli.unwrap());
        c
    } else if cfg!(debug_assertions) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("npm");
        c
    } else {
        return Err(std::io::Error::other(
            "内核依赖物化程序缺失：runtime/node_modules/npm 未随包安装（更新不可用）",
        ));
    };
    cmd.arg("install")
        .arg("--prefix")
        .arg(kernel_new)
        .args(["--omit=dev", "--no-audit", "--no-fund"])
        .arg(tgz);
    match cmd.output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            return Err(std::io::Error::other(format!(
                "npm install failed: {}",
                String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("")
            )));
        }
        Err(e) => return Err(e),
    }

    // 布局归一：根布局（npm 空目录语义）或平铺兜底
    if kernel_new.join("lib").join("bin.js").exists() {
        return Ok(());
    }
    let pkg = kernel_new.join("node_modules").join("@deepseek-ai").join("dsh");
    if !pkg.join("lib").join("bin.js").exists() {
        return Err(std::io::Error::other(
            "installed package missing lib/bin.js (root layout and flatten fallback both failed)",
        ));
    }
    for entry in std::fs::read_dir(&pkg)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if name_str == "node_modules" || name_str == ".package-lock.json" {
            continue;
        }
        let dst = kernel_new.join(&name);
        if dst.exists() {
            if dst.is_dir() {
                let _ = std::fs::remove_dir_all(&dst);
            } else {
                let _ = std::fs::remove_file(&dst);
            }
        }
        std::fs::rename(entry.path(), &dst)?;
    }
    let _ = std::fs::remove_dir_all(&pkg);
    Ok(())
}

/// 冒烟：node <new>/lib/bin.js web --help（隔离 DSH_HOME），退出码 0 = 通过（快速失败）
pub fn smoke_kernel(node: &Path, kernel_new: &Path) -> std::io::Result<()> {
    let home = std::env::temp_dir().join(format!("dsh-desktop-update-smoke-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&home);
    let out = Command::new(node)
        .arg(kernel_new.join("lib").join("bin.js"))
        .arg("web")
        .arg("--help")
        .env("DSH_HOME", &home)
        .env("DSH_TELEMETRY_DISABLED", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let _ = std::fs::remove_dir_all(&home);
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(std::io::Error::other(format!("smoke exit {}", o.status))),
        Err(e) => Err(e),
    }
}

// ---------- v0.2.1 A/B：镜像 home 冒烟 + 插件健康断言 ----------

/// A：更新冒烟失败签名（覆盖内核 bundle 解析失败与 Node 模块缺失的已知形态）
const FAILURE_SIGNATURES: [&str; 4] = [
    "cannot resolve profile bundle",
    "ERR_MODULE_NOT_FOUND",
    "Cannot find module",
    "failed to load bundle",
];

/// A：失败签名匹配器（纯函数可单测）。命中返回错误摘要：
/// 优先提取内核行 cannot resolve profile bundle "<插件名>" 的肇事插件名，其余签名带原始行。
pub fn match_failure_signature(line: &str) -> Option<String> {
    if !FAILURE_SIGNATURES.iter().any(|s| line.contains(s)) {
        return None;
    }
    if let Some(i) = line.find("cannot resolve profile bundle ") {
        let rest = line[i + "cannot resolve profile bundle ".len()..].trim_start_matches('"');
        if let Some(end) = rest.find('"') {
            return Some(format!("插件「{}」无法解析 | {}", &rest[..end], line.trim()));
        }
    }
    Some(line.trim().to_string())
}

/// Windows 目录联接（junction，无需管理员权限；mklink 是 cmd 内建须经 cmd /C）；
/// 非 Windows 用符号链接等价实现。
fn make_dir_link(link: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // mklink 会把正斜杠段当开关解析（C:/Users → 开关 /Users → 无效语法），
        // 而 CLI/settings 来源的 DSH_HOME 常用正斜杠——必须归一化为反斜杠（v0.2.1 反例实测）
        let link_s = link.to_string_lossy().replace('/', "\\");
        let target_s = target.to_string_lossy().replace('/', "\\");
        let out = Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&link_s)
            .arg(&target_s)
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;
        if !out.status.success() {
            return Err(std::io::Error::other(format!(
                "mklink /J failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link)
    }
}

/// A：镜像 home——把真实 <home>/profiles/web 的 package.json（+cordis.patch.yml 若存在）复制到
/// work 同路径，node_modules 做成指向真实目录的 junction（零拷贝），让更新前冒烟以用户真实
/// 插件集启动（实测 24 bundles 可正常就绪）。返回 Ok(false)=真实 profile 不存在（退回纯隔离冒烟）。
pub fn build_mirror_home(real_home: &Path, work: &Path) -> std::io::Result<bool> {
    let src = real_home.join("profiles").join("web");
    let src_pkg = src.join("package.json");
    if !src_pkg.exists() {
        return Ok(false);
    }
    let dst = work.join("profiles").join("web");
    std::fs::create_dir_all(&dst)?;
    std::fs::copy(&src_pkg, dst.join("package.json"))?;
    let src_patch = src.join("cordis.patch.yml");
    if src_patch.exists() {
        std::fs::copy(&src_patch, dst.join("cordis.patch.yml"))?;
    }
    let src_nm = src.join("node_modules");
    if src_nm.exists() {
        make_dir_link(&dst.join("node_modules"), &src_nm)?;
    }
    Ok(true)
}

/// 【安全红线】镜像 home 清理：必须先 std::fs::remove_dir 摘除 node_modules junction
/// （RemoveDirectoryW 对 reparse point 只删链接自身、绝不递归目标），再 remove_dir_all 其余树。
/// 严禁对含 junction 的树直接 remove_dir_all——一旦顺链遍历会把用户真实
/// ~/.dsh/profiles/web/node_modules（全部已装插件）整个删光。
pub fn cleanup_mirror_home(work: &Path) {
    let junction = work.join("profiles").join("web").join("node_modules");
    let _ = std::fs::remove_dir(&junction);
    let _ = std::fs::remove_dir_all(work);
}

// ---------- B³：junction 写穿防护（快照 → 冒烟 → 按快照修复） ----------
//
// 冒烟镜像用 junction 挂真实 node_modules，而内核的 profile 兜底物化
// （healProfileModuleFallback）以 profile.dir 为根写 .dsh-module-fallback 符号链接：
// DSH_HOME 指向冒烟树时，链接实体写进冒烟树、入口链接却经 junction 写进真实
// node_modules——冒烟树随后被清理，真实 profile 顶层数十条链接全部悬空（实测 24 条）。
// 对策：冒烟前递归快照（≤4 层），冒烟后按快照修复被改写的链接目标、清除指向冒烟树
// 的非快照新链接。真实目录/文件不碰。

fn smoke_path_marker(target: &str) -> bool {
    target.contains("dsh-upd-smoke") || target.contains("dsh-desktop-update-smoke")
}

fn normalize_link_target(target: &str) -> String {
    target.trim_start_matches(r"\\?\").replace('/', "\\")
}

fn snapshot_profile_tree(root: &Path, rel: &str, depth: u32, out: &mut Vec<(String, Option<String>)>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel_child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}\\{name}")
        };
        match std::fs::read_link(entry.path()) {
            Ok(target) => out.push((rel_child, Some(target.to_string_lossy().into_owned()))),
            Err(_) => {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    snapshot_profile_tree(&entry.path(), &rel_child, depth + 1, out);
                    out.push((rel_child, None));
                } else {
                    out.push((rel_child, None));
                }
            }
        }
    }
}

/// 冒烟前快照真实 profile node_modules（相对路径 → 链接目标；None = 真实目录/文件）。
fn snapshot_profile_entries(real_home: &Path) -> Vec<(String, Option<String>)> {
    let nm = real_home.join("profiles").join("web").join("node_modules");
    let mut out = Vec::new();
    snapshot_profile_tree(&nm, "", 0, &mut out);
    out
}

/// 冒烟后按快照修复：链接目标被改写（含冒烟树清理后的悬空）→ 摘除重建；
/// 快照之外新增的指向冒烟树的链接 → 直接清除。真实目录/文件一律不碰。
/// 返回修复条数（仅日志用）。
fn repair_profile_entries(real_home: &Path, snapshot: &[(String, Option<String>)]) -> usize {
    let nm = real_home.join("profiles").join("web").join("node_modules");
    let snap: std::collections::HashSet<&str> = snapshot.iter().map(|(r, _)| r.as_str()).collect();
    let mut fixed = 0usize;
    for (rel, want) in snapshot {
        let path = nm.join(rel.replace('\\', std::path::MAIN_SEPARATOR_STR));
        let Some(want) = want else {
            // 快照是真实目录/文件：仅当现场被换成指向冒烟树的链接时清除
            if let Ok(cur) = std::fs::read_link(&path) {
                let cur = cur.to_string_lossy();
                if smoke_path_marker(&cur) {
                    let _ = std::fs::remove_dir(&path);
                    let _ = std::fs::remove_file(&path);
                    fixed += 1;
                }
            }
            continue;
        };
        let want_norm = normalize_link_target(want);
        let matches = std::fs::read_link(&path)
            .map(|cur| normalize_link_target(&cur.to_string_lossy()) == want_norm)
            .unwrap_or(false);
        if !matches {
            let _ = std::fs::remove_dir(&path);
            let _ = std::fs::remove_file(&path);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if make_dir_link(&path, Path::new(&want_norm)).is_ok() {
                fixed += 1;
            }
        }
    }
    // 清除快照之外新增的冒烟树链接（heal 可能创建了快照里没有的包条目）
    let mut fresh = Vec::new();
    snapshot_profile_tree(&nm, "", 0, &mut fresh);
    for (rel, link) in fresh {
        if snap.contains(rel.as_str()) {
            continue;
        }
        if let Some(target) = link {
            if smoke_path_marker(&target) {
                let path = nm.join(rel.replace('\\', std::path::MAIN_SEPARATOR_STR));
                let _ = std::fs::remove_dir(&path);
                let _ = std::fs::remove_file(&path);
                fixed += 1;
            }
        }
    }
    fixed
}

/// B：尽力从 HTTP body 提取 JSON（Connection: close 直读 body 可能带 chunked 分块噪声——
/// 直接 parse 失败时截取首个 {/[ 到末个 }/] 的切片再试；再失败返回 None，按规格不算错误）
pub fn extract_json(body: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(body.trim()) {
        return Some(v);
    }
    let start = body.find(['{', '['])?;
    let end = body.rfind(['}', ']'])?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&body[start..=end]).ok()
}

/// B：递归扫描 plugin-manager list JSON，收集 exists/resolved 为 false 的条目名。
/// 形状鲁棒：不假设顶层结构，凡对象含 exists=false 或 resolved=false 即取其 name/id/package/bundle。
pub fn scan_unresolved_plugins(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| scan_unresolved_plugins(x, out)),
        Value::Object(m) => {
            let bad = ["exists", "resolved"]
                .iter()
                .any(|k| m.get(*k).and_then(|x| x.as_bool()) == Some(false));
            if bad {
                let name = ["name", "id", "package", "bundle"]
                    .iter()
                    .find_map(|k| m.get(*k).and_then(|x| x.as_str()))
                    .unwrap_or("<unnamed>");
                out.push(name.to_string());
            }
            m.values().for_each(|x| scan_unresolved_plugins(x, out));
        }
        _ => {}
    }
}

/// B：健康断言——就绪后逐条 GET（5s 超时）须 2xx；/plugin-manager/api/list 附加未解析条目检查
fn assert_health_routes(port: u16, routes: &[String]) -> std::io::Result<()> {
    for route in routes {
        if !crate::settings::valid_health_route(route) {
            eprintln!("[updater] skip invalid health route: {route:?}");
            continue;
        }
        let resp = crate::http::http_get(route, port, Duration::from_secs(5)).unwrap_or_default();
        if !crate::http::is_2xx(&resp) {
            return Err(std::io::Error::other(format!(
                "mirror smoke: 健康断言失败——GET {route} 非 2xx（{}）",
                crate::http::status_code(&resp)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "无响应".into())
            )));
        }
        if route == "/plugin-manager/api/list" {
            if let Some(json) = crate::http::body_of(&resp).and_then(extract_json) {
                let mut bad = Vec::new();
                scan_unresolved_plugins(&json, &mut bad);
                if !bad.is_empty() {
                    return Err(std::io::Error::other(format!(
                        "mirror smoke: 插件清单存在未解析项（exists/resolved=false）：{}",
                        bad.join(", ")
                    )));
                }
            } // 解析失败按规格不算错（best-effort）
        }
    }
    Ok(())
}

/// D+A+B 破坏性更新保护（v0.2.1 加固）：镜像真实 profile 的完整冒烟。
/// 镜像 home（真实 package.json/cordis.patch.yml + node_modules junction）→
/// spawn web --patch <desktop> --no-open --port 0（stdout+stderr 双捕获）→ 45s 内就绪且
/// 无失败签名 → GET /health 200 → B 健康断言（health_routes 全 2xx + 未解析插件检查）→
/// /quit 优雅退出。任一步失败 = 新内核会破坏用户现有插件（调用方拒绝换版并清理 kernel.new）。
/// 真实 profile 不存在时退回 v0.2 纯隔离语义（仅桌面 patch 插件，跳过 B 断言）。
pub fn smoke_kernel_with_patch(
    node: &Path,
    kernel_new: &Path,
    exe_dir: &Path,
    settings: &crate::settings::AppSettings,
) -> std::io::Result<()> {
    let root = crate::kernel::repo_root(exe_dir);
    let quit_js = root.join("desktop").join("quit.js");
    let health_js = root.join("desktop").join("health.js");
    if !quit_js.exists() || !health_js.exists() {
        return Err(std::io::Error::other("desktop/quit.js|health.js missing"));
    }
    let work = std::env::temp_dir().join(format!("dsh-upd-smoke-{}", std::process::id()));
    if work.exists() {
        // 上轮残留同名目录也必须按安全序清理（可能含 junction）
        cleanup_mirror_home(&work);
    }
    std::fs::create_dir_all(&work)?;
    // B³：镜像 junction 会让内核的兜底物化写穿到真实 profile（冒烟树清理后链接
    // 全部悬空）——冒烟前递归快照，冒烟后按快照修复
    let snapshot = snapshot_profile_entries(&settings.real_dsh_home());
    let result = mirror_smoke_run(node, kernel_new, &quit_js, &health_js, &work, settings);
    // 【安全红线】统一走 cleanup_mirror_home：先 remove_dir 摘 junction，再删树
    cleanup_mirror_home(&work);
    let fixed = repair_profile_entries(&settings.real_dsh_home(), &snapshot);
    if fixed > 0 {
        eprintln!("[updater] smoke 后按快照修复真实 profile 链接 {fixed} 条");
    }
    result
}

fn mirror_smoke_run(
    node: &Path,
    kernel_new: &Path,
    quit_js: &Path,
    health_js: &Path,
    work: &Path,
    settings: &crate::settings::AppSettings,
) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

    let patch = work.join("smoke.patch.yml");
    crate::kernel::write_patch(&patch, quit_js, health_js)?;
    let mirrored = build_mirror_home(&settings.real_dsh_home(), work)?;

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new(node);
    cmd.arg(kernel_new.join("lib").join("bin.js"))
        .arg("web")
        .arg("--patch")
        .arg(&patch) // launcher flags 必须先于应用 flags（契约 §10.1）
        .arg("--no-open")
        .arg("--port")
        .arg("0")
        .env("DSH_HOME", work)
        .env("DSH_TELEMETRY_DISABLED", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn()?;
    // 树级 Job（KILL_ON_JOB_CLOSE）：kill 只杀直接子进程，真实插件集可能派生孙进程——
    // Job 随 _job 析构自动收树，防冒烟残留孤儿
    #[cfg(windows)]
    let _job = crate::jobobject::JobObject::new()
        .and_then(|j| j.assign_child(&child).map(|_| j))
        .ok();

    enum Ev {
        Ready(u16),
        Fatal(String),
        Line(String),
    }
    let (tx, rx) = mpsc::channel::<Ev>();
    let spawn_reader = |out: Box<dyn std::io::Read + Send>, tx2: mpsc::Sender<Ev>| {
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                if let Some(msg) = match_failure_signature(&line) {
                    let _ = tx2.send(Ev::Fatal(msg));
                } else if let Some(p) = crate::kernel::parse_port_from_line(&line) {
                    let _ = tx2.send(Ev::Ready(p));
                } else {
                    let _ = tx2.send(Ev::Line(line));
                }
            }
        });
    };
    if let Some(out) = child.stdout.take() {
        spawn_reader(Box::new(out), tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(Box::new(err), tx.clone());
    }
    drop(tx);

    // 就绪等待 45s（镜像模式带真实 24 插件比空 home 慢）；失败签名/早退/超时 → Err（带肇事行）
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let push_tail = |t: &mut std::collections::VecDeque<String>, l: String| {
        if t.len() >= 20 {
            t.pop_front();
        }
        t.push_back(l);
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    let port: u16 = loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ev::Ready(p)) => break p,
            Ok(Ev::Fatal(msg)) => {
                let _ = child.kill();
                // Fatal 择优（反例实测）：首个签名行可能是 Node 栈帧回显的源码模板行
                // （throw new Error(`…cannot resolve profile bundle ${JSON.stringify(packageName)}…`)，
                // 不含实际插件名）；短暂排干后续输出，优先取提取到插件名（「插件「」标记）的渲染行。
                let mut best = msg;
                if !best.contains("插件「") {
                    let until = std::time::Instant::now() + Duration::from_millis(800);
                    while std::time::Instant::now() < until {
                        match rx.try_recv() {
                            Ok(Ev::Fatal(m)) => {
                                if m.contains("插件「") {
                                    best = m;
                                    break;
                                }
                            }
                            Ok(_) => {}
                            Err(_) => std::thread::sleep(Duration::from_millis(40)),
                        }
                    }
                }
                return Err(std::io::Error::other(format!("mirror smoke: {best}")));
            }
            Ok(Ev::Line(l)) => push_tail(&mut tail, l),
            Err(_) => {}
        }
        if let Ok(Some(st)) = child.try_wait() {
            // 早退：给读线程 500ms 排干缓冲，Fatal 择优（优先带插件名的渲染行，栈帧模板行兜底）
            let drain_until = std::time::Instant::now() + Duration::from_millis(500);
            let mut first_fatal: Option<String> = None;
            while std::time::Instant::now() < drain_until {
                match rx.try_recv() {
                    Ok(Ev::Fatal(msg)) => {
                        if msg.contains("插件「") {
                            return Err(std::io::Error::other(format!("mirror smoke: {msg}")));
                        }
                        first_fatal.get_or_insert(msg);
                    }
                    Ok(Ev::Line(l)) => push_tail(&mut tail, l),
                    Ok(Ev::Ready(_)) => {}
                    Err(_) => std::thread::sleep(Duration::from_millis(30)),
                }
            }
            if let Some(msg) = first_fatal {
                return Err(std::io::Error::other(format!("mirror smoke: {msg}")));
            }
            return Err(std::io::Error::other(format!(
                "mirror smoke: kernel exited before ready ({st}); last output: {}",
                tail.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join(" | ")
            )));
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            return Err(std::io::Error::other(format!(
                "mirror smoke: ready timeout (45s); last output: {}",
                tail.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join(" | ")
            )));
        }
    };

    // 桌面集成自检（v0.2 语义保留）：/health 必须 200
    let health = crate::http::http_get("/health", port, Duration::from_secs(3)).unwrap_or_default();
    if !crate::http::is_ok(&health) {
        let _ = child.kill();
        return Err(std::io::Error::other("mirror smoke: /health not 200"));
    }

    // B：插件健康断言（仅镜像模式——隔离 home 没有用户插件，断言无意义）；空数组=跳过
    if mirrored {
        if let Err(e) = assert_health_routes(port, &settings.health_routes) {
            let _ = child.kill();
            return Err(e);
        }
    }

    let _ = crate::http::http_get("/quit", port, Duration::from_secs(3));
    // 正例实测：真实插件集（24 bundles）析构可超过 10s——冒烟目标（插件可加载+健康断言）
    // 此刻已全部达成，优雅退出超时不判失败：20s 宽限后树杀兜底并告警。
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    eprintln!("[updater] mirror smoke: quit timeout (20s), tree-kill fallback");
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
    Ok(())
}

/// 递归复制目录（config/ 回填用；caller 保证 src/dst 均存在且 dst 不存在）。
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Windows 上内核进程树刚退出的短窗口内，杀毒/索引/插件派生的工作进程仍可能
/// 短暂持有 kernel/ 目录句柄，rename 报 os error 5（拒绝访问）——重试消化瞬态锁，
/// 不再让一次可自愈的争用演变成「换版失败 + 回滚失败 + 三目录全丢」。
fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    const ATTEMPTS: usize = 8;
    const WAIT_MS: u64 = 400;
    let mut last: Option<std::io::Error> = None;
    for attempt in 0..ATTEMPTS {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt + 1 < ATTEMPTS => {
                eprintln!(
                    "[updater] rename 拒绝访问（第 {} 次），{}ms 后重试: {} -> {}",
                    attempt + 1,
                    WAIT_MS,
                    from.display(),
                    to.display()
                );
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(WAIT_MS));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

/// 原子替换：kernel → kernel.old，kernel.new → kernel；验证 lib/bin.js+version；失败自动回退
pub fn apply_swap(kernel_root: &Path, new_version: &str) -> std::io::Result<()> {
    let install = kernel_root.parent().unwrap_or(Path::new("."));
    let kernel_new = install.join("kernel.new");
    let kernel_old = install.join("kernel.old");
    let mut swapped = false;
    if kernel_root.exists() {
        if kernel_old.exists() {
            let _ = std::fs::remove_dir_all(&kernel_old);
        }
        rename_retry(kernel_root, &kernel_old)?;
        swapped = true;
    }
    match std::fs::rename(&kernel_new, kernel_root) {
        Ok(()) => {}
        Err(e) => {
            if swapped {
                if let Err(re) = rename_retry(&kernel_old, kernel_root) {
                    // 回滚失败绝不能静默——三目录全丢的事故源头
                    eprintln!("[updater] 回滚 rename 也失败: {re}");
                    return Err(std::io::Error::other(format!(
                        "swap failed: {e}; rollback failed: {re} — 旧版完整保留在 kernel.old，请手动将 kernel.old 改名为 kernel"
                    )));
                }
            }
            return Err(e);
        }
    }
    // 官方 0.1.2 起 npm 包不再携带 config/（agent-presets 模板），换版后从上一版
    // 内核回填，避免升级丢配置；回填失败不阻断换版（config 可由首次启动重建）。
    let config_dir = kernel_root.join("config");
    if !config_dir.exists() {
        let old_config = kernel_old.join("config");
        if old_config.exists() {
            if let Err(e) = copy_dir_all(&old_config, &config_dir) {
                eprintln!("[updater] config backfill failed (non-fatal): {e}");
            }
        }
    }
    let ok = kernel_root.join("lib").join("bin.js").exists()
        && current_version(kernel_root).as_deref() == Some(new_version);
    if !ok {
        if let Err(re) = rollback(kernel_root, true) {
            eprintln!("[updater] post-swap 验证失败的回滚也失败: {re}");
        }
        return Err(std::io::Error::other("post-swap verification failed"));
    }
    Ok(())
}

/// 回滚：kernel → kernel.bad；kernel.old → kernel；keep_bad=false 时清理 kernel.bad
pub fn rollback(kernel_root: &Path, keep_bad: bool) -> std::io::Result<()> {
    let install = kernel_root.parent().unwrap_or(Path::new("."));
    let kernel_bad = install.join("kernel.bad");
    let kernel_old = install.join("kernel.old");
    let _ = std::fs::remove_dir_all(&kernel_bad);
    if kernel_root.exists() {
        rename_retry(kernel_root, &kernel_bad)?;
    }
    if kernel_old.exists() {
        rename_retry(&kernel_old, kernel_root)?;
    }
    if !keep_bad {
        let _ = std::fs::remove_dir_all(&kernel_bad);
    }
    Ok(())
}

/// 完整准备阶段（不停机）：下载 → sha512 校验 → 捆绑 npm 重放安装（完整树）→ --help 冒烟
/// → D 带 patch 冒烟 → 全部通过才可交换
pub fn prepare_new_kernel(
    info: &UpdateInfo,
    node: &Path,
    kernel_root: &Path,
    exe_dir: &Path,
    settings: &crate::settings::AppSettings,
) -> std::io::Result<PathBuf> {
    let install = kernel_root.parent().unwrap_or(Path::new("."));
    let tgz = install.join(".update_download.tgz");
    let out = Command::new("curl")
        .args(["-sSLf", "-o"])
        .arg(&tgz)
        .arg(&info.tarball)
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            return Err(std::io::Error::other(format!(
                "download failed: {}",
                String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("")
            )));
        }
        Err(e) => return Err(e),
    }
    verify_tarball(&tgz, &info.integrity)?;

    let kernel_new = install.join("kernel.new");
    let _ = std::fs::remove_dir_all(&kernel_new);
    materialize_dependencies(&kernel_new, node, &tgz)?;
    let _ = std::fs::remove_file(&tgz);

    smoke_kernel(node, &kernel_new)?;
    // D+A/B：镜像 home 冒烟（真实插件集+健康断言）——不兼容则宁可不换版本
    if let Err(e) = smoke_kernel_with_patch(node, &kernel_new, exe_dir, settings) {
        let _ = std::fs::remove_dir_all(&kernel_new);
        return Err(std::io::Error::other(format!(
            "官方新版本与现有插件/桌面集成不兼容，已保留当前版本（{e}）"
        )));
    }
    Ok(kernel_new)
}

/// 更新流程入口：检查 → 用户确认 → 准备（下载/校验/重放安装/冒烟）→ 返回 (结果文案, 新内核)
pub fn run_update_flow(
    kernel_root: &Path,
    node: &Path,
    exe_dir: &Path,
    settings: &crate::settings::AppSettings,
    on_question: impl FnOnce(&str) -> bool,
) -> (String, Option<(PathBuf, String)>) {
    match check_latest(kernel_root) {
        None => ("已是最新版本".to_string(), None),
        Some(info) => {
            let cur = current_version(kernel_root).unwrap_or_default();
            let ask = format!(
                "发现新版本：{}（当前 {}）\n是否现在下载并安装？",
                info.version, cur
            );
            if !on_question(&ask) {
                return ("已取消".to_string(), None);
            }
            match prepare_new_kernel(&info, node, kernel_root, exe_dir, settings) {
                Ok(knew) => (
                    format!("新版本 {} 已准备，正在重启内核…", info.version),
                    Some((knew, info.version)),
                ),
                Err(e) => (format!("更新失败：{e}"), None),
            }
        }
    }
}

/// 仅测试用：解析 npm integrity 串
#[allow(dead_code)]
pub fn parse_integrity(integrity: &str) -> Option<Vec<u8>> {
    let encoded = integrity.strip_prefix("sha512-")?;
    base64::engine::general_purpose::STANDARD.decode(encoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_roundtrip() {
        let b = b"hello-world-bytes";
        let enc = base64::engine::general_purpose::STANDARD.encode(b);
        let s = format!("sha512-{enc}");
        assert_eq!(parse_integrity(&s).as_deref(), Some(b.as_slice()));
        assert_eq!(parse_integrity("sha1-abc"), None);
    }

    #[test]
    fn check_latest_none_on_network_fail() {
        let r = check_latest(Path::new("X:/nonexistent-kernel"));
        assert!(r.is_none());
    }

    #[test]
    fn failure_signature_matcher() {
        // 内核实测原文形态：肇事插件名必须被提取进错误摘要
        let l = r#"dsh: cannot resolve profile bundle "@x/y" from the dsh installation or C:/Users/u/.dsh/profiles/web; run 'dsh plugin --profile web install' to fetch it"#;
        let m = match_failure_signature(l).expect("must match");
        assert!(m.contains("@x/y"), "must name the culprit bundle: {m}");
        assert!(m.contains("cannot resolve profile bundle"), "must keep raw line: {m}");
        // 其余三类签名
        assert!(match_failure_signature("Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'foo'").is_some());
        assert!(match_failure_signature("Error: Cannot find module 'bar'").is_some());
        assert!(match_failure_signature("plugin host: failed to load bundle xyz").is_some());
        // 阴性：就绪行与普通日志不得误报
        assert!(match_failure_signature("dsh web: http://127.0.0.1:8369").is_none());
        assert!(match_failure_signature("normal log line").is_none());
    }

    #[test]
    fn unresolved_plugin_scan() {
        let j: Value = serde_json::from_str(
            r#"{"plugins":[{"name":"a","exists":true,"resolved":true},{"name":"bad-one","exists":false},{"id":"bad-two","resolved":false},{"name":"c"}]}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        scan_unresolved_plugins(&j, &mut out);
        assert_eq!(out, vec!["bad-one".to_string(), "bad-two".to_string()]);
    }

    #[test]
    fn json_extraction_best_effort() {
        assert!(extract_json(r#"{"a":1}"#).is_some());
        // chunked 噪声：前后杂质仍可提取
        assert!(extract_json(r#"7f garbage {"a":[1,2]} trailing 0"#).is_some());
        assert!(extract_json("plain text").is_none());
    }

    #[test]
    fn mirror_home_build_and_safe_cleanup() {
        let base = std::env::temp_dir().join(format!("dsh-mirror-ut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real");
        let work = base.join("work");
        let real_nm = real
            .join("profiles")
            .join("web")
            .join("node_modules")
            .join("@scope")
            .join("pkg");
        std::fs::create_dir_all(&real_nm).unwrap();
        std::fs::write(real_nm.join("index.js"), "ok").unwrap();
        std::fs::write(real.join("profiles").join("web").join("package.json"), "{}").unwrap();
        std::fs::write(real.join("profiles").join("web").join("cordis.patch.yml"), "x: 1").unwrap();
        std::fs::create_dir_all(&work).unwrap();

        assert!(build_mirror_home(&real, &work).unwrap());
        let wnm = work.join("profiles").join("web").join("node_modules");
        // junction 生效：透过链接可见真实文件（核验路径必须带完整 @scope 段）
        assert!(wnm.join("@scope").join("pkg").join("index.js").exists());
        assert!(work.join("profiles").join("web").join("package.json").exists());
        assert!(work.join("profiles").join("web").join("cordis.patch.yml").exists());

        cleanup_mirror_home(&work);
        assert!(!work.exists(), "work tree must be fully removed");
        // 【安全红线验证】真实 node_modules 必须毫发无损
        assert!(real_nm.join("index.js").exists(), "real node_modules must survive cleanup");

        // 回归：正斜杠形态的 real_home（CLI/settings 来源）也必须能建 junction
        // （mklink 把 /x 段当开关解析，未归一化时报「无效语法」）
        let real_fwd = PathBuf::from(real.to_string_lossy().replace('\\', "/"));
        let work_fwd = base.join("work-fwd");
        std::fs::create_dir_all(&work_fwd).unwrap();
        assert!(build_mirror_home(&real_fwd, &work_fwd).unwrap());
        assert!(work_fwd
            .join("profiles").join("web").join("node_modules")
            .join("@scope").join("pkg").join("index.js")
            .exists());
        cleanup_mirror_home(&work_fwd);
        assert!(real_nm.join("index.js").exists(), "real node_modules must survive fwd-slash cleanup");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn mirror_home_absent_profile_falls_back() {
        let base = std::env::temp_dir().join(format!("dsh-mirror-ut2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real-empty");
        let work = base.join("work2");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        assert!(!build_mirror_home(&real, &work).unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn repair_profile_entries_restores_smoke_rewrites() {
        let base = std::env::temp_dir().join(format!("dsh-repair-ut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real");
        let nm = real.join("profiles").join("web").join("node_modules");
        // 顶层的真实目录 + 链接；@scope 内的链接（heal 写穿的典型深度）
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(nm.join("real-dir")).unwrap();
        make_dir_link(&nm.join("linked"), &elsewhere).unwrap();
        let scoped_parent = nm.join("@scope").join("pkg-a");
        std::fs::create_dir_all(&scoped_parent).unwrap();
        let scoped_target = base.join("scoped-target");
        std::fs::create_dir_all(&scoped_target).unwrap();
        make_dir_link(&scoped_parent.join("dep"), &scoped_target).unwrap();

        let snapshot = snapshot_profile_entries(&real);
        assert_eq!(snapshot.len(), 5, "real-dir/linked/@scope/pkg-a/dep + 中间目录");

        // 模拟 heal 写穿：两条链接改指冒烟树（随后冒烟树被清 → 悬空）
        let smoke_fb = base.join("dsh-upd-smoke-x").join("fb");
        std::fs::create_dir_all(&smoke_fb).unwrap();
        std::fs::remove_dir(nm.join("linked")).unwrap();
        make_dir_link(&nm.join("linked"), &smoke_fb).unwrap();
        std::fs::remove_dir(scoped_parent.join("dep")).unwrap();
        make_dir_link(&scoped_parent.join("dep"), &smoke_fb).unwrap();
        // heal 新建的快照外条目也指向冒烟树
        make_dir_link(&nm.join("fresh-pkg"), &smoke_fb).unwrap();

        let fixed = repair_profile_entries(&real, &snapshot);
        assert_eq!(fixed, 3, "两条改写 + 一条快照外新增");
        let linked_now = normalize_link_target(
            &std::fs::read_link(nm.join("linked")).unwrap().to_string_lossy(),
        );
        assert!(linked_now.ends_with("elsewhere"), "linked 恢复为原目标: {linked_now}");
        let dep_now = normalize_link_target(
            &std::fs::read_link(scoped_parent.join("dep")).unwrap().to_string_lossy(),
        );
        assert!(dep_now.ends_with("scoped-target"), "dep 恢复为原目标: {dep_now}");
        assert!(!nm.join("fresh-pkg").exists(), "快照外冒烟链接被清除");
        assert!(nm.join("real-dir").is_dir(), "真实目录不碰");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rename_retry_moves_directory() {
        let base = std::env::temp_dir().join(format!("dsh-rename-ut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("from").join("lib")).unwrap();
        std::fs::write(base.join("from").join("lib").join("bin.js"), "x").unwrap();
        rename_retry(&base.join("from"), &base.join("to")).unwrap();
        assert!(!base.join("from").exists());
        assert!(base.join("to").join("lib").join("bin.js").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
