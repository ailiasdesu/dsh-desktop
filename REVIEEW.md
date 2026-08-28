# DSH Desktop 验收评审报告（t6）

- 评审人：reviewer（架构/质检/验收） · 团队任务 t6 · 日期：2026-08-29
- 评审对象：`dsh desktop版 以及dsh插件制作/dsh-desktop`（git 提交 6e68738 = M1+M2+M4/NSIS）
- 评审范围：用户五条要求逐项验收；实际运行安装包与开发版（启动/托盘/退出/内核生命周期/更新流程 dry-run）验证
- 说明：报告文件名按任务原文 REVIEEW.md 输出（疑似 REVIEW.md 笔误，建议后续更正）
- Google 无外部依赖；所有实测均使用隔离 DSH_HOME（临时目录），未触碰用户 ~/.dsh，测试安装已卸载清理

---

## 0. 验收结论（一句话）

**壳层核心链路（属主内核：spawn→就绪→窗口→/health→/quit→无残留）实测通过；但「跟随官方更新」完全未实现、已提交的 NSIS 安装包安装后启动必失败（资源落位错误 + 内核/Node 未捆绑）、托盘缺失——五条要求仅 ②③⑤ 达标（⑤受④阻塞），不可交付。** 工作区存在一个未提交的 tauri.conf.json 修复（resources 对象映射），方向正确但未重新打包验证，见 P0-1。

## 1. 五条要求逐项验收

| # | 要求 | 判定 | 实测证据 |
|---|---|---|---|
| ① | 跟随官方更新内核 | ❌ 未实现 | 全仓库无 updater/版本检查/下载/替换代码（grep 实测）；settings.rs 仅存 3 个死配置字段（update_channel/auto_check_update/keep_old_kernel，无任何代码读取）；registry latest=0.1.1-rc.2=捆绑内核版本（`npm view @deepseek-ai/dsh version`），恰好无更新可触发；ARCHITECTURE §5 的 updater.rs 流程仅为设计稿；README 却宣称「跟随官方 npm 更新」——过度声明 |
| ② | 性能好 | ✅（壳层）/ ⚠️（整体体积未回填） | 实测壳 WS≈27MB（28,221,440 B）；内核 node WS≈111MB（功能必需）；壳 exe 7.9MB（strip+lto）；安装器 3.44MB（不含内核）；无 Chromium（WebView2 系统复用）；启动到就绪 <12s。⚠️ runtime/node.exe 实测 92MB（92,279,112 B），比 research-stack 预估 ~50MB 大一倍；内核 260MB+node 92MB+壳 8MB 解包 ≈360MB，D6「整体 <220–250MB」是否成立需新打包实测回填（M1 承诺未兑现） |
| ③ | 非纯 web 套皮 | ✅ | 判据满足：进程属主（Rust spawn/kill）、单实例、Job Object 防孤儿、端口管理、崩溃重启、退出清理全部原生 Rust；WebView2 仅作视口加载内核官方前端（http://127.0.0.1:<port>/）；内核零修改（官方 CLI 形态 + 官方 --patch 机制挂 quit/health） |
| ④ | 有安装包可安装 | ⚠️→❌（已提交产物） | NSIS 安装器存在（DSH Desktop_0.1.0_x64-setup.exe，3.44MB；currentUser → %LOCALAPPDATA%\DSH Desktop；HKCU 注册✓；卸载✓实测）。**但安装即坏**：资源被 NSIS 放到 `_up_\desktop\`（实测落位），`<exe>\desktop\` 不存在 → 运行时生成 patch 引用不存在的 quit.js/health.js → 内核 ERR_MODULE_NOT_FOUND → 崩溃×3 → 「内核反复崩溃」→ 应用不可用（详 P0-1）。且安装包不含 kernel/ 与 runtime/（干净机器无内核可用，依赖 %APPDATA%/npm 全局 dsh —— 本机恰好存在故未能直接演示） |
| ⑤ | 双击启动无需 shell | ✅（壳层，受④阻塞） | windows_subsystem="windows" 无控制台；Start-Process/双击 → 窗口「DSH Desktop」出现（MainWindowHandle 实测）；子进程 CREATE_NO_WINDOW 无黑框。🔶 干净机器上因④不可用（需 Node 在 PATH + npm 全局 dsh 或手改 settings） |

## 2. 运行时验证记录（DSH_HOME 隔离）

### 2.1 smoke（release exe，开发布局）— ✅ 通过
`DSH_HOME=<temp> dsh-desktop.exe --smoke` →
`SMOKE health=true port=7415 / SMOKE_OK port=7415 / EXIT=0`。
就绪契约与实测一致：stdout `dsh web: http://127.0.0.1:7415` → parse_port_from_line ✓（单测同款逻辑：cargo test 1/1 通过）。

