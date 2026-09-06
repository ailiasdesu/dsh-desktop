# DSH Desktop 0.2.5：性能优化交付

内核保持 **0.1.2-rc.1**。本次交付原生历史读取、独立会话文本索引和 Advisor 增量处理。原始会话格式、官方写入/恢复逻辑、原插件界面保留。

## 已安装的功能

- **大历史读取**：Windows Node-API 后台解压，有界二进制批次及预取；官方函数继续解码 packed rows、provenance，官方 coordinator 继续校验、管理生命周期和恢复。仅对压缩文件至少 32 MiB 启用，较小日志走官方路径。
- **独立文本索引**：Rust 辅助进程按需启动，支持增量更新、大消息重叠分片、1 GiB/4096 会话限额、完整会话 LRU 淘汰和事务替换。并发淘汰/替换不能产生虚假的“无匹配”结果。`desktop_session_search` 可在当前会话中使用。
- **Advisor**：正常事件流只处理增量，不再每回合复制/扫描完整历史；删除未被消费的全消息指纹计算。中途订阅或恢复缺失前缀仍补齐，legacy 行为和原 UI/CSS 保留。
- **更新回退**：未验证的内核版本、缺少原生文件时保留官方后端；原生读取失败回到官方读取，取消保持取消。不会将跳过的坏帧或部分历史当作成功。

程序文件已安装到 `C:/Users/34021/AppData/Local/DSH Desktop`。当前已打开的旧进程没有被强制结束；请在方便时从托盘选择“退出”，再打开 DSH Desktop，让新版本全部生效。

## 最终测量

均为隔离合成数据、相同输入和结果检查；进程冷热与 OS 磁盘缓存分别标注。数字不代表所有真实项目都获得相同比例提升。

| 场景 | 官方/原实现 | 新实现 | 证据 |
|---|---:|---:|---|
| 128 MiB 历史读取 p95 | 653.93 ms | 470.56 ms | `results/history-final/summary.json` |
| 同场景总私有内存峰值中位数 | 522.72 MiB | 356.43 MiB | 同上，5 次独立进程/实现 |
| 已建索引的文本搜索总私有内存峰值中位数 | 410.02 MiB | 87.98 MiB | `results/index-final/summary.json`，含辅助进程 |
| 同搜索 p95 | 101.08 ms | 94.27 ms | 同上，每实现 60 次查询 |
| 普通日志读取 p95（不启用原生读取） | 110.34 ms | 110.32 ms | `results/history-balanced-final/summary.json` |
| Advisor：10 万历史回合后每新回合 p95 | 52.41 ms | 0.14 ms | `plugins/memory-advisor-performance/results/benchmark.json`，仅 observer 基准 |

Advisor 大历史基准的峰值 RSS 为 461.56 → 165.42 MiB；它是隔离 observer 进程，不是整个 DSH 的内存。初次建立文本索引有单独成本，不能用已建索引的搜索耗时冒充首次建立耗时。

## 验证

- Native helper 的协议、配额、分片、SQLite 迁移、原子更新和压缩流测试通过。
- 最终 Node 测试 24 项通过，覆盖真实 Node-API、官方 Session/持久化服务、版本回退、文件变更、所有 skippable magic、帧头限制、最后一批取消及 Buffer 生命周期。
- 两个真实 helper 的并发淘汰/替换回归通过；9 项安装恢复测试通过，包括在运行的 Windows 映像替换和重命名中途恢复。
- 23 项桌面壳测试、136 项 Advisor 原/新行为对照测试通过。
- 已安装目录的 17 个文件哈希通过；新可执行文件的隔离 mirror smoke 返回 `MIRROR_SMOKE_OK`。
- 使用已安装的模块，在完整隔离 DSH CLI 中验证了真实 history WebSocket 快照、原配置保留、当前会话搜索工具、官方回退和正常退出。见 `results/installed-runtime/summary.json`。
- 原有 21 项适配回归通过，45 个修复文件的校验与 JavaScript 语法通过。
- 完整审查的已确认问题均修复；审查原件及调用方处理记录见 `review/`。

## 有意保留的边界

- 官方目前仍需要完整事件源。真正的磁盘分页历史接口在 0.1.2 中不存在，本次没有另写会话控制器，也不把只读预览冒充已恢复会话。
- 标题清理和部分投影工作仍由官方 JavaScript 完成；原生读取不能消除它们，也不能加速远端模型自身的推理。
- 侧栏预览缓存候选因轮换延迟和内存退化没有安装。原侧栏继续使用原实现，实验结果保留在 `plugins/dsh-side-panel-performance/bench/RESULTS.md`。
- Rust 哈希/文件片段能力已构建和测量，但没有绕过官方 `ctx.fs` 契约硬接到附件模块；当前 provider 不提供原始字节流/原生 hash 注入点。它不是已完成的附件 UI 加速承诺。
- 更新到其他内核版本会停用未验证的这层原生加速，重新验证后再启用。第三方插件升级也可能覆盖本地 Advisor 改动；安装脚本对原文件 SHA 不匹配会拒绝覆盖。

## 文件和回退

安装包：`src-tauri/target/release/bundle/nsis/DSH Desktop_0.2.5_x64-setup.exe`。

本次备份清单：`C:/Users/34021/AppData/Local/DSH Desktop/repair-checks/native-performance-backup-20260906T101747330484Z/manifest.json`。原始备份和两次校验/策略微调前的字节均保留。

在源码仓库执行以下命令可恢复本次运行文件；会检查当前哈希，遇到其他修改会拒绝覆盖：

```powershell
python scripts/deploy-performance.py --rollback "C:\Users\34021\AppData\Local\DSH Desktop\repair-checks\native-performance-backup-20260906T101747330484Z\manifest.json"
```

回退针对安装文件，不重置源码分支。源码版本校验与运行文件校验应区分；不应通过改源码来掩盖运行文件不匹配。
