# DSH 桌面版架构设计（ARCHITECTURE）

- 作者：engineer-main（工程师·桌面实现） · 团队任务 t3
- 日期：2026-08-29
- 上游输入：
  - research-core.md（t1 · DSH 内核嵌入能力调研，全链路实测）
  - research-stack.md（t2 · 桌面栈选型：Tauri 2.x）
  - 两份调研原稿位于团队工作区 .agent-teams/dsh-desktop/ 下
- 范围：最终架构综合——目录结构、进程模型、端口与生命周期、版本跟随、更新与回滚、配置与数据路径、五条要求逐条对照表，及施工契约与决策点。
- 状态：设计定稿（待评审）；「决策点」需 captain/用户拍板后才完全冻结。

---

## 0. 一句话架构

> **Rust（Tauri 2.x）原生壳 + WebView2 视口**：壳以「进程属主」身份直接 spawn 捆绑的 node.exe 运行捆绑的官方内核 @deepseek-ai/dsh（web profile，本地 HTTP 服务），窗口只加载 http://127.0.0.1:<动态端口>/（官方随包分发的前端 dist）；停止走自定义 /quit 路由（Windows 下唯一优雅退出路径）；版本跟随 = Registry 检查 → 下载 tgz → 原子替换 <install>/kernel/ → 重启，**DSH_HOME（用户数据）全程不动**。

---

## 1. 分层架构

```
┌──────────────────────────────────────────────────────────────┐
│  用户可见层                                                    │
│  WebView2 窗口（加载官方前端 http://127.0.0.1:<port>/）         │
│  托盘图标（核心 API TrayIconBuilder）+ 深链（可选 plugin）      │
├──────────────────────────────────────────────────────────────┤
│  原生壳层（Rust / Tauri 2.x）★ 本产品全部自研代码所在           │
│  ├─ kernel.rs        内核子进程管理：spawn/就绪/quit/重启/Job   │
│  ├─ updater.rs       版本检查/下载/原子替换/回滚                │
│  ├─ tray.rs          托盘与窗口生命周期（hide-to-tray）         │
│  ├─ single_instance  单实例互斥（命名互斥体 / 官方 plugin）      │
│  └─ settings.rs      壳层配置（端口策略/更新通道/DSH_HOME 开关）│
├──────────────────────────────────────────────────────────────┤
│  内核子进程（官方产物，零修改）                                  │
│  node.exe <kernel>/lib/bin.js web --patch desktop.patch.yml    │
│    --no-open --port 0                                          │
│  → Cordis 插件树 + HTTP/WS 服务（127.0.0.1 单机监听）           │
│  → 官方前端 dist、/api RPC、/api/events.* WebSocket             │
├──────────────────────────────────────────────────────────────┤
│  数据层（与内核版本完全解耦）                                    │
│  DSH_HOME（默认 ~/.dsh）：profiles/ sessions/ storages/ 等      │
│  <AppData>/MyDsh/        壳层自己的日志/设置（非 DSH_HOME）      │
└──────────────────────────────────────────────────────────────┘
```

关键性质：
1. **壳零前端维护**：@deepseek-ai/dsh 包内携带官方前端 dist（@deepseek-ai/dsh-web-frontend，随 npm 包分发），内核更新 = UI 更新。
2. **内核零修改**：内核以官方 CLI 形态被启动，不 fork、不改源码；唯一自定义是 --patch 挂载的 quit/health 插件（几十行，官方机制）。
3. **数据零迁移**：用户数据全部在 DSH_HOME，内核包可整个替换（详见 §5–§7）。

---

## 2. 目录结构

### 2.1 源码仓库布局（目标产出之一：源码仓库）

