# DSH Desktop 验收评审报告（t6→t9 复核版）

- 评审人：reviewer（架构/质检/验收） · 日期：2026-08-29（t9 复核）
- **复核基线：HEAD = 7e0d79e（M3+M4：托盘+hide-to-tray+updater 接线与回滚）+ c3c1a37（evidence：内核解析路径日志）**，即评审起点 c914b07 之后的 M3/M4 集成状态。
- 环境：engineer 正在并行修复 P0（捆绑 npm）与 P1-3（smoke 退出码）——工作区 kernel.rs/main.rs/updater.rs/tauri.conf.json 有未提交改动，本报告按「已提交 HEAD 为准 + 两项标记修复中」记录，未改动任何 src。
- 产物：src-tauri/target/release/bundle/nsis/DSH Desktop_0.1.0_x64-setup.exe（20:57 构建，≈HEAD 代码，**未含 runtime/npm**——npm 于 21:01 才入树，属修复中项）。
- 全部实测使用隔离 DSH_HOME / 临时目录，未触碰用户 ~/.dsh；测试安装已卸载清理。
- 本版修订：①表头基线说明 ②①项判定（机制已实现、端到端受限）③已关闭项勾销（P0-1/P1-4/P0-2 机制版）④新增 P0-13 等缺陷。

---

## 0. 结论（一句话）

**M3（托盘/隐藏）与 M4（更新机制全链路）已提交并编译/单测/契约级验证通过；「全捆绑安装包」在 HEAD 产物上再次装机冒烟通过（SMOKE_OK port=11827，安装布局 bundled 路径解析证据成立）；hide-to-tray 实测（窗口关闭进程存活+内核保留）与 JobObject 硬杀回收实测通过。** 但代码走查发现 **P0-13：更新重启标志（restart）永不复位 → 更新后内核将被无限 kill/relaunch，更新功能端到端必坏**；另有「捆绑 npm 路径不匹配」（修复中）在工作区已见修复方向。因此 **①（跟随官方更新）判定：机制已实现（材料化依赖+sha512+isolated 冒烟+原子替换+崩溃回滚），但因 P0-13 与 npm 路径未通，端到端尚不可用——不能判「已实现」**，待修复+P0 重打包后官方出新包实测。

## 1. 五条要求逐项（复核版）

| # | 要求 | 判定（t6→t9） | 关键证据 |
|---|---|---|---|
| ① | 跟随官方更新内核 | ⚠️ 机制已实现 / 端到端受限 | HEAD：updater.rs 已 `mod updater` 接线 + 托盘「检查更新」触发；materialize_dependencies（npm install --omit=dev 物化依赖树，修复 tgz 无 node_modules 问题）+ sha512 integrity + 隔离 DSH_HOME 冒烟 + apply_swap/rollback + 崩溃循环自动回滚，cargo test 3/3（integrity/网络失败安全返回/就绪行）。**但**：①P0-13 restart 永不复位→更新后内核无限重启环路；②捆绑 npm 路径不匹配（updater 找 `<node_dir>/npm-cli.js`，实际为 `runtime/npm/index.js`，工作区修复中）；③registry latest=0.1.1-rc.2=捆绑版本，无更新可端到端触发（待官方出新版） |
| ② | 性能好 | ✅ | 壳 WS≈25-28MB、exe 8.35MB（HEAD，含托盘/更新代码）、无 Chromium；安装器 50.4MB（HEAD 产物实测）、解包 356MB/29,488 文件；启动到就绪 <12s |
| ③ | 非纯 web 套皮 | ✅ | 进程属主/单实例/Job Object/端口管理/崩溃重启/托盘/更新全原生 Rust；WebView2 仅视口 |
| ④ | 有安装包可安装 | ✅（HEAD 产物复验） | 20:57 全捆绑 NSIS（50.4MB）安装 → 布局 desktop/{quit,health}.js+kernel(260MB)+runtime/node.exe+exe+uninstall 正确 → 安装态 smoke `SMOKE health=true port=11827 / SMOKE_OK / EXIT=0` + **bundled 路径证据**（resolved node=…`DSH Desktop\runtime\node.exe bin=…`kernel\lib\bin.js）→ 卸载干净（目录+注册表）。注：该产物未含 runtime/npm（修复中项），重打包后需复验一次 |
| ⑤ | 双击启动无需 shell | ✅ | windows_subsystem 无控制台；GUI 启动窗口「DSH Desktop」；子进程 CREATE_NO_WINDOW；关闭→隐藏托盘（进程存活）；托盘「退出」为唯一退出路径 |

