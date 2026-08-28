# DSH Desktop

DSH (DeepSeek Harness) 桌面版：Tauri 2.x (Rust) 原生 Windows 壳，属主管理官方内核
`@deepseek-ai/dsh` 子进程（无控制台 / 托盘 / 跟随官方 npm 更新 / 非网页套皮 / 性能好）。

设计文档：ARCHITECTURE.md（本仓库根）。

## 当前里程碑（M1+M2，t4）

- M1：Tauri 2 骨架（MSVC），git 已初始化。
- M2：内核集成（`src-tauri/src/kernel.rs`）——spawn 就绪契约 / /quit 优雅停止 / Job Object
  防孤儿 / 限次崩溃重启 / 端口策略（默认 --port 0）。

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