```
dsh-desktop/                        # 仓库根
├── src-tauri/                      # Rust 壳
│   ├── src/
│   │   ├── main.rs                 # #![windows_subsystem = "windows"]；入口/事件循环
│   │   ├── kernel.rs               # ★ 内核进程管理（属主）：spawn/就绪/quit/重启/Job Object
│   │   ├── updater.rs              # ★ 版本检查/下载/冒烟/原子替换/回滚（§5/§6）
│   │   ├── tray.rs                 # 托盘菜单：显示/检查更新/关于/退出
│   │   ├── settings.rs             # 壳层配置读写（§7）
│   │   └── crash.rs                # 崩溃检测+限次重启+降级错误页（§4.4）
│   ├── tauri.conf.json             # 窗口/打包配置；URL 启动后 set（动态端口）
│   ├── capabilities/               # 最小权限（spawn 全在 Rust 侧，不依赖 shell 插件）
│   └── Cargo.toml
├── desktop/                        # 随包分发内核补丁（构建时把 __INSTALL__ 占位符替换为绝对路径）
│   ├── quit.js                     # GET /quit → ctx.get("appExit")(0)
│   ├── health.js                   # GET /health → 200（心跳/自检，可选）
│   └── desktop.patch.yml           # 引用 file:///<install>/desktop/quit.js、health.js
├── runtime/                        # 构建期放入：捆绑 node.exe（官方 x64 zip，≥22 推荐 24）
├── kernel/                         # @deepseek-ai/dsh@<pin> 解包产物（构建产物，gitignore）
├── installer/                      # NSIS 定制（图标/快捷方式/卸载/协议注册）
├── docs/ARCHITECTURE.md            # 本文档
├── .github/workflows/release.yml   # （M5 可选）构建+发布
└── package.json / Cargo.lock
```

### 2.2 安装目录布局（NSIS 安装后）

```
<InstallDir>（缺省 %LOCALAPPDATA%\Programs\MyDsh 或 Program Files，安装器决策）
├── MyDsh.exe                        # 壳主程序（release + strip + lto，约 10MB）
├── runtime/node.exe                 # 捆绑 Node（无系统依赖）
├── kernel/                          # ★ 当前内核版本（@deepseek-ai/dsh 解包，含官方前端）
│   ├── lib/bin.js
│   └── node_modules/……
├── desktop/
│   ├── quit.js  health.js
│   └── desktop.patch.yml            # file://<InstallDir>/desktop/…（安装器写死）
├── kernel.old/                      # 更新时暂存旧版（回滚用）
├── uninstall.exe                    # NSIS
└── （更新临时目录 kernel.new/ 只存活于更新过程）
```

> 安装包体积事实（实测）：内核全量 260MB / 29,483 文件；node.exe 约 50MB。**安装包总体积 ≠ 壳层体积**——「壳 ≤15MB」只适用于纯壳；整体目标见 §9 决策点 D6（内核按 win32-x64 裁剪后争取整体 <220–250MB）。

### 2.3 数据目录（DSH_HOME 与壳层自管目录分离）

```
DSH_HOME（默认 ~/.dsh，环境变量可覆盖；★ 与官方 CLI 共用、多版本兼容）
├── profiles/
│   ├── node_modules/               # 平面 fallback 符号链接闭包（内核启动时自愈重建）
│   └── web/                        # 用户 profile（package.json/cordis.patch.yml/外部插件）
├── sessions/                       # 会话日志（session.jsonl.zstd，按 workspace-hash/<id>/）
├── storages/                       # credentials.json / settings / workspace.json
├── cordis.patch.yml                # home 级用户 patch
└── skills/  plugins/ 等

<AppData>/MyDsh/                    # ★ 壳层自管（不属于 DSH_HOME）
├── settings.json                   # 壳配置：更新通道/端口策略/DSH_HOME 覆盖/telemetry
├── logs/kernel.log                 # 内核 stderr 滚动日志（5MB 轮转）
└── locks/                          # 单实例互斥、更新锁
```

---

## 3. 进程模型（桌面宿主 + DSH 核心子进程）

### 3.1 进程清单

| 进程 | 谁启动 | 说明 |
|---|---|---|
| MyDsh.exe | 用户/开机自启 | Rust 壳；单实例互斥；**进程属主** |
| node.exe … lib/bin.js web | Rust spawn（**跳过 npx**） | 真正内核；只监听 127.0.0.1 |
| 内核再 spawn 的孙进程 | 内核 | 工具持久 shell（bash/pwsh）、node-pty、workflow worker —— **必须按进程树/Job Object 清理** |

