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

---

# v0.2 复核（t13 · 终审）

- 基线：HEAD = **c3630af（终）+ 195369b（F 内存看门狗）+ 840106d（破坏性更新保护/性能/降级）+ c32483b（并发守卫/数据独立/自启深链）+ b56df65（虎鲸图标）+ fe4eb30（P0-13/P0-A/P1-3 修复）**；195369b 与 c3630af 于评审窗口内落地（F 看门狗与 loading 失败路径修复——后者即本报告 P2-18 的上游修复）；本报告验证基线=上述全量 HEAD（初始构建与装机验证在 840106d+F 树上完成，最后以 c3630af 终版重打包复验，见文末补记）。
- 验证环境：vcvars64（LIB/INCLUDE 正斜杠）；所有运行时测试使用隔离 DSH_HOME（测试发现 shell 环境自带 DSH_HOME 环境变量，并发守卫测试需显式 env -u DSH_HOME 才命中真场景）。
- 重打包：npx tauri build 成功 → **DSH Desktop_0.2.0_x64-setup.exe = 52,185,310 B（≈49.8 MiB）**；exe 8,354,816 B。

## v0.2 判定表（对照 t13 任务书 §1-§5）

| 验收点 | 判定 | 证据 |
|---|---|---|
| §1 并发守卫无误杀/不漏检 | ✅（含一处低危） | 复算 wmic 解析：检测到 npx 宿主+真内核（2 命中）；自身排除=按自身内核路径子串；端到端实测：`env -u DSH_HOME DSH_DESKTOP_GUARD_ANSWER=no` → `[guard] foreign kernel detected, user declined` → EXIT=0、无残留 shell/内核。⚠️P2-17：guard.rs 命中格式化 `&cmd[..117]` 字节切片在中文路径 >120 字符时会 panic（UTF-8 非边界），建议 chars().take() |
| §1 更新兼容保护（smoke_kernel_with_patch 失败不换版本+清 kernel.new） | ✅（代码级） | prepare_new_kernel：with-patch 冒烟失败 → `remove_dir_all(kernel.new)` + 返回「已保留当前版本」（updater.rs 236-244 行）；冒烟含 就绪30s→/health 200→/quit→≤10s 全契约 |
| §1 降级路径（无 patch 仅非 smoke；graceful_stop 语义） | ✅ | 崩溃超限（非 smoke）→ degraded_flag=true → 无 patch 重试一次；degraded 警告弹一次（swap 守卫）；graceful_stop 在 degraded 时跳过 /quit 直接树杀；smoke 永不降级（exit_code=2 保持非零语义） |
| §1 立即窗口失败路径（loading 页错误提示） | ✅（c3630af 已补失败路径） | loading.html 窗口 503ms（dev）/524ms（装机）出现 ✓；`set_loading_status` 经 `w.eval` 注入 #status：崩溃超限/spawn 失败/就绪超时 三路径有提示；残余 P3-27（resolve 失败仅原生对话框） |
| §1 additional_browser_args 保留默认串 | ✅ | 自定 args 显式含 wry 默认串 `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` + 防节流 4 项 + 按配置追加 --disable-gpu；wry 默认参数由 Tauri 注入不受影响 |
| §1 settings 旧文件向后兼容 | ✅ | `#[serde(default)]` + 全字段 Default；实测 8 字段旧 settings.json 启动 `--update-check` 不炸、退出码 0 |
| §2 cargo test | ✅ 6/6 | memory_warn_hysteresis / guard×2 / updater×2 / parse_ready_line |
| §2 dev --smoke exit0 | ✅ | SMOKE health=true port=7986 / SMOKE_OK / EXIT=0（含 degraded=false 证据行） |
| §2 坏路径 exit2 | ✅ | settings.kernel_path=X:/nope + --smoke → `no ready port` → EXIT=2（P1-3 复验：失败非零） |
| §2 并发守卫实测 | ✅ | 上述 `env -u DSH_HOME` + answer=no → exit0 无残留；真实宿主（npx+内核）均被检出（提示框内容含 pid 列表——代码/测试钩子验证） |
| §2 托盘全菜单 | ✅（代码级） | show/check/独立数据模式/开机自启/退出 五项菜单；构建成功（setup 失败即 app 退出）；菜单点击无法程序化触发（t10 已实测 data/autostart 往返） |
| §2 深链 start dsh-desktop:// | ✅ | 运行中 `Start-Process 'dsh-desktop://test/abc'` → 实例数保持 1（第二实例快速退出+聚焦）；协议注册幂等（启动自写 HKCU Classes；卸载不清理=设计，README 记录；本轮测试后已清残留） |
| §2 自启 reg query | ✅ | `--set-autostart 1` → HKCU Run「DSH Desktop」REG_SZ=exe✓；`--set-autostart 0` → 键消失；往返 OK |
| §2 独立数据模式切换 | ✅ | `--set-independent 1` → INDEPENDENT_SET=isolation + settings.dsh_home=…/dsh-home + 目录创建；`--set-independent 0` → shared；GUI 切换路径=托盘复选→保存→restart→内核线程每轮重载 settings（代码级）；t10 已实测往返 |
| §3 重打包+装机 | ✅ | 安装 31,363 文件 / 372MB；runtime/node_modules/npm ✓（P0-A 入包）；安装态 smoke `SMOKE_OK port=6659`（bundled 路径解析）；双击 524ms 出 loading 窗口；内核就绪 → /health ok；关闭→隐藏（托盘）；硬杀无残留；卸载干净（目录+注册表）；**安装包 49.8 MiB（52.2MB）** |
| §3 图标=黑色虎鲸 | ✅ | read_image 目检：512px 纯黑虎鲸（官方 favicon 源，fill=#000）✓；icon.ico 18.6KB/icon.png 23.7KB（b56df65 全套 50 文件） |
| §4 dsh-plugin-manager | ✅（含交付 diff 核验） | unit.mjs **54 断言 ALL PASSED**（本机复跑）；走查：结构不认识拒绝编辑、行级文本编辑（其余字节不变）、备份 keep10+原子写、核心 bundle 保护、patch 层只读、origin 围栏、add 绝对路径+lib/index.js 校验；**注册 diff 干跑核验：addBundle(真实 manifest) → JSON 有效、位置=session-manager 之后、dependencies 零改动、纯增 1 行+末项补逗号；未应用**（真实 manifest 不变，junction 已建） |
| §5 REVIEEW 追加 | ✅ | 本章节 |
| P0-13 复位 | ✅ | Restart 分支 `restart.store(false)` + launch_once `consume_restart()`（swap 双保险）；更新流不可端到端实测（无新版） |