## 2. 复核实测记录（全部基于 HEAD）

1. **cargo test**（vcvars64 环境）：3/3 通过——kernel::parse_ready_line ✓ updater::integrity_roundtrip ✓ updater::check_latest_none_on_network_fail ✓。
2. **dev 布局 --smoke**（20:57 release exe，隔离 DSH_HOME）：`SMOKE health=true port=5439 / SMOKE_OK / EXIT=0`；新 evidence 行 `[kernel] resolved node=…`runtime\node.exe bin=…`kernel\lib\bin.js` ✓（c3c1a37 验收证据落地）。
3. **updater 冒烟契约**：`runtime/node.exe kernel/lib/bin.js web --help` → exit 0 ✓（隔离 DSH_HOME，smoke_kernel 判据成立）。
4. **M3 托盘/hide-to-tray**（GUI 运行）：窗口出现；WM_CLOSE（=点 X）→ **进程存活（PID 24432）+ 内核子进程（7256）保留 = 隐藏到托盘 ✓**；托盘由 build_tray（TrayIconBuilder+show/check/quit 菜单+TrayHandle 保活）在 setup 中构建，进程存活即构建成功（失败会 setup Err 退出）；菜单点击无法程序化触发，为代码级验证（show→聚焦 / check→spawn_update / quit→ctl.stop）。
5. **JobObject 硬杀回收**：`taskkill /F` 杀壳 → 4s 内内核 node 消失，无残留 ✓（防孤儿兜底再次实证）。
6. **安装包（HEAD 产物）**：/S 安装 → 布局正确 → 安装态 smoke `SMOKE_OK port=11827` + bundled 路径证据 ✓ → uninstall /S 干净（目录+注册表均除）✓。
7. **更新流程代码走查**（关键）：
   - materialize_dependencies：tgz 骨架（33KB）→ `npm install --omit=dev --no-audit --no-fund` 物化 ~250MB 依赖树 → 冒烟 → 原子替换 → 崩溃回滚——机制链条补齐（t6 的 P0-2「tgz 无 node_modules 必败」已在机制上关闭）。
   - **P0-13（新发现）**：`ctl.restart` 仅在 spawn_update（main.rs:107）`store(true)`，全工程无任何 `store(false)`；内核线程 Restart 分支处理完 take(update_path)+apply_swap 后 continue，下一轮 launch_once 阶段2 立即读到 restart=true → graceful_stop → 再次 Restart（此时 update_path 已 None，跳过 swap）→ **无限「spawn→杀→spawn」环路**，更新永远无法稳定运行（每次循环 ~就绪耗时）。
   - **npm 路径不匹配（P0 修复中）**：HEAD 的 materialize_dependencies 找 `node.parent()/npm-cli.js`（runtime/npm-cli.js）——实测不存在；实际入口 `node runtime/npm/index.js install…`（实测 --version → 11.13.0 ✓）。故捆绑环境回退 `cmd /C npm`（干净机器无 npm → 更新失败）。工作区已见整改（runtime/npm 入树 + resources 加 `../runtime/npm`）。
   - apply_swap/rollback：rename 序列 + 验证 + 失败自动回退 ✓（代码级）；崩溃循环回滚：`cur==uv && crash_count>=2`（600s 窗口）✓（存在旧崩溃污染计数的边界，见 P3-11）。
8. **registry 状态**：latest=0.1.1-rc.2=捆绑版本 → 「检查更新」路径当前结果为「已是最新版本」；无端到端更新事件可测（待官方出新版，或用 mock registry 集成测试）。

## 3. 问题清单（复核版）

### ✅ 已关闭（勾销自 t6）
- **P0-1 安装产物不可用** → 28d3e67 全捆绑修复，t9 复核再次装机冒烟通过（§2.6）。
- **P1-4 托盘缺失** → M3 已实现（TrayIconBuilder+菜单+hide-to-tray 实测，§2.4）；README/架构声明与实际相符。
- **P0-2「跟随官方更新完全缺失」** → 机制级已实现（§2.7 链条），但整体判定见①/P0-13。
- **P2-9 文档过度声明** → 已缓解（README 已描述更新实现；**残留**：README「当前里程碑 M1+M2，t4」仍是旧值，需更新为 M1-M4——P2-E）。