> 实测官方实例是 npx-cli.js @deepseek-ai/dsh web（转发层）+ 真正内核两层；壳直接 spawn lib/bin.js，省一层进程、避免 npx 网络检查。

### 3.2 属主与无控制台

- 主程序：#![windows_subsystem = "windows"]（杜绝黑框）。
- 子进程：std::os::windows::process::CommandExt::creation_flags(CREATE_NO_WINDOW = 0x0800_0000)（标准库自带，不依赖插件）。
- stdout 管道捕获（就绪契约），stderr 重定向到 <AppData>/MyDsh/logs/kernel.log。
- 环境：继承 + 追加；DSH_TELEMETRY_DISABLED=1（默认关闭遥测）；DSH_HOME 按 §7 策略注入。
- **Job Object（防孤儿正确机制）**：把 node 进程放入 Job Object 并设 JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE——app 进程无论怎么死，Windows 自动回收内核整棵进程树（Rust win32job/windows-sys 数十行实现）。这是「退出后任务管理器无残留 node」验收的兜底保证。

### 3.3 单实例

- 命名互斥体（或 tauri-plugin-single-instance）：第二实例启动 → 聚焦/恢复已有窗口后退出；深链（若启用）的 args 经官方回调桥接到首实例（注意 plugin 文档明示的 argv 校验模式）。

---

## 4. 端口与生命周期（启动/停止/崩溃重启）

### 4.1 端口策略（★ 决策点 D2）

- **默认：--port 0**（OS 分配空闲端口）+ 从 stdout 解析就绪行拿端口——规避「默认 3080 落入 Hyper-V/WinNAT 保留段 → EACCES」问题（本机实测保留段动态变化，用户 profile 正是因此改 3379）。
- 可选「固定端口」模式（设置项，便于外部浏览器直达）：启动前用 netsh int ipv4 show excludedportrange + TCP 探活双重校验；命中保留段/被占用则回退 --port 0 并提示。
- **禁止 --host 0.0.0.0**（内核刻意拒绝；暴露网络 = 远程代码执行风险面）。

### 4.2 启动时序（Rust 属主）

```
[Stopped]
  │ 单实例检查 OK；读 settings.json（端口策略/DSH_HOME/更新通道）
  ├─ spawn: <runtime>/node.exe <install>/kernel/lib/bin.js web
  │        --patch <install>/desktop/desktop.patch.yml   ← ⚠ launcher flags 必须在应用 flags 之前
  │        --no-open --port 0
  │        creation_flags=CREATE_NO_WINDOW, stdout=piped, stderr=log文件
  ├─ 行缓冲读 stdout，正则 /^dsh web: http:\/\/127\.0\.0\.1:(\d+)/m
  │     → 拿到 port → WebView 导航 http://127.0.0.1:<port>/
  │     （启动失败行 dsh: … failed to load / fatal load failure → 记日志 → 错误面板）
  ├─ timeout 30s 无就绪行 → 视为启动失败 → kill 树 → 错误面板（含日志路径、点击重试）
  └─ 附带：把 <port> 与进程句柄存入状态；stderr 落 5MB 滚动日志
[Running]
```

### 4.3 停止时序（优雅优先，硬杀兜底）

```
[Running] 用户点退出 / 托盘退出 / 更新需要重启
  ├─ GET http://127.0.0.1:<port>/quit   → 200 bye
  │     内核走 ctx.get("appExit")(0) → 5s 宽限 dispose（webserver close+closeAllConnections）
  ├─ 等 child 退出 ≤8s
  ├─ 超时 → 树级硬杀（TaskTree/Job Object terminate）——★ 不能只用单 PID kill
  └─ 退出前校验内核已死（进程树无 node 遗留）
[Stopped]
```