### 2.2 GUI 启动 — ✅
- 窗口：dsh-desktop.exe（PID 14256）MainWindowTitle=DSH Desktop ✓
- 内核 spawn 契约完整：`../runtime/node.exe ../kernel/lib/bin.js web --patch %APPDATA%\com.dshdesktop.app\desktop.patch.yml --no-open --port 0` ✓（运行时生成 patch，引用 repo_root 推断的 desktop/quit.js、health.js）
- 动态端口：--port 0 → 内核监听 127.0.0.1:13797（实测 netstat）✓；GET /health → 200 "ok" ✓

### 2.3 单实例 — ✅
第二实例 Start-Process 0.29s 内退出；6s 后仅剩 1 个壳进程 + 1 个内核子进程（回调聚焦首实例，聚焦行为无法程序化断言）。

### 2.4 退出（用户路径：窗口 X）— ✅
taskkill（等效 WM_CLOSE）→ ExitRequested → ctl.stop → /quit → 内核优雅退出 → 壳退出；8s 后壳进程与内核 node 全部消失，无任何残留（Job Object KILL_ON_JOB_CLOSE + /quit 双保险生效）。

### 2.5 内核生命周期
- 崩溃重启：验证期间观察到 GUI 首起内核先退出一次 → 自动重启（退避 1s）→ 新内核就绪并重定向窗口 → 健康；上限 2 次后放弃并弹错误对话框（安装版崩溃×3 场景验证了「收敛放弃」路径）。
- 注意语义：任何内核退出（含外部触发 /quit、exit code 0 的优雅退出）都被计为「崩溃」进入重启队列（限次 2 次/600s）。属可接受设计，但建议对 exit code 0 的退出不计数（P3）。

### 2.6 更新流程 dry-run — ❌ 无实现
无任何代码可执行；registry 查询显示最新版 = 捆绑版（0.1.1-rc.2），无更新事件；settings 默认 auto_check_update=true 但无检查器。

