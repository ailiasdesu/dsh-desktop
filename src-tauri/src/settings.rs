//! 壳层配置（ARCHITECTURE.md §7.3：app_data_dir/settings.json）
//! 默认值遵循决策：D1 latest / D2 端口 auto / D4 DSH_HOME 共享(空=~/.dsh) / 遥测默认关
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// 更新通道：latest | next（D1）
    pub update_channel: String,
    /// 启动时后台检查更新 + 每 24h（D1）
    pub auto_check_update: bool,
    /// 端口策略：auto（--port 0，D2 默认）| fixed:<port>
    pub port_mode: String,
    /// DSH_HOME 覆盖；空=默认 ~/.dsh（D4 共享默认）
    pub dsh_home: String,
    /// 透传 DSH_TELEMETRY_DISABLED（默认关）
    pub telemetry_disabled: bool,
    /// 更新后保留 kernel.old 用于回滚（D5）
    pub keep_old_kernel: bool,
    /// node.exe 路径；空=捆绑 runtime/node.exe 或 PATH node
    pub node_path: String,
    /// 内核包路径（lib/bin.js 所在包的根）；空=捆绑 kernel/ 或 npm 全局
    pub kernel_path: String,
    /// 硬件加速（E：false 时 WebView2 追加 --disable-gpu）
    pub hardware_acceleration: bool,
    /// 内核 NODE_OPTIONS（E：默认 4G 堆上限；空=不注入）
    pub node_options: String,
    /// 内核子进程优先级提升（E：ABOVE_NORMAL）
    pub boost_priority: bool,
    /// 低内存预警（F）：可用提交内存低于该 MB 值弹一次警告；0=关闭（默认 1536）
    pub memory_warn_mb: u64,
    /// B（v0.2.1）：更新镜像冒烟就绪后的健康断言路由（逐条 GET 须 2xx）；空数组=跳过
    pub health_routes: Vec<String>,
    /// C（v0.2.1）：安全模式——用最小 profile（仅官方 dsh-base/dsh-web-app）启动，禁用第三方插件
    pub safe_mode: bool,
    /// C（v0.2.1）：安全模式 profile 名（<DSH_HOME>/profiles/<safe_profile>），缺省 dsh-safe
    pub safe_profile: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            update_channel: "latest".into(),
            auto_check_update: true,
            port_mode: "auto".into(),
            dsh_home: String::new(),
            telemetry_disabled: true,
            keep_old_kernel: true,
            node_path: String::new(),
            kernel_path: String::new(),
            hardware_acceleration: true,
            node_options: "--max-old-space-size=4096".into(),
            boost_priority: true,
            memory_warn_mb: 1536,
            health_routes: vec!["/plugin-manager/api/list".into()],
            safe_mode: false,
            safe_profile: "dsh-safe".into(),
        }
    }
}

impl AppSettings {
    pub fn load(dir: &Path) -> Self {
        let p = dir.join("settings.json");
        match std::fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                eprintln!("[settings] parse error, use defaults: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let _ = std::fs::create_dir_all(dir);
        let p = dir.join("settings.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| std::io::Error::other(e))?;
        std::fs::write(&p, json)?;
        Ok(p)
    }

    /// 真实 DSH_HOME（A/C 共用）：settings.dsh_home 非空则用之，否则 %USERPROFILE%/.dsh（POSIX 退 HOME）
    pub fn real_dsh_home(&self) -> PathBuf {
        if !self.dsh_home.is_empty() {
            return PathBuf::from(&self.dsh_home);
        }
        let base = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        Path::new(&base).join(".dsh")
    }
}

/// B：health route 合法性（须以 / 开头且不含空白）；非法项由调用方跳过并告警
pub fn valid_health_route(r: &str) -> bool {
    r.starts_with('/') && !r.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_v021_fields() {
        let s = AppSettings::default();
        assert_eq!(s.health_routes, vec!["/plugin-manager/api/list".to_string()]);
        assert!(!s.safe_mode);
        assert_eq!(s.safe_profile, "dsh-safe");
    }

    #[test]
    fn old_settings_json_backward_compatible() {
        // v0.2.0 的 settings.json（无 v0.2.1 新字段）必须能反序列化并落默认值
        let old = r#"{"update_channel":"latest","auto_check_update":true,"port_mode":"auto","dsh_home":"","telemetry_disabled":true,"keep_old_kernel":true,"node_path":"","kernel_path":"","hardware_acceleration":true,"node_options":"","boost_priority":true,"memory_warn_mb":1536}"#;
        let s: AppSettings = serde_json::from_str(old).expect("old settings must parse");
        assert_eq!(s.health_routes, vec!["/plugin-manager/api/list".to_string()]);
        assert!(!s.safe_mode);
        assert_eq!(s.safe_profile, "dsh-safe");
    }

    #[test]
    fn health_route_validation() {
        assert!(valid_health_route("/plugin-manager/api/list"));
        assert!(valid_health_route("/health"));
        assert!(!valid_health_route(""));
        assert!(!valid_health_route("health"));
        assert!(!valid_health_route("/a b"));
        assert!(!valid_health_route("http://x/y"));
    }

    #[test]
    fn real_dsh_home_prefers_override() {
        let s = AppSettings { dsh_home: "X:/mirror-home".into(), ..Default::default() };
        assert_eq!(s.real_dsh_home(), PathBuf::from("X:/mirror-home"));
        let d = AppSettings::default().real_dsh_home();
        assert!(d.ends_with(".dsh"));
    }
}