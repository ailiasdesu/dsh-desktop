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
use std::process::Command;
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

/// D 破坏性更新保护：带桌面 patch 插件的完整冒烟。
/// 临时 DSH_HOME + 临时 patch（指向现装 desktop/quit.js、health.js）→ spawn web --patch …
/// → 30s 内解析就绪行 → GET /health 期望 200 → GET /quit → 等退出（≤10s）。
/// 任一步失败 = 官方新版本与桌面集成插件不兼容（调用方清理 kernel.new 并提示回滚）。
pub fn smoke_kernel_with_patch(
    node: &Path,
    kernel_new: &Path,
    exe_dir: &Path,
) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

    let root = crate::kernel::repo_root(exe_dir);
    let quit_js = root.join("desktop").join("quit.js");
    let health_js = root.join("desktop").join("health.js");
    if !quit_js.exists() || !health_js.exists() {
        return Err(std::io::Error::other("desktop/quit.js|health.js missing"));
    }
    let work = std::env::temp_dir().join(format!("dsh-upd-smoke-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    let patch = work.join("smoke.patch.yml");
    crate::kernel::write_patch(&patch, &quit_js, &health_js)?;

    let smoke_settings = crate::settings::AppSettings {
        dsh_home: work.display().to_string(),
        telemetry_disabled: true,
        node_options: String::new(),
        ..Default::default()
    };
    let res = crate::kernel::Resolved {
        node: node.to_path_buf(),
        bin: kernel_new.join("lib").join("bin.js"),
        patch: patch.clone(),
        log: work.join("kernel.log"),
    };
    let mut child = crate::kernel::spawn_kernel(&res, &smoke_settings, "0".to_string(), Some(&patch))?;

    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel::<u16>();
    if let Some(out) = stdout {
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines() {
                if let Ok(l) = line {
                    if let Some(p) = crate::kernel::parse_port_from_line(&l) {
                        let _ = tx.send(p);
                        break;
                    }
                }
            }
        });
    }
    let port = match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(p) => p,
        Err(_) => {
            let _ = child.kill();
            let _ = std::fs::remove_dir_all(&work);
            return Err(std::io::Error::other("with-patch smoke: ready timeout"));
        }
    };
    let health = crate::http::http_get("/health", port, Duration::from_secs(3)).unwrap_or_default();
    if !crate::http::is_ok(&health) {
        let _ = child.kill();
        let _ = std::fs::remove_dir_all(&work);
        return Err(std::io::Error::other("with-patch smoke: /health not 200"));
    }
    let _ = crate::http::http_get("/quit", port, Duration::from_secs(3));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = std::fs::remove_dir_all(&work);
                    return Err(std::io::Error::other("with-patch smoke: quit timeout"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
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
        std::fs::rename(kernel_root, &kernel_old)?;
        swapped = true;
    }
    match std::fs::rename(&kernel_new, kernel_root) {
        Ok(()) => {}
        Err(e) => {
            if swapped {
                let _ = std::fs::rename(&kernel_old, kernel_root);
            }
            return Err(e);
        }
    }
    let ok = kernel_root.join("lib").join("bin.js").exists()
        && current_version(kernel_root).as_deref() == Some(new_version);
    if !ok {
        let _ = rollback(kernel_root, true);
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
        std::fs::rename(kernel_root, &kernel_bad)?;
    }
    if kernel_old.exists() {
        std::fs::rename(&kernel_old, kernel_root)?;
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
    // D：带桌面插件冒烟——不兼容则宁可不换版本
    if let Err(e) = smoke_kernel_with_patch(node, &kernel_new, exe_dir) {
        let _ = std::fs::remove_dir_all(&kernel_new);
        return Err(std::io::Error::other(format!(
            "官方新版本与桌面集成插件不兼容，已保留当前版本（{e}）"
        )));
    }
    Ok(kernel_new)
}

/// 更新流程入口：检查 → 用户确认 → 准备（下载/校验/重放安装/冒烟）→ 返回 (结果文案, 新内核)
pub fn run_update_flow(
    kernel_root: &Path,
    node: &Path,
    exe_dir: &Path,
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
            match prepare_new_kernel(&info, node, kernel_root, exe_dir) {
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
}