### 2.7 安装包验证（已提交产物）— ❌ 安装即坏（复现）
静默安装（/S，currentUser）→ 目录 `%LOCALAPPDATA%\DSH Desktop\`：dsh-desktop.exe + uninstall.exe + `_up_\desktop\{quit,health}.js`（注意：**在 _up_ 下，不在 desktop/**）。
运行 `dsh-desktop.exe --smoke`（隔离 DSH_HOME）→ 内核 3 次启动失败 `ERR_MODULE_NOT_FOUND: Cannot find module 'C:\\Users\\34021\\AppData\\Local\\DSH Desktop\\desktop\\health.js'`（patch 指向 `<exe>\desktop\`，文件实际在 `_up_\desktop\`）→ `[kernel] crashed too many times, giving up` → **EXIT=0（假成功）**。诊断对话框出现（非 smoke 模式）。

### 2.8 构建与测试环境
- cargo test：1/1 通过（parse_ready_line），但直跑 git bash 必失败——已知坑复现：①/usr/bin/link（coreutils）劫持 MSVC link.exe；②无 vcvars 时缺 kernel32.lib（LNK1181）。解法：vcvars64.bat + MSVC bin PATH（本机 14.44.35207），验证可行。**签入 README 的构建指引**（当前 README 未提）。
- 评审期间检测到工作区 tauri.conf.json 被未提交修改（resources 对象映射，见 P0-1 修复建议），属团队并行改动，本报告按「已提交基线 + 未提交修复待验证」双状态记录。

## 3. 问题清单（严重度 + 修复建议）

### P0-1 安装产物不可用（资源落位错误 + 内核/Node 未捆绑）
- 现象：已提交产物安装后启动必失败（§2.7 全复现）；根因 a) tauri.conf.json `resources` 数组形式在 NSIS 下把 `../desktop/*.js` 落到 `$INSTDIR\_up_\desktop\`，而 kernel.rs 的 repo_root/fallback 找 `<exe>\desktop\`；根因 b) resources 未声明 `../kernel` 与 `../runtime/node.exe`，干净机器无内核可启动。
- 修复：①resources 改为对象映射（工作区已有未提交改动：`{"../desktop/quit.js":"desktop/quit.js", "../desktop/health.js":"desktop/health.js", "../runtime/node.exe":"runtime/node.exe", "../kernel":"kernel"}`，方向正确）；②更稳健的替代：quit.js/health.js 改为 include_str! 内嵌常量、运行时写入 data_dir（彻底摆脱安装目录布局推断与 Program Files 写权限问题）；③重建后必须重复 §2.7 全流程；④在 CI/交付清单加「安装→smoke→退出无残留」自动化门禁。

### P0-2 「跟随官方更新」完全缺失
- 现象：无 updater 代码；settings 3 个更新字段为死配置；README 过度声明。
- 修复：实现 ARCHITECTURE §5（registry dist-tags 检查 → tgz+integrity → kernel.new --help 冒烟 → 原子 rename 替换 → 重启 → 崩溃回滚 kernel.old）；若 M4 体量大，先交付最小闭环「启动后台检查 + 有更新时提示 + 手动下载替换」，并把 README 措辞改为「规划中」。

### P1-3 smoke 失败假成功（EXIT=0）
- 现象：安装版 3 次崩溃后无 SMOKE_OK 仍 EXIT=0（kernel.rs resolve 失败路径与崩溃放弃路径均走到 process::exit(0)）。
- 修复：smoke 失败返回非 0 退出码（如 2）；成功路径仍打印 SMOKE_OK port=N 后 exit 0。**上线后所有自动化验收都依赖此信号。**

### P1-4 托盘缺失（README/架构宣称 vs 实现）
- 现象：无 tray.rs；窗口关闭 = 整体退出，无 hide-to-tray/托盘菜单（显示/检查更新/退出均无）。
- 修复：M3 补 TrayIconBuilder + close→hide + 托盘菜单；或先行在 README 标注「托盘未实现」。

### P1-5 壳层诊断日志丢失（GUI 子系统无控制台）
- 现象：kernel.rs 全部 eprintln（ready port/崩溃计数/错误原因）在 windows_subsystem=window 下无输出地；仅内核 stderr 落 kernel.log（无 5MB 轮转，append 无限增长）。
- 修复：壳日志写 data_dir/logs/shell.log + 滚动轮转（ARCHITECTURE §2.3 已承诺）；Job Object assign 失败时至少记日志/告警（当前仅 eprintln，等于静默丢防孤儿）。

### P2-6 MODULE_TYPELESS_PACKAGE_JSON 警告（每次内核启动）
- 现象：desktop/ 无 package.json {"type":"module"}，node 每次加载 quit.js 打警告（实测日志 3 条）。
- 修复：desktop/ 放 package.json 并纳入资源；或 quit.js 改为 .mjs。

### P2-7 安装目录含空格（DSH Desktop）
- 现象：默认安装目录 "DSH Desktop" 含空格；file_url() 不做百分号编码（原始空格直出）；实测 node 在错误路径中已自行 %20 转义，未因空格失败（当前失败是因文件缺失），修复布局后需回归一次以确定无空格隐患。

### P2-8 体积指标未回填
- node.exe 92MB（预估 50MB）；整体解包 ~360MB 与 D6「<220–250MB」矛盾待实测；建议 README/ARCHITECTURE 回填真实数字并同步 D6 决策。

### P2-9 文档与实现脱节
- README 宣称「无控制台/托盘/跟随官方 npm 更新/性能好」——托盘与更新实际未实现；ARCHITECTURE §8 对照表「✅定稿」与实现状态不符。修复：README 补「当前里程碑」限制说明；对照表增加「实现状态」列。

### P2-10 构建指引缺失
- MSVC 构建必须 vcvars64（coreutils link 劫持 + LIB 缺失，本机双坑实测）；README 仅写 `cargo run`，new dev 或 CI 会直接踩坑。修复：README 加 vcvars64/构建环境说明（或 .cargo/config 固化 linker）。

### P3-11 崩溃计数语义
- 内核 exit code 0 的退出（未来的优雅退出路径）也被计入崩溃重启；建议按退出码区分，0 不计入。

## 4. 通过项（供 captain 决策保留）
1. 内核 spawn/就绪/端口契约全部与实测一致（动态端口、防保留段冲突）。
2. /quit 优雅停止 + Job Object 兜底实测通过；退出无残留（验收指标之一达成）。
3. 单实例（第二实例快速退出 + 聚焦回调）。
4. 限次崩溃重启（≤2 次 + 退避），超限收敛到错误对话框而非无限拉起。
5. 壳静态体积极小（exe 7.9MB / WS 27MB），无 Chromium，性能数据健康。
6. DSH_HOME 与内核解耦（DSH_HOME 环境变量/设置透传），隔离运行验证通过。
7. 单元测试（就绪行解析）通过；测试逻辑与真实输出一致。

## 5. 验收判定汇总
- ① 跟随官方更新：**❌ 未实现**（P0-2）
- ② 性能：**✅ 达标**（壳层；整体体积口径待回填，不影响判定基线）
- ③ 非纯 web 套皮：**✅ 达标**（架构判据）
- ④ 安装包可安装：**❌ 当前产物不可用**（P0-1 阻塞，工作区修复待验证）
- ⑤ 双击启动无需 shell：**✅ 壳层达标**（交付依赖④修复）

**结论：当前提交不可交付；预计修复 P0-1（打包）与 P0-2（更新）后重验，方可进入交付评审。**
