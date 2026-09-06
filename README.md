# DSH Desktop

DeepSeek Harness 的 Windows 桌面版：Rust/Tauri 原生窗口与进程管理，WebView2 显示官方前端。

当前桌面版本 **0.2.5**，原生性能适配验证基线为官方内核 **0.1.2-rc.1**。

## 特性

- **原生窗口**：Tauri 2.x (Rust) 壳，无控制台黑框，托盘常驻，关闭按钮最小化到托盘
- **即装即用**：安装包捆绑完整运行时（Node + 官方内核），离线可用，无需 npm/pnpm
- **自动更新**：跟随官方 `@deepseek-ai/dsh` 更新，下载→校验→替换→失败回滚；会话格式与迁移由官方负责
- **性能**：复用系统 WebView2；大历史通过 Rust Node-API 后台分批读取，独立文本索引按需启动
- **兼容**：原生加速只对已验证的内核启用，未知版本使用官方路径；原始会话不依赖原生缓存才能读取
- **进程管理**：单实例互斥、托盘退出和子进程生命周期管理

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
| 会话搜索 | 官方搜索功能 | 保留官方搜索，额外提供当前会话 Rust 文本搜索工具 |
| 数据共享 | `~/.dsh` | 默认共享，可选独立 |

## 0.2.5 性能优化

- 压缩日志至少 32 MiB 时启用 Rust 后台读取与有界预取，较小日志保留官方读取。事件解码、回放、写入和修复继续使用官方实现。
- `desktop_session_search` 使用独立 SQLite 派生缓存，支持增量更新、容量限制、会话淘汰和版本校验；不向官方数据库预填充数据。
- 可选 Advisor 增量补丁消除每回合的完整历史扫描，保留原插件 UI。它由独立部署清单管理，桌面安装包不会自动覆盖任意版本的第三方插件。

隔离合成基准：128 MiB 历史读取 p95 从 653.93 ms 降至 470.56 ms，对应进程总私有内存峰值中位数从 522.72 MiB 降至 356.43 MiB。数字仅描述该读取场景，不代表所有真实会话、前端首屏或远端模型推理。

详见[交付报告](docs/performance/RELEASE.md)、[验证证据与限制](docs/performance/completion-audit.json)和[Advisor 补丁说明](plugins/memory-advisor-performance/README.md)。真正的磁盘分页仍依赖官方范围读取接口；侧栏缓存实验未通过性能门槛，没有启用。

## 开发

需要 Windows Rust/MSVC 工具链、Node.js 和 npm。`kernel/`、`runtime/` 与生成的 `native/` 不纳入 Git；打包前需准备官方内核及 Node 运行时。本次验证使用内核 0.1.2-rc.1 和 Node 24.16.0。

```powershell
npm ci
powershell -NoProfile -File scripts/build-native.ps1
npm run tauri -- build --bundles nsis
```

Tauri 的构建前脚本也会生成原生组件。安装包输出到 `src-tauri/target/release/bundle/nsis/`。

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
- **历史读取**：异步 Node-API Rust 模块、有界批次；版本不匹配或读取失败时回退官方路径
- **文本搜索**：按需启动的 Rust helper 和独立 SQLite 缓存；按事件进行字面文本匹配，不替代官方排序全文搜索

## 许可

与 DeepSeek Harness 相同。
