//! M4 版本跟随与回滚（ARCHITECTURE.md §5/§6）
//! 流程：检查(Registry dist-tags) → 下载 tgz(sha512 integrity 校验) → 解压 kernel.new →
//!       --help 冒烟(隔离 DSH_HOME) → (内核停止后) rename 原子替换 → 重启；
//!       失败/崩溃循环 → kernel.old 回滚。安装包捆绑 ≠ 更新机制变化。
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha512};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let out = Command::new("curl")
        .args(["-sSLf", REGISTRY_URL])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// 检查更新：None=已是最新（或网络失败）；Some(info)=可更新
pub fn check_latest(kernel_root: &Path) -> Option<UpdateInfo> {
    let cur = current_version(kernel_root)?;
    let reg = fetch_registry()?;
    let latest = reg
        .get("dist-tags")?
        .get("latest")?
        .as_str()?
        .to_string();
    if latest == cur {
        return None;
    }
    // 容忍 -rc.x：dist-tag 直出版本，字符串不一致即视为有新版本（ARCHITECTURE §5.1）
    let v = reg.get("versions")?.get(&latest)?;
    let tarball = v.get("dist")?.get("tarball")?.as_str()?.to_string();
    let integrity = v.get("dist")?.get("integrity")?.as_str()?.to_string();
    Some(UpdateInfo {
        version: latest,
        tarball,
        integrity,
    })
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

/// 解压 tgz → <install>/.update_work/package → 平铺为 <install>/kernel.new
pub fn extract_tarball(tgz: &Path, kernel_new: &Path) -> std::io::Result<()> {
    let install = kernel_new.parent().unwrap_or(Path::new("."));
    let work = install.join(".update_work");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let file = std::fs::File::open(tgz)?;
    let giz = flate2::read::GzDecoder::new(file);
    let mut ar = tar::Archive::new(giz);
    ar.unpack(&work)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let pkg = work.join("package");
    if !pkg.join("lib").join("bin.js").exists() {
        return Err(std::io::Error::other("tarball package/lib/bin.js missing"));
    }
    let _ = std::fs::remove_dir_all(kernel_new);
    std::fs::rename(&pkg, kernel_new)?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

/// 物化依赖树：npm tgz 只含 lib/config/package.json（约 33KB），可运行完整树 = 包 + node_modules
/// （npm install 产物，含 dsh-* 家族 co-release 子包与 win32-x64 prebuild，约 250MB）。
/// 优先使用捆绑 runtime/npm-cli.js（node <runtime>/npm-cli.js install）；无则 cmd /C npm。
pub fn materialize_dependencies(kernel_new: &Path, node: &Path) -> std::io::Result<()> {
    let npm_cli = node.parent().map(|p| p.join("npm-cli.js"));
    let mut cmd = if let Some(cli) = npm_cli {
        if cli.exists() {
            let mut c = Command::new(node);
            c.arg(&cli);
            c
        } else {
            let mut c = Command::new("cmd");
            c.arg("/C").arg("npm");
            c
        }
    } else {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("npm");
        c
    };
    cmd.arg("install")
        .args(["--omit=dev", "--no-audit", "--no-fund"])
        .current_dir(kernel_new);
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(std::io::Error::other(format!(
            "npm install failed: {}",
            String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("")
        ))),
        Err(e) => Err(e),
    }
}

/// 冒烟：node <new>/lib/bin.js web --help（隔离 DSH_HOME），退出码 0 = 通过
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

/// 完整准备阶段（不停机）：下载 → 校验 → 解压 kernel.new → 冒烟
pub fn prepare_new_kernel(
    info: &UpdateInfo,
    node: &Path,
    kernel_root: &Path,
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
    extract_tarball(&tgz, &kernel_new)?;
    let _ = std::fs::remove_file(&tgz);

    // ⚠ npm 包 tgz 不含 node_modules：必须先物化依赖树（~250MB，联网 npm install）
    materialize_dependencies(&kernel_new, node)?;

    smoke_kernel(node, &kernel_new)?;
    Ok(kernel_new)
}

/// 更新流程入口：检查 → 用户确认 → 准备（下载/校验/解压/冒烟）→ 返回 (结果文案, 已准备的新内核)
pub fn run_update_flow(
    kernel_root: &Path,
    node: &Path,
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
            match prepare_new_kernel(&info, node, kernel_root) {
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