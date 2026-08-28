# DSH Desktop 验收评审报告（t6）

- 评审人：reviewer（架构/质检/验收） · 团队任务 t6 · 日期：2026-08-29
- 评审基线：`dsh desktop版 以及dsh插件制作/dsh-desktop`，评审期间 HEAD 持续推进：
  - 评审起点 6e68738（M1+M2+M4/NSIS）
  - 评审中新增提交 80ac3c8（D6 方案B 拍板 docs）/ 28d3e67（t5 全捆绑 NSIS）/ c914b07（README 更新机制改为「设计定稿/实现待办」）
  - 未提交工作区：Cargo.toml（新增 tray-icon feature + sha2/base64/flate2/tar）+ updater.rs（272 行，未接线）
- 报告文件名按任务原文 REVIEEW.md 输出（疑似 REVIEW.md 笔误，建议更正）
- 全部实测均使用隔离 DSH_HOME / 临时目录，未触碰用户 ~/.dsh；测试安装（两次）均已卸载并清理

---

## 0. 验收结论（一句话）

**壳层核心链路（属主内核：spawn→就绪→窗口→/health→/quit→无残留→单实例→崩溃限次重启）实测全部通过；「全捆绑 NSIS 安装包」（28d3e67 提交）已由本评审重新打包+装机冒烟验证可用（50MB 安装器，解包 356MB 布局正确）；但「跟随官方更新」虽已有 updater.rs 初稿，dry-run 实测证明其更新流程在真实 npm tarball 上必然失败（tgz 不含 node_modules → 冒烟 ERR_MODULE_NOT_FOUND），且未接线、未提交；托盘仍缺失（Cargo.toml 已加 tray-icon feature，无实现代码）。** 五条要求：①❌ ②✅ ③✅ ④✅（当前 HEAD 已验证） ⑤✅。

## 1. 五条要求逐项验收

| # | 要求 | 判定 | 实测证据 |
|---|---|---|---|
| ① | 跟随官方更新内核 | ❌ 未实现/不可用 | updater.rs（未提交未接线）初稿存在：check_latest / verify_tarball / extract_tarball / smoke_kernel / apply_swap / rollback / run_update_flow。**但 dry-run 实测必败**：npm tgz 仅含 lib/config/package.json（33KB，实测无 node_modules）→ extract 后 kernel.new 无依赖 → smoke `node bin.js web --help` 直接 `ERR_MODULE_NOT_FOUND: Cannot find package '@deepseek-ai/dsh-app-boot'（实测复现）→ 更新永远报「更新失败」。其余问题：fetch 依赖外部 curl；update_channel/keep_old_kernel 设置项仍未接入（写死 latest）；无 UI/托盘触发点；main.rs 无 `mod updater`（不编译，纯死文件） |
| ② | 性能好 | ✅ | 壳 WS≈27MB（28,221,440 B）、exe 7.9MB（strip+lto）；内核 node WS≈111MB（功能必需）；无 Chromium；启动到就绪 <12s。安装器 50MB（全捆绑，实测）、解包 356MB（kernel 260MB + runtime 89MB + 壳 8MB）——远优于 Electron 方案；D6「<220–250MB」以「安装器体积」口径达成（50MB），以「解包 footprint」口径未达成（356MB），建议 README 明确口径（ARCHITECTURE §9 原文即口径分裂，见 P2-8） |
| ③ | 非纯 web 套皮 | ✅ | 进程属主（Rust spawn/kill）/ 单实例 / Job Object 防孤儿 / 端口管理 / 崩溃重启 / 退出清理全原生 Rust；WebView2 仅作视口加载内核官方前端；内核零修改（官方 CLI + 官方 --patch 机制） |
| ④ | 有安装包可安装 | ✅（当前 HEAD 全捆绑版） | 已提交产物（6e68738，3.44MB）**安装即坏**（实测复现：资源落位 `_up_\desktop\` + 内核/Node 未捆绑 → ERR_MODULE_NOT_FOUND → 崩溃×3）。28d3e67 全捆绑修复后由本评审**重新打包装机验证**：布局 desktop/{quit,health}.js + kernel/ + runtime/node.exe + exe + uninstall.exe 正确；`--smoke` → `SMOKE health=true port=10543 / SMOKE_OK` ✓；卸载干净 ✓。安装器 50MB（原 3.44MB 壳版仍可作「薄壳形态」备选） |
| ⑤ | 双击启动无需 shell | ✅ | windows_subsystem="windows" 无控制台；Start-Process/双击 → 窗口「DSH Desktop」（MainWindowHandle 实测）；子进程 CREATE_NO_WINDOW；全捆绑后干净机器可用（不需要 Node/npm 预装） |

## 2. 运行时验证记录（DSH_HOME 隔离，两次安装均实测后卸载）

### 2.1 dev 布局 smoke — ✅
`DSH_HOME=<temp> src-tauri/target/release/dsh-desktop.exe --smoke` → `SMOKE health=true port=7415 / SMOKE_OK / EXIT=0`；就绪契约 stdout `dsh web: http://127.0.0.1:7415` ↔ parse_port_from_line 一致；cargo test 1/1 通过。