> ★ Windows 关键事实：process.kill / Taskkill /F = TerminateProcess 硬杀，**不触发**内核 JS 信号处理器；内核**没有**任何 HTTP 关闭端点（全树 grep 实测）。唯一优雅路径 = 自带 quit 插件。quit.js 契约见 §10（file:/// 绝对路径模块名，Windows 裸路径会 ERR_UNSUPPORTED_ESM_URL_SCHEME）。

### 4.4 崩溃检测与自动重启

| 事件 | 判别 | 动作 |
|---|---|---|
| 内核进程退出 | Child::try_wait / exit 事件 | 记录退出码 + 最后 100 行日志 |
| 卡死（可选加强） | 每 5s GET /health，连续 3 次失败 | 视为 hung → 树杀 → 进入重启流程 |
| 自动重启 | —— | **同一版本连续崩溃 ≤2 次**（指数退避 1s/5s）；达上限 → 托盘气泡 + 错误面板（重试/检查更新/打开日志），不再自动拉起 |
| 更新后崩溃循环 | 新版本 10 分钟内崩溃 ≥2 次 | **自动回滚 kernel.old 并重启**（§6.2），通知用户 |

### 4.5 生命周期状态机

Stopped → Starting → Running ⇄ (crash)→ Restarting(限次) → Running
        ↘ Failed(错误面板，手动重试)
Running → Stopping(/quit) → Stopped
Running → (更新) → Stopping → [替换 kernel] → Starting → Running

---

## 5. 版本跟随策略（检查 npm → 下载 → 原子替换 → 重启）

### 5.1 事实底座（实测）

- 内核版本 = npm 包版本：https://registry.npmjs.org/@deepseek-ai/dsh → dist-tags:{"latest":"0.1.1-rc.2","next":"0.1.1-rc.2"}；**无 engines 字段**（Node 下限自理，捆绑 ≥22、推荐 24 LTS 系）。
- **用户数据与内核版本解耦**：DSH_HOME 下 profiles/sessions/storages 与安装位置无关；每次启动内核自动 healProfilesModuleFallback()（按当前安装重建 profiles/node_modules 平面符号链接闭包）+ normalizeShippedProfile()（bundles 规范化回官方元组）——**换内核 = 替换包目录，数据零迁移**。这就是「跟随官方更新」的机制底座。

### 5.2 更新流程（updater.rs）

```
① 检查（app 启动后台 + 每 24h + 手动「检查更新」）
    GET https://registry.npmjs.org/@deepseek-ai/dsh
    → dist-tags[channel]（channel 设置项：latest 默认 / next 可选）
    → 与 <install>/kernel/package.json 的 version 比对
    → 不相等（容忍 -rc.x：按同一 dist-tag 比较）→ 提示更新

② 下载与校验
    GET https://registry.npmjs.org/@deepseek-ai/dsh/-/dsh-<ver>.tgz
    → integrity（registry 返回的 sha512）校验
    → 解压（tar+flate2）到 <install>/kernel.new/（tgz 内 package/ 平铺）

③ 冒烟校验（防「坏版本上机」）
    <runtime>/node.exe <install>/kernel.new/lib/bin.js web --help
    隔离 DSH_HOME（临时目录）+ 退出码 0 即通过
    （--help 在 launcher 阶段解析，无需 --patch）

④ 原子替换（先停内核）
    /quit → 等退出（§4.3）
    rename kernel → kernel.old ; rename kernel.new → kernel
    验证 kernel/lib/bin.js 存在且 package.json.version == 目标版本

⑤ 重启内核（§4.2 时序）→ 加载新内核/新前端

⑥ 收尾
    更新成功且运行稳定（≥10 分钟无崩溃）→ 删除 kernel.old（或保留 1 份，§6.2）
    任一步失败 → 进入 §6.2 回滚
```

### 5.3 尺寸与裁剪（发布时）

- 内核全量 260MB；打包按平台剪 node-pty/prebuilds/（只留 win32-x64）、koffi 只留 win32-x64，保守可再评估 --omit=dev 与 SDK 层；目标整体安装包 <220–250MB（§9 D6，M1 实测回填）。
- 内核包依赖闭包自足（无需 pnpm/npm 即可运行，实测），捆绑拷贝即用。

