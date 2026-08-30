/**
 * @dsh-external/dsh-subagent-model —— 模型目录聚合（内核实时枚举 ∪ settings.yaml，去重合并）。
 *
 * 内核侧数据源与官方模型选择 UI 同源：dsh-host-apiproxy 对 `session.models` / `llm.models`
 * 两个 RPC 的实现 buildModelCatalog(ctx) = ctx.llm.listProviders() → ctx.llm.listModels(id)
 * → ctx.llm.resolveModelInfo(id, modelId)（reasoning.efforts[].id 即思考强度目录）。
 * 本模块用同一个 cordis 服务 `llm`（@deepseek-ai/dsh-llm 的 LlmRuntime）复刻该枚举：
 * 逐 provider 容错（单个失败进 failures，不拖垮目录），llm 服务缺席时 available=false，
 * 目录退化为 settings.yaml 单源；枚举失败绝不影响 /list 主功能。
 */

function errText(e) { return String(e && e.message ? e.message : e); }

/**
 * 经 ctx.get('llm') 复刻内核 buildModelCatalog 枚举。
 * 返回 { available, providers:[{name, models:[{id,name,contextWindow,maxTokens,efforts}]}], failures:[{provider,message}] }。
 */
export async function kernelCatalog(ctx) {
  let llm;
  try { llm = ctx !== null && typeof ctx === 'object' && typeof ctx.get === 'function' ? ctx.get('llm') : undefined; } catch { llm = undefined; }
  if (llm === undefined || llm === null) return { available: false, providers: [], failures: [] };
  let listed;
  try { listed = llm.listProviders(); } catch (e) {
    return { available: false, providers: [], failures: [{ provider: '(listProviders)', message: errText(e) }] };
  }
  const providers = [];
  const failures = [];
  for (const p of Array.isArray(listed) ? listed : []) {
    const pid = p !== null && typeof p === 'object' && typeof p.id === 'string' ? p.id : '';
    if (pid === '') continue;
    try {
      const models = await llm.listModels(pid);
      const entries = [];
      for (const m of Array.isArray(models) ? models : []) {
        const mid = m !== null && typeof m === 'object' && typeof m.id === 'string' ? m.id : '';
        if (mid === '') continue;
        let efforts = [];
        try {
          const info = await llm.resolveModelInfo(pid, mid);
          const reasoning = info !== null && typeof info === 'object' ? info.reasoning : undefined;
          if (reasoning !== null && typeof reasoning === 'object' && Array.isArray(reasoning.efforts)) {
            efforts = reasoning.efforts
              .map((e) => (typeof e === 'string' ? e : e !== null && typeof e === 'object' && typeof e.id === 'string' ? e.id : ''))
              .filter((id) => id !== '');
          }
        } catch { /* 单模型 efforts 解析失败：模型仍列出，efforts 留空 */ }
        entries.push({
          id: mid,
          name: typeof m.name === 'string' && m.name !== '' ? m.name : mid,
          contextWindow: null,
          maxTokens: null,
          efforts,
        });
      }
      if (entries.length > 0) providers.push({ name: pid, models: entries });
    } catch (e) {
      failures.push({ provider: pid, message: errText(e) });
    }
  }
  return { available: true, providers, failures };
}

/**
 * 合并 settings.yaml 目录与内核枚举：按 provider name / model id 去重；内核在前
 * （与官方选择器顺序一致），settings 独有的追加在后；efforts 取并集保序（内核先）；
 * contextWindow/maxTokens 缺失时由另一源回填；source 标注 'kernel' / 'settings' /
 * 'kernel+settings' 供 UI 悬停提示。纯函数，无 I/O。
 */
export function mergeCatalogs(settingsProviders, kernelProviders) {
  const list = [];
  const byName = new Map();
  const ensure = (name) => {
    let entry = byName.get(name);
    if (entry === undefined) {
      entry = { name, sources: [], models: [], modelById: new Map() };
      byName.set(name, entry);
      list.push(entry);
    }
    return entry;
  };
  const addAll = (provs, source) => {
    for (const p of Array.isArray(provs) ? provs : []) {
      if (p === null || typeof p !== 'object' || typeof p.name !== 'string' || p.name === '') continue;
      const entry = ensure(p.name);
      if (!entry.sources.includes(source)) entry.sources.push(source);
      for (const m of Array.isArray(p.models) ? p.models : []) {
        if (m === null || typeof m !== 'object' || typeof m.id !== 'string' || m.id === '') continue;
        let model = entry.modelById.get(m.id);
        if (model === undefined) {
          model = {
            id: m.id,
            name: typeof m.name === 'string' && m.name !== '' ? m.name : m.id,
            contextWindow: typeof m.contextWindow === 'number' ? m.contextWindow : null,
            maxTokens: typeof m.maxTokens === 'number' ? m.maxTokens : null,
            efforts: [],
          };
          entry.modelById.set(m.id, model);
          entry.models.push(model);
        } else {
          if (model.name === model.id && typeof m.name === 'string' && m.name !== '') model.name = m.name;
          if (model.contextWindow === null && typeof m.contextWindow === 'number') model.contextWindow = m.contextWindow;
          if (model.maxTokens === null && typeof m.maxTokens === 'number') model.maxTokens = m.maxTokens;
        }
        for (const eff of Array.isArray(m.efforts) ? m.efforts : []) {
          if (typeof eff === 'string' && eff !== '' && !model.efforts.includes(eff)) model.efforts.push(eff);
        }
      }
    }
  };
  addAll(kernelProviders, 'kernel');
  addAll(settingsProviders, 'settings');
  return list.map((e) => ({ name: e.name, source: e.sources.join('+'), models: e.models }));
}