### 2.2 GUI 启动 — ✅
窗口「DSH Desktop」（PID 14256）；内核 spawn 契约完整：runtime/node.exe + kernel/lib/bin.js web --patch %APPDATA%\com.dshdesktop.app\desktop.patch.yml --no-open --port 0；--port 0 → 动态端口 13797（netstat 实测）；GET /health → 200 "ok"。

### 2.3 单实例 — ✅
第二实例 0.29s 退出；仅剩 1 壳 + 1 内核；聚焦回调存在（聚焦动作无法程序化断言）。

### 2.4 用户退出（窗口 X）— ✅
WM_CLOSE → ExitRequested → ctl.stop → /quit → 内核优雅退出 → 壳退出；8s 后壳+内核 node 全部消失，无残留（/quit + Job Object 双保险）。

### 2.5 内核生命周期 — ✅（含观察项）
- 崩溃→自动重启（退避 1s/5s）→ 窗口重定向到新端口 → 健康；限次 2 次后收敛放弃 + 错误对话框（安装版崩溃×3 场景验证）。
- 观察项：任何内核退出（含外部 /quit）均计「崩溃」入重启队列（P3-11）；GUI 模式壳 eprintln 全部丢失无处可查（P1-5）；首起内核曾有 1 次异常退出后自动重启成功（原因无法从日志定位，印证 P1-5）。

### 2.6 安装包（旧提交 6e68738）— ❌ 安装即坏（复现，见 §1 ④）
静默安装 → 资源落 `_up_\desktop\`；`--smoke` → 内核 3 次 `ERR_MODULE_NOT_FOUND …desktop\health.js` → `crashed too many times, giving up` → **EXIT=0（假成功，P1-3）**。

### 2.7 安装包（新提交 28d3e67 全捆绑，本评审重打包复验）— ✅
50MB 安装器 /S → `%LOCALAPPDATA%\DSH Desktop\`：desktop/{quit,health}.js、kernel/（260MB，29,488 文件）、runtime/node.exe（89MB）、dsh-desktop.exe、uninstall.exe → 安装态 smoke `SMOKE health=true port=10543 / SMOKE_OK / EXIT=0` ✓ → 卸载清理 ✓。**结论：P0-1 已随 28d3e67 修复并验证。**

### 2.8 更新流程 dry-run（对 updater.rs 全流程）— ❌ 必然失败
以真实 tarball `@deepseek-ai/dsh-0.1.1-rc.2.tgz`（33KB）复现 updater.rs 逻辑：下载 ✓ → integrity 字段存在（sha512-…）✓ → 解压 `package/` 后**无 node_modules**（tar 列表实测 0 条）→ `node kernel.new/lib/bin.js web --help` → `ERR_MODULE_NOT_FOUND: Cannot find package '@deepseek-ai/dsh-app-boot'（实测报错）→ 冒烟返回非 0 → run_update_flow 报「更新失败」。**系统是安全失败（不破坏现有 kernel），但功能不可用**。修复方向见 P0-2。

### 2.9 构建/测试环境
cargo test 1/1 通过（须 vcvars64 环境；git bash 直跑复现双坑：coreutils link 劫持 + LNK1181 缺 kernel32.lib）。已知坑已属团队记忆，但 README 未写演练指引（P2-10）。

## 3. 问题清单（严重度 + 修复建议）

### P0-1 ✅已修复（28d3e67，本评审验证）安装产物资源落位 + 内核/Node 未捆绑
原提交产物安装即坏（§2.6 全复现）；28d3e67 resources 对象映射 + kernel/runtime 全捆绑后，本评审重打包装机验证通过（§2.7）。遗留建议：① CI/交付门禁加「产物安装→--smoke→退出无残留」自动化（当前产物验证靠手工）；② 更稳健替代（可选）：quit.js/health.js 改 include_str! 内嵌 + 运行时写入 data_dir，彻底摆脱安装目录布局推断。

### P0-2 ❌未移交（有初稿）「跟随官方更新」不可用
见 §2.8——updater.rs 全流程 dry-run 必败（tgz 无依赖）。修复方向（择一）：
a) 更新时在 kernel.new 上执行 `npm install --omit=dev --no-fund`（依赖目标机 npm，与「免 npm」设计冲突，需评估）；
b) **推荐**：采用「全量捆绑包」更新源（自建分发，每次发布打包含 node_modules 的完整包）——与 D6 方案B 一致；
c) 官方包若提供含依赖的集成发布物则改用它。
同时：接线（main.rs mod updater + 触发器：托盘菜单/设置页/启动后台检查）、fetch 去 curl 依赖（reqwest+rustls 或复用内核网络栈）、接入 update_channel（写死 latest）、keep_old_kernel 策略接入。

