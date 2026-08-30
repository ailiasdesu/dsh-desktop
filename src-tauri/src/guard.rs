//! 并发守卫（A）：检测「另一个 DSH 内核」正在共享 ~/.dsh 运行（双实例=写坏会话日志）。
//! 检测 = 枚举 node.exe 进程命令行，命中 @deepseek-ai/dsh 族内核、
//! 且排除本安装内核（自身进程树由 Job Object 管理、命令行含本内核路径）。
#![cfg(windows)]
use std::process::Command;

/// wmic CSV 行解析（列序实测：Node,CommandLine,ProcessId —— PID 在最后）。
/// CommandLine 字段整体被引号包裹、内含引号与逗号：取「第一个逗号后、最后一个逗号前」为主字段。
pub fn parse_wmic_line(line: &str, self_kernel_marker: &str) -> Option<String> {
    let first = line.find(',')?;
    let last = line.rfind(',')?;
    if last <= first {
        return None;
    }
    let mut cmd = line[first + 1..last].to_string();
    if cmd.starts_with('"') && cmd.ends_with('"') && cmd.len() >= 2 {
        cmd = cmd[1..cmd.len() - 1].to_string();
    }
    let pid = line[last + 1..].trim().trim_matches('"').to_string();
    if pid.is_empty() || pid.eq_ignore_ascii_case("processid") {
        return None;
    }
    let cmd_upper = cmd.to_uppercase();
    if !is_dsh_cmdline(&cmd_upper) {
        return None;
    }
    if !self_kernel_marker.is_empty() && cmd_upper.contains(&self_kernel_marker.to_uppercase()) {
        return None; // 自身安装内核
    }
    let summary = if cmd.len() > 120 {
        format!("{}…", floor_char_boundary(&cmd, 117))
    } else {
        cmd
    };
    Some(format!("pid={pid} {summary}"))
}

/// 命中判定：@deepseek-ai/dsh 内核（正/反斜杠）或 dsh\lib\bin.js
fn is_dsh_cmdline(u: &str) -> bool {
    u.contains("@DEEPSEEK-AI\\DSH")
        || u.contains("@DEEPSEEK-AI/DSH")
        || u.contains("DSH\\LIB\\BIN.JS")
        || u.contains("DSH/LIB/BIN.JS")
}

/// 检测外部 DSH 内核：返回命中命令行摘要
pub fn detect_foreign_kernel(self_kernel_dir: &str) -> Vec<String> {
    let marker = self_kernel_dir.replace('\\', "/").to_lowercase();
    let mut found: Vec<String> = Vec::new();

    // ① wmic（CSV 实测列序 Node,CommandLine,ProcessId）
    let wmic = Command::new("wmic")
        .args([
            "process",
            "where",
            "name='node.exe'",
            "get",
            "ProcessId,CommandLine",
            "/format:csv",
        ])
        .output();
    if let Ok(out) = wmic {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if let Some(s) = parse_wmic_line(line, &marker) {
                    found.push(s);
                }
            }
        }
    }
    if !found.is_empty() {
        return found;
    }

    // ② 兜底：powershell Get-CimInstance
    if let Ok(out) = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" | Select-Object ProcessId,CommandLine | Format-List",
        ])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut cur_pid: Option<String> = None;
            for line in text.lines() {
                let l = line.trim();
                if let Some(pid) = l.strip_prefix("ProcessId : ") {
                    cur_pid = Some(pid.trim().to_string());
                } else if let Some(cmd) = l.strip_prefix("CommandLine : ") {
                    if let Some(pid) = cur_pid.take() {
                        if let Some(s) = parse_ps_pair(&pid, cmd.trim(), &marker) {
                            found.push(s);
                        }
                    }
                }
            }
        }
    }
    found
}

/// P2-17：字节预算内的字符边界安全截断（中文命令行防 panic）
fn floor_char_boundary(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn parse_ps_pair(pid: &str, cmd: &str, marker: &str) -> Option<String> {
    if !is_dsh_cmdline(&cmd.to_uppercase()) || cmd.is_empty() {
        return None;
    }
    if !marker.is_empty() && cmd.to_lowercase().contains(marker) {
        return None;
    }
    Some(format!("pid={pid} {}", floor_char_boundary(cmd, 120)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_char_boundary_chinese_safe() {
        let s = format!("x{}", "中".repeat(50));
        let t = floor_char_boundary(&s, 117);
        assert!(t.len() <= 117 && s.starts_with(t));
        assert_eq!(floor_char_boundary("abc", 120), "abc");
    }

    #[test]
    fn wmic_line_parse_real_layout() {
        // 本机实测行（列序 Node,CommandLine,ProcessId；CommandLine 含内部引号）
        let row = r#"AILIAS,""C:\Users\34021\AppData\Roaming\npm\node_modules\@deepseek-ai\dsh\lib\bin.js" web",15348"#;
        assert!(parse_wmic_line(row, "d:/xf/core/kernel").is_some());
        // 自身排除
        let own = r#"AILIAS,""D:\xf\core\kernel\lib\bin.js" web --patch x",9012"#;
        assert!(parse_wmic_line(own, "d:/xf/core/kernel").is_none());
        // 无关进程
        assert!(parse_wmic_line(r#"AILIAS,""C:\vendor.exe" x",123"#, "").is_none());
    }

    #[test]
    fn dsh_family_variants() {
        assert!(is_dsh_cmdline("@DEEPSEEK-AI\\DSH\\LIB\\BIN.JS WEB"));
        assert!(is_dsh_cmdline("C:\\NODE\\@DEEPSEEK-AI/DSH/LIB/BIN.JS"));
        assert!(is_dsh_cmdline("X:\\DSH\\LIB\\BIN.JS --profile web"));
        assert!(!is_dsh_cmdline("C:\\EXE\\MYTOOL.EXE"));
    }
}