---

## 6. 更新与回滚

### 6.1 安全性设计

| 关注点 | 机制 |
|---|---|
| 一致性 | 原子替换（两条 rename，无半成品状态）；替换前停内核、替换后验证 |
| 并发 | 更新期间持有更新锁；单实例互斥保证只有一个壳进程；DSH_HOME 不受更新影响 |
| 联网失败 | 下载失败/校验失败 → 保留现有 kernel 不动，提示重试 |
| 坏版本 | 冒烟校验（--help）+ 启动崩溃循环检测（10 分钟 2 次 → 自动回滚） |
| 磁盘空间 | 更新前检查 ≥ 2×内核体积的空闲空间 |

### 6.2 回滚策略

- **立即回滚**：步骤④/⑤失败 → 若 kernel 已替换但起不来：rename kernel → kernel.bad，kernel.old → kernel，恢复旧版本并重启，随后删除 kernel.bad。
- **运行期回滚**：新版本 10 分钟内崩溃 ≥2 次 → 自动回滚 kernel.old + 重启 + 用户通知（附新旧版本号）。
- **手动回滚**：设置页「回退到上一版本」→ 同上流程（依赖 kernel.old 尚在）。保留策略：默认保留最近 1 份旧版本；可配置「更新成功后立即删除」以省空间。
- **数据安全**：整个更新/回滚过程不触碰 DSH_HOME（sessions/storages/profiles），任何版本组合下用户数据一致。

---

## 7. 配置与数据路径（DSH_HOME 保持 ~/.dsh，多版本兼容）

### 7.1 默认策略：共享 ~/.dsh

- 桌面包不设 DSH_HOME（即默认 ~/.dsh）→ 与官方 CLI/GUI 共享 credentials/providers/settings/会话，用户无缝切换。
- 首启零配置：web profile 由内核模板自动初始化（无需 npm/pnpm，bundles 直接解析安装闭包，实测）。
- **多版本兼容（自动）**：内核每次启动的自愈机制（§5.1）保证不同内核版本先后跑同一 ~/.dsh 均正常；INSTALLATION_OWNED_PROFILE_TUPLES 在 bundles 变化时规范化用户 profile。

### 7.2 风险与护栏（共享模式注意点）

- **单实例锁**：壳内单实例（§3.3），避免双开同一 profile 写同一会话文件。
- **与官方 CLI 并行**：CLI 与桌面版同时活跃同一 web profile 时可能并发写同一 sessions 文件 → 文档明示「不建议同时使用」；桌面包默认窗口操作即独占。
- **可选隔离模式**（设置项，决策点 D4）：壳自管 DSH_HOME（如 %LOCALAPPDATA%\MyDsh\home，模板自动初始化，零配置首启），并提供「导入/指向 ~/.dsh」迁移入口。两条工程路径均成立（research-core §6），默认共享，隔离为开关。

### 7.3 壳层自身配置（<AppData>/MyDsh/settings.json）

```jsonc
{
  "updateChannel": "latest",        // latest | next
  "autoCheckUpdate": true,          // 24h 周期
  "portMode": "auto",               // auto(--port 0, 默认) | fixed:3379
  "dshHome": "",                    // 空=默认 ~/.dsh；可指向自定义/隔离目录
  "telemetryDisabled": true,        // 透传 DSH_TELEMETRY_DISABLED=1（默认）
  "keepOldKernel": true,            // 更新后保留 kernel.old 用于回滚
  "nodePath": "",                   // 空=使用捆绑 runtime/node.exe
  "kernelPath": ""                  // 空=使用安装目录 kernel/
}
```

> 以上字段为建议契约，最终以实现（kernel.rs / updater.rs / settings.rs）为准。

---

## 8. 五条要求逐条对照表