### P1-3 ❌ smoke 失败假成功（EXIT=0）
resolve 失败/崩溃放弃路径均 `process::exit(0)`；安装版崩溃×3 实测 EXIT=0（无 SMOKE_OK）。**所有自动化验收依赖此信号**。修复：失败路径 exit 非 0（如 2），成功打印 SMOKE_OK 后 exit 0。

### P1-4 ❌ 托盘缺失
Cargo.toml 已加 `tray-icon` feature，但无 tray.rs/无 TrayIconBuilder 代码（grep 实测）；当前窗口关闭=整体退出。修复：M3 实现托盘（显示/检查更新/退出 + close→hide 语义）；README 已由 c914b07 改为「设计/待办」，需功能落地后回改。

### P1-5 ❌ 壳层诊断日志丢失（GUI 子系统无控制台）
kernel.rs 全部 eprintln（ready port/崩溃计数/失败原因）无落盘；仅内核 stderr 进 kernel.log（append，无 5MB 轮转）。修复：壳日志写 data_dir/logs/shell.log + 轮转；Job Object assign 失败需告警（当前仅 eprintln=静默丢防孤儿）。

### P2-6 MODULE_TYPELESS_PACKAGE_JSON 警告（每次内核启动，实测 3 条）
修复：desktop/ 加 package.json {"type":"module"}（随资源分发）或改用 .mjs。

### P2-7 安装目录含空格（"DSH Desktop"）
file_url() 不做百分号编码；实测 node 自行 %20 转义未出问题（本次失败是文件缺失所致）；修复布局后待回归确认（建议 file_url 主动 encode，顺带消除隐患）。

### P2-8 体积口径分裂
解包 356MB vs D6「<220–250MB」（安装器口径 50MB 则达标）。建议：README/ARCHITECTURE 明确「安装器 ≤250MB」口径并回填实测值（50MB 安装器/356MB 解包/29,488 文件）；按 ARCHITECTURE §5.3 裁剪内核（去 dev/平台切片）压 footprint 留作 D6 后续决策。

### P2-9 ✅已缓解（c914b07）文档过度声明
README 已改为「更新机制=设计定稿/实现待办」；其余 ARCHITECTURE §8「✅定稿」列仍建议增加「实现状态」列。

### P2-10 ❌ 构建指引缺失
README 仅写 `cargo run`；MSVC 双坑（link 劫持/LNK1181）无说明。修复：README 补 vcvars64 或 .cargo/config.toml 固化 linker。

### P3-11 崩溃计数语义
任何内核退出（含未来优雅退出 path）计「崩溃」；建议 exit code 0 不计入。

### P3-12 updater 单元测试覆盖率
仅 2 测（integrity 解析/无网络安全返回）；建议为 extract/swap/rollback 加临时目录单测，并把 §2.8 dry-run 固化为集成测试（mock registry）。

## 4. 通过项（保留证据）
1. 内核 spawn/就绪/端口契约与实测一致（动态端口、防保留段冲突）。
2. /quit 优雅停止 + Job Object 兜底；退出无残留（验收指标达成）。
3. 单实例：第二实例快速退出 + 聚焦回调。
4. 限次崩溃重启（≤2 + 退避），超限收敛错误对话框。
5. 壳静态体积极小（exe 7.9MB / WS 27MB），无 Chromium。
6. DSH_HOME 与内核解耦，隔离运行验证通过（更新/隔离模式下数据零迁移的前提成立）。
7. 单元测试通过；测试逻辑与真实输出一致。
8. 全捆绑安装包（28d3e67）装机可用：布局正确、安装态 smoke 通过、安装/卸载干净（本评审两轮实测）。

## 5. 验收判定汇总
- ① 跟随官方更新：**❌ 不可用**（updater 初稿 dry-run 必败 + 未接线，P0-2）
- ② 性能：**✅ 达标**（壳层数字健康；体积口径待文档化）
- ③ 非纯 web 套皮：**✅ 达标**
- ④ 安装包可安装：**✅ 达标**（28d3e67 全捆绑版已验证；需 CI 门禁固化）
- ⑤ 双击启动无需 shell：**✅ 达标**

**结论：M1/M2/M4-打包 已达成可交付质量（P0-1 已修），M3（托盘）与 M4-更新 未完成——按用户五条要求，当前仅剩「跟随官方更新」未达成；建议：updater 按 P0-2 修复方向重设计（全量捆绑更新源），完成接线+托盘触发后做第二轮验收。**
