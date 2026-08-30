# @dsh-external/dsh-subagent-model

DSH 设置页「子代理模型」：为标准子代理委派工具 **subagent**（id: tool-subagent，continuable）与
**subagent_fork**（id: tool-subagent-fork，one-shot）单独指定「提供商 / 模型 / 思考强度」，
写入 profile 补丁层的**哨兵托管块**，重启 DSH 后生效。

## 原理（内核契约）

- 两个委派工具均由 @deepseek-ai/dsh-tool-subagent 提供，在 dsh-base 的 cordis.patch.yml 里以
  id tool-subagent / tool-subagent-fork 注册；顶层 config.provider 是执行驱动（spawn/fork），
  子代理的 LLM 路由是 config.agentOptions（{provider, model, maxTokens}）。
- 用户 profile 的 cordis.patch.yml 按 id 覆盖 config 是**合并**语义：只写 agentOptions、
  不重写顶层必填 provider，内核照常启动（隔离 DSH_HOME 实测）。
- reasoningEffort 未在 agentOptions schema 声明，但 @deepseek-ai/schemastery 的 z.object
  **保留未声明键**，dsh-tool-subagent 原样透传、dsh-subagent 以 ...requested 展开、dsh-agent
  将其按一等字段解析——实测生效。**属"未声明但透传"的字段，官方若收紧校验可能失效**（UI 已如实标注）。
- 模型目录只读自 ~/.dsh/settings.yaml 的 llm-pi-ai.providers.<名>.models[]
  （id / name / contextWindow / maxTokens / reasoningEfforts 键表）。

## 写入契约（红线）

- 唯一写入目标：<profile>/cordis.patch.yml（默认 ~/.dsh/profiles/web/cordis.patch.yml）。
- 只维护带哨兵的托管块，块外内容**字节级不变**（字符串切片替换/追加，绝不全量 YAML 反序列化重写）：

  # >>> dsh-subagent-model (managed) >>>
  - id: tool-subagent
    name: '@deepseek-ai/dsh-tool-subagent'
    config:
      agentOptions:
        provider: 'gorouter'
        model: 'claude-opus-5'
        reasoningEffort: 'high'
  # <<< dsh-subagent-model <<<

- 写前备份 <file>.bak-sm-<epoch>（保留最近 10 份）+ 临时文件 rename 原子替换。
- 托管块之外已存在针对 tool-subagent / tool-subagent-fork 的用户条目 → **拒绝写入**并返回可读原因
  （避免 duplicate loader entry id），不擅自合并；请先手工删除/迁移该条目。
- settings.yaml 只读；profile package.json 不属于本插件（那是 dsh-plugin-manager 的领域）。
- reset 边界：移除最后一个托管条目后若文件只剩注释/空白，则**删除文件**（备份保留）——
  内核对存在的 profile 补丁要求必须是顶层 YAML 数组（空/纯注释文件会启动失败，
  dsh-app-boot loadProfile:564 实测），缺省文件才等于“无用户层”。

## API（/subagent-model/api/，本机 loopback origin 围栏，恶意 origin 403）

- GET|POST /subagent-model/api/list
  -> { ok, profile, patchFile, settingsFile, blockBroken, targets:[{id, toolName, mode, managed,
       effective:{provider,model,reasoningEffort?,maxTokens?}|null}], conflicts:[{line,id}],
       catalog:{providers:[{name, models:[{id, name, contextWindow, maxTokens, efforts:[...]}]}]} }
- POST /subagent-model/api/set   { id, provider, model, reasoningEffort?, maxTokens? }
  -> { ok, needRestart:true, message, backup, block }
- POST /subagent-model/api/reset { id }
  -> { ok, needRestart:true, message, backup }   // 移除该目标的托管条目（回到继承主会话）

## UI

设置页「子代理模型」：两行（subagent / subagent_fork），每行显示当前生效值（未设置显示
"继承主会话"）+ 提供商下拉 + 模型下拉（随提供商联动）+ 思考强度下拉（选项取自该模型的
reasoningEfforts 键，附"跟随继承"项）+ 保存 / 恢复继承；顶部「重启 DSH 后生效」提示条；
表格下方小字说明 reasoningEffort 的透传性质。当前生效值不在目录中时下拉追加"（目录外）"项。

## 安装（web profile）

1. junction 链接插件目录（未注册前完全惰性零副作用）：

   cmd /c mklink /J "%USERPROFILE%\.dsh\profiles\web\node_modules\@dsh-external\dsh-subagent-model" "<本插件目录>"

2. ~/.dsh/profiles/web/package.json 的 dsh.profile.bundles 数组追加
   "@dsh-external/dsh-subagent-model"（行级文本编辑，勿全量重序列化；写前备份）。
3. 重启 DSH。

## 开发

- node --check lib/index.js lib/store.js lib/client.js（npm run build）
- node tests/unit.mjs（npm run check；74 断言：托管块插入/更新/移除幂等、块外字节不变、
  冲突条目拒写、哨兵破损拒写、备份轮替 keep10、原子写无残留、catalog 解析、按模型给出
  efforts、CRLF 保持、YAML 引号转义）
- 冒烟（隔离 DSH_HOME，勿碰真实 ~/.dsh）：临时 home 里 junction 内核 node_modules 与本插件，
  最小 bundles [@deepseek-ai/dsh-base, @deepseek-ai/dsh-web-app, 本插件] 启动
  node <kernel>/lib/bin.js web --no-open --port 0，就绪行 "dsh web: http://127.0.0.1:<port>"。
  清理时先 cmd /c rmdir 摘除 junction 再删目录树。

## License

BSD-3-Clause