| # | 要求 | 实现方案 | 支撑/证据 | 状态 |
|---|---|---|---|---|
| 1 | **原生窗口应用（无控制台/托盘）** | 主程序 #![windows_subsystem="windows"]；子进程 CREATE_NO_WINDOW（0x08000000）；托盘用核心 API tauri::tray::TrayIconBuilder（左键恢复窗口、右键菜单：显示/检查更新/退出）；关闭按钮 → hide 到托盘，真正退出时先停内核 | research-stack §3/§7（官方核心 API 无自研）；research-core §7（Node 侧 hidden 无副作用） | ✅ 定稿 |
| 2 | **安装包** | Tauri bundler 内置 NSIS：安装/卸载/快捷方式/升级/深链协议注册；WebView2 目标机策略走 embedBootstrapper（离线可装，+~1.8MB）或 downloadBootstrapper（决策点 D3）；NSIS 本机未装，bundler 构建时联网自动获取 | research-stack §2/§7（官方文档 B/C 实测 200）；§6 工具链实测 | ✅（D3 待拍板） |
| 3 | **跟随官方 npm 内核更新** | 壳自管一份内核（<install>/kernel/），Registry 直查 dist-tags → 下载 tgz（integrity 校验）→ 解压 kernel.new → --help 冒烟 → 原子替换 + 重启；前端 dist 随内核包更新，壳零前端维护；用户数据在 DSH_HOME 与内核解耦，零迁移 | research-core §5/§8.1（机制底座：profile 自愈 + bundles 规范化，实测） | ✅ 定稿 |
| 4 | **非网页套皮** | 按 research-stack §5.0 判据：系统集成层（进程属主/托盘/单实例/深链/安装/更新/窗口生命周期）**全部原生 Rust**；WebView2 仅作 UI 渲染视口，加载官方 npm 包内官方前端（VS Code/Spotify 同类分层，但更轻）；**不合格红线**：node 不由 Rust 管理 / 页面绕过内核 / 关闭窗口后 node 孤儿化 | research-stack §5.0（判据）、§7（全部官方 API）；research-core §3（窗口只加载 1 个同源 URL） | ✅ 定稿（按判据验收） |
| 5 | **性能好** | Tauri 复用系统 WebView2：不携带 Chromium，壳层安装包 2–5MB 级 / 常驻内存 20–60MB 量级（对比 Electron 85–110MB 包 + 100–200MB 内存）；内核 node 进程（40–80MB）为功能必需且与壳解耦；release + strip + lto | research-stack §4（量级证据，M1 后实测回填） | ✅ 定稿（数值待回填） |
| 附 | **目标产出：Windows 安装包 + 源码仓库** | 仓库见 §2.1；GitHub Actions 构建 + NSIS 产物（M5 可选 CI 发布流水线）；无签名首发（SmartScreen 提示，决策点 D7） | research-stack §9（M5/R9） | ✅ 计划 |

**验收指标（来自 research-stack §9，施工期实测回填）**：托盘常驻；双击第二实例 → 聚焦首实例；退出后任务管理器无残留 node（Job Object 兜底）；内核崩溃自动重启 ≤2 次；静默启动无控制台。安装包体积见 §9 D6（壳层 ≤15MB 与整体含内核 ≤250MB 分开定义）。

---

## 9. 决策点（需 captain/用户拍板）

| # | 决策点 | 建议 | 依据 |
|---|---|---|---|
| D1 | 内核版本跟随通道 | 默认 latest，设置可切 next | 容忍 -rc.x（当前 latest=rc.2） |
| D2 | 端口策略 | **默认 --port 0 动态**；固定端口（3379）作为可选项 | research-core §2.2/§9-9（3080 撞 Hyper-V 保留段） |
| D3 | WebView2 安装策略 | 推荐 embedBootstrapper（离线可装，+1.8MB） | research-stack §8 R3 |
| D4 | DSH_HOME | **默认共享 ~/.dsh**；隔离模式做设置项（不首发） | research-core §6 两方案均成立 |
| D5 | 更新后旧版本保留 | 保留最近 1 份（回滚友好），可配置删除 | §6.2 |
| D6 | 安装包体积口径 | **壳层 ≤15MB** 与 **整体（壳+node+裁剪内核）<220–250MB** 分列；M1 实测回填 | research-stack §9（原指标未区分，需修正） |
| D7 | 代码签名 | 首发无签名（用户体验折中）；正式发布前购 EV 证书 | research-stack §8 R9 |
| D8 | 深链/开机自启 | 二期（M5）再上；一期仅托盘+单实例 | research-stack §8 R6/M5 |

