# dsh-plugin-manager

> DSH (DeepSeek Harness) 插件管理插件：在 **设置 → 插件管理** 页面直观列出 / 启用 / 停用 / 添加 / 移除
> profile bundles（插件），patch 层（cordis.patch.yml）只读展示。零运行时外部依赖；
> **绝不重启 DSH**（改动由用户手动重启后生效）；所有写操作自动备份 + 原子写，安全可逆。

## ⚠️ 与任务书的一处事实修正（重要）

任务书描述「settings.yaml 的 bundles 配置」，经本机核实：**bundles 不在 settings.yaml**（那是设置存储，47KB，
无 bundles 内容）；实际注册位置是 **`~/.dsh/profiles/web/package.json` 的 `dsh.profile.bundles` 数组**（JSON）。
因此本插件的编辑对象 = profile manifest 的 bundles 数组，红线等价执行：
**绝不全量重序列化（JSON 重写会丢字段顺序/格式），只做 bundles 数组段的行级/括号内精确文本编辑**，
文件其余内容字节级不变（有单测断言）。cordis.patch.yml 全程只读。

## 功能一览

| 功能 | 说明 | 安全 |
| --- | --- | --- |
| **list** | bundles（已启用）+ 已安装未启用（profile/node_modules 扫描）+ patch insert 项（只读展示），含 name/path/kind/enabled/exists/meta | 只读 |
| **toggle** | 启/停一个 bundle（行级插入/删除 bundles 数组） | 备份 + 原子写，needRestart:true |
| **add** | 按绝对路径新增：校验目录存在 + lib/index.js + package.json name，防重复 | 备份 + 原子写 |
| **remove** | 从 bundles 数组移除（不删插件目录）；核心 bundle（dsh-base / dsh-web-app）**受保护不可移除** | 备份 + 原子写 |
| **patch 只读** | cordis.patch.yml 的 insert 条目只展示，绝不写该文件（铁律） | 只读 |

## 端点（同源本机 Web UI；origin 围栏防跨站）

- POST `/plugin-manager/api/list` → { ok, profile:{dir,name}, bundles:[], installed:[], patches:[] }
- POST `/plugin-manager/api/toggle` { name, enabled } → { ok, needRestart, message, backup }
- POST `/plugin-manager/api/add` { path } → { ok, needRestart, message, backup }
- POST `/plugin-manager/api/remove` { name } → { ok, needRestart, message, backup }

写操作流程：先备份 `<manifest>.bak-pm-<epoch>`（保留最近 10 份自动清理）→ 临时文件 + rename 原子落盘
→ 返回 needRestart:true（**重启 DSH 后生效**）。

## 安装（三步）

1. **链接到 profile**（任选其一，推荐 junction——与 dsh-session-manager 同模式）：

```bash
# 方案 A：junction（无需 pnpm）
cmd //c "mklink /J C:\Users\34021\.dsh\profiles\web\node_modules\@dsh-external\dsh-plugin-manager C:\Users\34021\.dsh\dsh-plugin-manager"
# 方案 B：pnpm 管理
dsh plugin --profile web add file:C:/Users/34021/.dsh/dsh-plugin-manager
```

2. **注册 bundle**：在 `~/.dsh/profiles/web/package.json` 的 `dsh.profile.bundles` 数组追加
`"@dsh-external/dsh-plugin-manager"`（插入位置建议在 `@dsh-external/dsh-session-manager` 之后；
注意补上上一项的尾逗号）。插件自带 `dsh.bundle.patch`（cordis.patch.yml 插入
`plugin-manager` 行），加载器自动应用。

3. **重启 DSH** → 设置 → 插件管理。

## 回滚

- **恢复 bundle 列表**：每个写操作自动生成 `package.json.bak-pm-<epoch>`（保留最近 10 份）——
  把 `profiles/web/package.json` 换成最新一份备份即可（`package.json.bak-pm-*` 全部位于
  `profiles/web/` 下，与 dsh-session-manager 的备份策略同款）。
- **移除插件**：从 bundles 数组删除 `@dsh-external/dsh-plugin-manager`（或启停开关一键停用），
  删除 `web/node_modules/@dsh-external/dsh-plugin-manager` 链接，重启。
- 本插件**从不修改** cordis.patch.yml / settings.yaml / 任何会话数据。

## 架构

| 文件 | 职责 |
| --- | --- |
| `lib/bundle-store.js` | 纯函数核心：bundles 段解析（多行/单行）、行级 add/remove/toggle、备份轮替、原子写、模块解析、patch 只读解析、已安装扫描 |
| `lib/index.js` | host 插件：/plugin-manager/api/* 四个端点（list/toggle/add/remove）+ 写流程（备份+原子写+needRestart）+ origin 围栏 |
| `lib/client.js` | 浏览器端：设置面板 section（slots 注册），表格（名称/类型/路径/解析状态/操作）+ 添加输入 + 移除确认 + 顶部提示条；深色风格随官方 CSS 变量 |
| `tests/unit.mjs` | 54 项断言：list/toggle/add/remove/备份轮替/原子写/单行形/保护清单/resolve/patch 解析 |
| `cordis.patch.yml` | bundle patch：插入 `plugin-manager` 行 |

## 开发

```bash
cd ~/.dsh/dsh-plugin-manager
npm run build   # node --check 三个源文件
npm run check   # node tests/unit.mjs
```
