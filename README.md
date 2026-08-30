# DSH Desktop

DSH (DeepSeek Harness) 桌面版：Tauri 2.x (Rust) 原生 Windows 壳，属主管理官方内核
`@deepseek-ai/dsh` 子进程（无控制台 / 托盘 / 跟随官方 npm 更新 / 非网页套皮 / 性能好）。

设计文档：ARCHITECTURE.md（本仓库根）。

## 当前里程碑（M1+M2，t4）

- M1：Tauri 2 骨架（MSVC），git 已初始化。
- M2：内核集成（`src-tauri/src/kernel.rs`）——spawn 就绪契约 / /quit 优雅停止 / Job Object
  防孤儿 / 限次崩溃重启 / 端口策略（默认 --port 0）。

## v0.2 新特性（壳层，零内核改动）

- **并发守卫**：共享 ~/.dsh 时启动先检测另一 DSH 内核（浏览器版 dsh web 在跑即提示；双实例会写坏会话日志 torn JSONL）。
- **独立数据模式**：托盘→独立数据模式 → 数据转 app_data/dsh-home，与浏览器版不共享；切换立即重启内核。
- **开机自启 + 深链**：托盘→开机自启（HKCU Run）；dsh-desktop:// 协议启动即幂等注册（卸载不清理该键）。
- **破坏性更新保护**：新版本必须先通过「带桌面插件全冒烟」（就绪→/health→/quit），不兼容即保持当前版本；启动崩溃超限自动降级（无插件运行并提示，退出强制结束）。
- **性能**：立即窗口（loading.html 1s 内可见）→ 内核就绪自动进 UI；WebView2 防节流参数；NODE_OPTIONS 默认 4G 堆；内核 ABOVE_NORMAL 优先级。
- **低内存预警看门狗**：每 30s 检测系统可用提交内存，<1536MB 弹一次警告（滞回 >2.5GB 恢复）；settings.memory_warn_mb 可配（0=关闭）。
- settings.json 新字段（旧文件兼容默认值）：hardware_acceleration / node_options / boost_priority。

## v0.2.1 破坏性更新加固（壳层，零内核改动）

- **镜像 home 更新前冒烟（A）**：新版本准备完成后，用「镜像 DSH_HOME」做全量冒烟——复制真实
  `profiles/web/package.json`（与 `cordis.patch.yml` 若存在），`node_modules` 以 junction 指向真实目录，
  即以用户真实插件集启动新内核；45s 内必须就绪且 stdout+stderr 无失败签名
  （`cannot resolve profile bundle` / `ERR_MODULE_NOT_FOUND` / `Cannot find module` / `failed to load bundle`），
  命中即拒绝换版并**点名肇事插件**。真实 profile 不存在时退回 v0.2 纯隔离冒烟。
- **插件健康断言（B）**：镜像冒烟就绪后逐条 GET `settings.health_routes`
  （默认 `["/plugin-manager/api/list"]`，空数组=跳过，5s 超时），非 2xx 拒绝换版；
  plugin-manager 清单可解析时，对 `exists/resolved=false` 条目点名报错。
- **安全模式（C）**：托盘「安全模式（禁用第三方插件）」或 CLI `--safe-mode 1|0`——
  以最小 profile（`<DSH_HOME>/profiles/dsh-safe`，仅 `@deepseek-ai/dsh-base` + `@deepseek-ai/dsh-web-app`）
  启动内核，**绝不修改 profiles/web 任何文件**（safe profile 缺失自动生成、已存在不覆盖）；
  内核反复崩溃放弃时，错误对话框与 loading 页文案会引导用安全模式排查。
- settings.json 新字段（旧文件兼容默认值）：`health_routes` / `safe_mode` / `safe_profile`。
- 运维验证钩子：`dsh-desktop.exe --mirror-smoke <DSH_HOME>` 对当前内核跑镜像冒烟+健康断言
  （MIRROR_SMOKE_OK/FAIL，exit 0/1）。
- 清理安全红线：镜像 home 的 node_modules junction 清理必须先 `remove_dir` 摘链再删树
  （`cleanup_mirror_home` 强制此序），严禁对含 junction 的树 `remove_dir_all`。

## 安装包（D6 方案 B 全捆绑 · 用户拍板 2026-08-29）

`npx tauri build` 产出 NSIS 安装包，**直接包含**：Tauri 壳 + 捆绑 `runtime/node.exe`（v24.16.0）+ 官方内核 `kernel/`（npm 全局闭包，~250MB 未裁剪）+ `desktop/` patch 插件 + WebView2 embedBootstrapper（离线可装）。
即装即用、离线完整。**安装包捆绑 ≠ 更新机制变化**：运行时版本跟随已实现（updater.rs，M4）——Registry→tgz(sha512 integrity)→解压→**npm install 物化依赖树**（tgz 仅 33KB 骨架）→`--help` 冒烟→内核停止后 rename 原子替换 kernel↔kernel.old→重启；失败/新版本 10min 崩溃≥2 次自动回滚（kernel.old）；DSH_HOME 零迁移。托盘右键「检查更新」手动触发（当前 registry latest=0.1.1-rc.2=捆绑版本，无更新可检查）。

> 体积预期：整体安装包约 200-250MB（当前未做平台裁剪；后续可按 ARCHITECTURE §5.3 剪 node-pty/koffi 预编译再降 50-80MB）。
## 开发运行

```bash
cd src-tauri
cargo run            # 开发运行（内核缺省按序取：捆绑 kernel/ → DSH_DESKTOP_KERNEL 环境变量 → %APPDATA%/npm/node_modules/@deepseek-ai/dsh → npm root -g）
cargo test           # 单元测试（就绪行解析等）
cargo run -- --smoke # 冒烟：启动内核→就绪→/quit→退出（打印 SMOKE_OK port=N）
```

配置（首次运行生成到 app_data/settings.json；DSH_HOME 为空=默认 ~/.dsh 共享）：
```json
{ "portMode": "auto", "dshHome": "", "telemetryDisabled": true, "kernelPath": "", "nodePath": "" }
```

## 约束（ARCHITECTURE.md §10）

launcher flags（--patch）必须先于应用 flags；/quit 唯一优雅停止；Job Object 防孤儿；
DSH_TELEMETRY_DISABLED=1 默认；更新/回滚绝不触碰 DSH_HOME。