---

## 10. 施工契约（实现必须遵守，来自两份调研的实测约束）

1. spawn 固定形式：<runtime>/node.exe <kernel>/lib/bin.js web --patch <app>/desktop.patch.yml --no-open --port <0|固定>；**--patch 等 launcher flags 必须先于应用 flags**（否则报 unknown option --patch，实测踩坑）。
2. patch 插件模块名必须是 file:/// URL（Windows 裸路径报 ERR_UNSUPPORTED_ESM_URL_SCHEME，实测）；安装路径用安装器占位符替换。
3. 就绪契约：stdout 正则 ^dsh web: http://127.0.0.1:(\d+)；失败行 dsh: … failed to load / fatal load failure 一并落日志。
4. 停止先 /quit（200/bye）→ ≤8s → 树级硬杀；**不用** shell 信号（git-bash 下 MSYS 无法向原生进程发 POSIX 信号，实测 No such process；Windows kill = 硬杀无优雅）。
5. 孤儿防护 = Job Object（KILL_ON_JOB_CLOSE），且覆盖内核的孙进程树。
6. DSH_TELEMETRY_DISABLED=1 默认注入。
7. 更新/回滚绝不触碰 DSH_HOME；替换内核用 rename 序列，禁用「删除旧目录再复制」方式。
8. --host 0.0.0.0 禁止（内核拒绝）。
9. 捆绑 Node ≥22（推荐 24 LTS 系）；启动前 node -v 校验。
10. 内核自愈机制（healProfilesModuleFallback / normalizeShippedProfile）不可人为绕过。
11. 备用：dsh --profile headless "<任务>" 可作壳内离线自检/诊断通道（stdout 摘要 + exit code）。

---

## 11. 里程碑（engineer-main 施工，单人估时）

| 里程碑 | 内容 | 估时 |
|---|---|---|
| M1 | create-tauri-app 骨架 + rustup default stable-x86_64-pc-windows-msvc + 空壳跑通 + NSIS 出包 | 0.5–1 人日 |
| M2 | kernel.rs：spawn/就绪/quit/崩溃重启/Job Object/端口策略 | 1–1.5 人日 |
| M3 | 托盘/单实例/hide-to-tray/退出清理 + settings.rs | 0.5–1 人日 |
| M4 | NSIS 定制（图标/快捷方式/卸载/协议）+ updater.rs 版本跟随/回滚 | 1–1.5 人日 |
| M5 | （可选）CI 发布流水线 + 签名 + 深链/自启 | 1–2 人日 |
| **合计** | 核心链路 M1–M4 | **约 3–5 人日** |

---

## 12. 开放问题（评审关注）

1. **版本检查的网络依赖**：Registry 直连失败时应静默（不阻断使用）+ 手动重试；可选镜像配置（后续）。
2. **安装包体积**：内核 260MB 裁剪到 180–210MB 的收益与风险（预编译 binary 依赖校验）→ M1 用真实产物实测并回填 §8 指标。
3. **多版本并存的边界**：同一 ~/.dsh 下旧内核（rc.2）与新内核连跑——profile 自愈按新安装闭包重建链接，旧版若依赖已被移除的包需实测；INSTALLATION_OWNED_PROFILE_TUPLES 保证 bundles 规范化，属低风险，列入 M2 验收用例。
4. **更新时活动会话**：内核重启 = 服务重启；sessions 持久化在 DSH_HOME（重启后会话列表恢复），但进行中的对话会中断 → UX 上更新前提示「正在更新，将重启内核」。