## v0.2 问题清单（新增）

- **P2-17 并发守卫命中格式化 panic 风险**（中）：`&cmd[..117]` 字节切片；中文路径且命令行 >120 字符即 panic（GUI 无声崩溃）。修复：`chars().take(117)` / char_indices。
- **P2-18 loading 页无错误提示** → ✅ **已修复（c3630af，评审窗口内上游落地）**：`set_loading_status`（run_on_main_thread + `window.setStatus(...)` 注入 #status），覆盖崩溃超限放弃 / spawn 失败 / 就绪超时 三路径；**残余 P3-27**：resolve 失败（内核未找到）路径仍仅原生对话框、loading 页停留原文案（低）。
- **P1-5 壳日志无落盘**（保持开放）：GUI 启动时 eprintln 全丢（本轮 dialogs 可见但日志不可查）；JobObject assign 失败仅 eprintln（曾观测到一次 toolbox 环境孤儿 node=11052，已清；真实双击场景 assign 均成功→树级回收有效，但失败无告警）。建议 shell.log + assign 失败显式告警。
- **P2-16 settings 死字段**（保持开放）：update_channel/keep_old_kernel 仍未接入（check_latest 硬编码 latest；apply_swap 无条件覆盖 kernel.old）。
- **P3-24 冒烟输出污染**（低）：--smoke 尾部出现 wry `Failed to unregister class Chrome_WidgetWin_0 (1411)` stderr（无窗口 smoke 的 WebView2 环境清理噪音，不影响退出码）。
- **P3-25 守卫自排除标记（开发布局）**（低）：guard 的 own marker=`<exe_dir>/kernel`，开发布局下实际内核路径为 repo_root/kernel（../..），陈旧内核孤儿会当「外来」提示——语义可接受（确为数据冲突）但建议统一用 resolve_kernel_root。
- **P3-26 更新流端到端未测**：registry latest==捆绑版（0.1.1-rc.2），with-patch 冒烟/交换/回滚为代码+单测级验证；建议 mock registry 集成测试后等官方新版实测。

## v0.2 结论

**五特性（并发守卫/数据独立/自启深链/更新兼容保护/性能项）+ 插件管理 + 图标全部通过终审（P2-18 已由 c3630af 上游修复，仅剩 P2-17 待修 + P3-27 残余），重打包装机全链路验证通过，安装包 52.2MB（49.8 MiB），五条用户要求维持 ①⚠️（机制全链路已实现，P0-13 已修；端到端待官方新版）②✅ ③✅ ④✅ ⑤✅。** 建议：P2-17/P2-18 列入 v0.2.1 前修；mock registry 集成测试作为 t14 候选。
