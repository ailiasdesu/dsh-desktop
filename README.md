# DSH Desktop

DeepSeek Harness 的 Windows 桌面版：原生窗口应用，非网页套皮。

## 特性

- **原生窗口**：Tauri 2.x (Rust) 壳，无控制台黑框，托盘常驻，关闭按钮最小化到托盘
- **即装即用**：安装包捆绑完整运行时（Node + 官方内核），离线可用，无需 npm/pnpm
- **自动更新**：跟随官方 `@deepseek-ai/dsh` 更新，下载→校验→原子替换→自动回滚，数据零迁移
- **性能**：WebView2 系统组件（不携带 Chromium），壳内存 20-60MB，内核按需启动
- **安全**：单实例互斥，会话搜索由 Rust 侧预填充索引（官方路径在大数据量下会崩溃，已绕过）

## 使用

**安装**：下载 NSIS 安装包，双击安装（无需管理员权限）。

**开机自启**：托盘菜单 → 开机自启（默认关闭，手动开启）。

**独立数据模式**：托盘菜单 → 独立数据模式（与浏览器版 dsh web 不共享数据；默认共享 `~/.dsh`）。

**安全模式**：托盘菜单 → 安全模式（禁用第三方插件，用于排查插件兼容性问题）。

**检查更新**：托盘菜单 → 检查更新（自动检查+24h 周期+手动触发）。

## 与浏览器版的区别

| 维度 | 浏览器版 (`dsh web`) | DSH Desktop |
|---|---|---|
| 启动方式 | 终端 `npx dsh web` → 开浏览器 | 双击图标，窗口即用 |
| 进程管理 | 手动 Ctrl+C，可能残留 | 托盘退出，自动清理（Job Object） |
| 端口 | 固定 3080（可能撞保留段） | 动态 `--port 0`（自动分配空闲端口） |
| 更新 | 手动 `npm update` | 自动检测+下载+回滚 |
| 会话搜索 | 大数据量下崩溃 | Rust 预填充索引，毫秒级 |
| 数据共享 | `~/.dsh` | 默认共享，可选独立 |

## 开发

```bash
cd src-tauri
cargo run            # 开发运行（自动找内核：捆绑 → DSH_DESKTOP_KERNEL → npm 全局）
cargo test           # 单元测试
cargo run -- --smoke # 冒烟测试（启动→就绪→优雅退出）
```

**首次运行配置**（自动生成到 `%APPDATA%/com.dshdesktop.app/settings.json`）：
```json
{
  "portMode": "auto",           // auto(动态端口) | fixed:3379
  "dshHome": "",                // 空=默认 ~/.dsh；可指向独立目录
  "useAppHostname": true        // WebView 走 dsh.localhost（false 回退 127.0.0.1）
}
```

## 技术栈

- **壳层**：Tauri 2.x (Rust)、WebView2、tokio
- **内核**：官方 `@deepseek-ai/dsh`（Node.js，Cordis 插件树）
- **更新**：Registry → tgz → sha512 校验 → 原子替换 → 崩溃自动回滚
- **搜索**：Rust sidecar 预填充 SQLite FTS5 索引（多帧 zstd 解码）

## 许可

与 DeepSeek Harness 相同。