### 🔧 修复中（engineer 并行，勿动）
- **P0-A 捆绑 npm 闭环**：runtime/npm 已加入 + tauri.conf resources 已加 `../runtime/npm`；但 20:57 产物未含 npm、updater 引用路径待改为 `runtime/npm/index.js`（或捆绑 npm-cli.js）；完成标志=重打包 + 装机冒烟 + 更新 dry-run 通过。
- **P1-3 smoke 失败假成功（EXIT=0）**：HEAD main.rs 仍 `process::exit(0)`（成功/失败路径同码）；工作区已修改，待复验：失败路径 exit≠0、成功打印 SMOKE_OK 后 exit 0。

### ❌ 新增 P0-13：更新后无限重启环路（P0，更新即触发）
restart 标志无复位点（全工程仅有 store(true)）；导致 apply_swap 后新内核立刻被停、无限循环重启。**修复建议**：在 run_shell 的 Restart 分支完成 update_path take 后执行 `ctl.restart.store(false, SeqCst)`（或 launch_once 返回 Restart 前置 false）；并补回归测试（模拟 restart=true → 期望只重启一次后 restart 复位）。

### ❌ 开放（P2/P3）
- **P1-5 壳诊断日志无落盘**（开放）：GUI/double-click 下 eprintln 全部丢失（本轮 smoke 经 bash 继承句柄可见，双击场景不可见）；kernel.log 无轮转。建议 shell.log+5MB 轮转；JobObject assign 失败需显式告警。
- **P2-6 MODULE_TYPELESS_PACKAGE_JSON 警告**（开放）：desktop/ 无 package.json {"type":"module"}，每次内核启动警告；建议加 package.json 资源或改 .mjs。
- **P2-7 file_url 空格编码**（开放）：安装目录 "DSH Desktop" 含空格，file_url 不做百分号编码；建议主动 encode。
- **P2-8 体积口径**（开放）：实测安装器 50.4MB/解包 356MB/29,488 文件；README「200-250MB」与 D6「<220-250MB」口径均与实际不符（安装器口径 50MB 达标）；建议回填实测 + 明确口径（另：内核未裁剪，后续按 §5.3 再降）。
- **P2-10 构建指引**（开放）：cargo test/run 需 vcvars64（coreutils link 劫持+LNK1181），README 未写。
- **P3-11 崩溃计数边界**（开放）：更新后回滚判定 `crash_count>=2` 含更新前崩溃（600s 窗口混计），可能提前回滚；建议按「更新后」单独计数。
- **P3-12 updater 测试覆盖**（开放）：仅 2 测；未覆盖 materialize/smoke/apply_swap/rollback/restart 标志（若补测可提前抓出 P0-13）；建议 mock registry 集成测试。
- **P2-16 settings 死字段**（开放）：update_channel/keep_old_kernel 仍未接入（updater 硬编码 latest；apply_swap 无条件清理旧 kernel.old，与 keep_old_kernel=true 默认违背）。

## 4. 通过项（复核后保留）
1. 内核 spawn/就绪/端口契约与实测一致；bundled 路径解析（开发/安装两布局）证据落地。
2. /quit 优雅停止 + JobObject 兜底；退出/硬杀均无残留（两轮实测）。
3. 单实例（第二实例快速退出+聚焦回调，t6 实测）。
4. 限次崩溃重启 + 更新后崩溃自动回滚（代码级验证）。
5. M3：托盘构建成功、hide-to-tray 实测（关闭≠退出）、托盘菜单接线完整。
6. 全捆绑安装包装机可用：布局正确、安装态 smoke 通过（port=11827）、卸载干净。
7. 壳静态体积极小（8.35MB exe/25-28MB WS），无 Chromium；updater 材料化依赖+sha512+isolated 冒烟+原子替换+回滚链条成立。
8. cargo test 3/3（含 updater 单测）；web --help 冒烟契约实测 exit 0。

## 5. 判定汇总（t9 复核版）
- ① 跟随官方更新：**⚠️ 机制已实现并验证（单测/契约/代码走查），端到端不可用**：P0-13（重启环路）+ P0-A（捆绑 npm 路径，修复中）→ 待修后官方出新包实测（建议同时用 mock registry 跑通全链路集成测试）。
- ② 性能：✅ 达标（数字见 §1）。
- ③ 非纯 web 套皮：✅ 达标。
- ④ 安装包可安装：✅ 达标（HEAD 产物复验；P0-A 重打包后需再复验一次）。
- ⑤ 双击启动无需 shell：✅ 达标（含 hide-to-tray 语义）。

**下一步建议**：①engineer 完成 P0-A+P1-3 → 重打包 → 复验（安装+smoke+退出码）；②修 P0-13（1 行复位 + 1 条回归测试）；③mock registry 或等官方新版做端到端更新验收；④README 里程碑/体积数字同步（P2-8/P2-E）。
