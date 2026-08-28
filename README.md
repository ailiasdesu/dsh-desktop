# DSH Desktop

DSH (DeepSeek Harness) 桌面版：Tauri 2.x (Rust) 原生 Windows 壳，属主管理官方内核
`@deepseek-ai/dsh` 子进程（无控制台 / 托盘 / 跟随官方 npm 更新 / 非网页套皮 / 性能好）。

设计文档：ARCHITECTURE.md（本仓库根）。

## 当前里程碑（M1+M2，t4）

- M1：Tauri 2 骨架（MSVC），git 已初始化。
- M2：内核集成（`src-tauri/src/kernel.rs`）——spawn 就绪契约 / /quit 优雅停止 / Job Object
  防孤儿 / 限次崩溃重启 / 端口策略（默认 --port 0）。

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