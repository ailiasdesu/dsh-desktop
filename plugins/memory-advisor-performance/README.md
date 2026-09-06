# Advisor 增量事件统计

## 部署范围

只部署 deploy-manifest.json 中的三个文件到 dsh-memory-evolve 同名相对路径：
- lib/advisor/index.js：接线改为 handleSessionEvent(session, event)。
- lib/advisor/observer.js：保留原 handleEvent(sessionId, events, event) 数组接口；删除未被消费的 fingerprint；接入现代 Session 入口。
- lib/advisor/turn-tracker.js：基于公开 seq/eventAt 的增量 turn 分类。

其他 lib/ 与 original/ 文件仅用于隔离执行上游测试和原版对照，不应覆盖安装。没有修改真实安装件、用户数据、UI/CSS、官方包或调用模型。安装前核对 expectedOriginalSha256，遇到不匹配重新审查；不要盲目覆盖插件新版。

## 正确性

使用 Session 对象 WeakMap 隔离同 ID 的新会话。完整事件流只折叠新事件；回合进入 step 后，在任何 turn/end 消费对应标记，保留原 completed/max-tokens/error 评审门控。热加载、恢复 seed marker、漏发事件、订阅中途缺失的前缀，在首个需要分类的结束事件通过 eventAt 补齐；错误/不支持能力退回原快照扫描。原 agentic 模式闩锁只由观察到的 turn/end 驱动，没有偷偷将历史中的结束事件用于改变旧门控。原 rewrite、seedTo、渲染和回调顺序保留。

扫描整个已安装插件（lib/src/tests/docs，排除生成 map）确认 fingerprint 只有 observer 私有 renderer 上的字段声明和赋值，没有读取消费者。删除它没有改变原来并不存在的重写检测：实际重写检测仍依赖事件和 cursor 回退。

测试：125 项复制的原 Advisor 测试（observer/commands/opt-in/API/runtime/store/scopes/conversation/guard/kinds/visible-surface）以及 11 项原新版回调对照均通过；新增测试使用原 observer 的实际渲染与回调，不用替代输出 mock。涵盖多 turn、无 step、aborted/blocked/interrupted、rewrite、agentic、同 ID 新 Session、缺失前缀、重复事件、legacy 回退、eventAt 损坏回退，以及真实官方 Session append 和 seeded resume。完整日志 results/all-tests.log；新增测试日志 results/test-incremental.log。

## 实测

runtime/node.exe v24.16.0；每一组合独立子进程，预置 100 / 100000 个合成历史回合，每回合 5 事件、2 消息；先测一次冷恢复，然后连续处理 100 新回合。三次重复取中位数。实际调用原/新 observer 和可见消息渲染，deriveMessages 使用缓存数组模拟官方缓存投影，以隔离本次改变的处理开销。所有版本输出 delta 数量/字符数一致。内存为该基准进程采样值，不是整机、官方完整会话恢复或模型推理内存。

| 历史回合 | 实现 | 每回合均值 ms | P95 ms | 峰值 heap MiB | 峰值 RSS MiB | 100 回合 snapshot / eventAt 次数 |
|---:|---|---:|---:|---:|---:|---|
| 100 | original | 0.0914 | 0.2750 | 5.38 | 42.60 | 100 / 0 |
| 100 | optimized | 0.0222 | 0.0626 | 5.17 | 38.69 | 0 / 0 |
| 100,000 | original | 41.4026 | 52.4125 | 328.77 | 461.56 | 100 / 0 |
| 100,000 | optimized | 0.0416 | 0.1449 | 94.42 | 165.42 | 0 / 0 |

大历史正常路径由每回合全事件复制/扫描及全消息 fingerprint 降为新事件折叠与新增消息渲染。缺失前缀的首次恢复依然 O(历史事件)，没有宣称其变成 O(1)；状态保留未结束的 stepped turn ID 与最后结束事件，而非日志副本。完整冷启动/热运行原始读数、RSS/heap 前后值见 results/benchmark.json。辅助进程/Rust IPC 不适合这一纯计数热点，本修复没有增加 IPC。

## 复现

在本目录执行：
```
node --test --test-concurrency=1 tests/*.test.js
node --expose-gc benchmark.mjs original 100000
node --expose-gc benchmark.mjs optimized 100000
```

真实 Session 测试引用本机安装内核绝对路径，换机器需更新该只读导入。没有把整个原始会话藏进 native sidecar；官方消息投影、可见表面渲染和评审模型调用仍按原实现运行。
