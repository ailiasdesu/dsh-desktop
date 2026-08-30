/**
 * @dsh-external/dsh-subagent-model —— cordis.patch.yml 托管块引擎 + settings.yaml 模型目录解析（零依赖，纯函数）。
 *
 * 红线承诺：
 *  - 只维护带哨兵的托管块（MANAGED_BEGIN … MANAGED_END）；块外内容字节级不变
 *    （替换/追加均基于字符串切片，前后缀原样保留，绝不全量 YAML 反序列化再重写）。
 *  - settings.yaml 只读（parseCatalog 仅行级解析 llm-pi-ai.providers 子树）。
 *  - 托管块之外已存在针对目标 id 的条目 → 拒绝写入（避免 duplicate loader entry id，不擅自合并）。
 */
import { copyFileSync, existsSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';

export const MANAGED_BEGIN = '# >>> dsh-subagent-model (managed) >>>';
export const MANAGED_END = '# <<< dsh-subagent-model <<<';

/** 两个受管目标：id 与 dsh-base cordis.patch.yml 的注册条目一一对应。 */
export const TARGETS = [
  { id: 'tool-subagent', toolName: 'subagent', mode: 'continuable', moduleName: '@deepseek-ai/dsh-tool-subagent' },
  { id: 'tool-subagent-fork', toolName: 'subagent_fork', mode: 'one-shot', moduleName: '@deepseek-ai/dsh-tool-subagent' },
];
export const TARGET_IDS = TARGETS.map((t) => t.id);

function nlOf(text) { return text.includes('\r\n') ? '\r\n' : '\n'; }

function yamlQuote(v) { return "'" + String(v).replace(/'/g, "''") + "'"; }

function yamlUnquote(v) {
  const s = String(v).trim();
  if (s.length >= 2 && s.startsWith("'") && s.endsWith("'")) return s.slice(1, -1).replace(/''/g, "'");
  if (s.length >= 2 && s.startsWith('"') && s.endsWith('"')) return s.slice(1, -1);
  return s;
}

// ---------------------------------------------------------------------------
// 托管块定位 / 渲染 / 解析
// ---------------------------------------------------------------------------

/**
 * 定位托管块。返回：
 *  - null：无托管块；
 *  - { broken: true, reason }：哨兵不完整/重复（此时拒绝一切写入）；
 *  - { start, end, body }：start = BEGIN 行首字节偏移，end = END 行（含行尾 EOL）之后
 *    的字节偏移，body = [start, end) 原文。
 */
export function findManagedBlock(text) {
  const bi = text.indexOf(MANAGED_BEGIN);
  const ei = text.indexOf(MANAGED_END);
  if (bi === -1 && ei === -1) return null;
  if (bi === -1 || ei === -1 || ei < bi) {
    return { broken: true, reason: '托管块哨兵不完整（BEGIN/END 不成对），拒绝写入以免破坏文件' };
  }
  if (text.indexOf(MANAGED_BEGIN, bi + 1) !== -1 || text.indexOf(MANAGED_END, ei + 1) !== -1) {
    return { broken: true, reason: '检测到多个托管块哨兵，拒绝写入；请手工清理 cordis.patch.yml 后再试' };
  }
  if (bi > 0 && text[bi - 1] !== '\n') {
    return { broken: true, reason: '托管块 BEGIN 哨兵不在行首，拒绝写入' };
  }
  let end = ei + MANAGED_END.length;
  if (text.startsWith('\r\n', end)) end += 2;
  else if (text.startsWith('\n', end)) end += 1;
  return { start: bi, end, body: text.slice(bi, end) };
}

/** 生成托管块文本（含哨兵与结尾 EOL；按 TARGETS 顺序、固定键序，保证幂等）。 */
export function renderManagedBlock(entries, eol) {
  const lines = [MANAGED_BEGIN];
  for (const t of TARGETS) {
    const e = entries[t.id];
    if (e === undefined) continue;
    lines.push('- id: ' + t.id);
    lines.push('  name: ' + yamlQuote(t.moduleName));
    lines.push('  config:');
    lines.push('    agentOptions:');
    lines.push('      provider: ' + yamlQuote(e.provider));
    lines.push('      model: ' + yamlQuote(e.model));
    if (e.reasoningEffort !== undefined) lines.push('      reasoningEffort: ' + yamlQuote(e.reasoningEffort));
    if (e.maxTokens !== undefined) lines.push('      maxTokens: ' + e.maxTokens);
  }
  lines.push(MANAGED_END);
  return lines.join(eol) + eol;
}

/** 解析托管块内本插件生成的条目（只认本插件的固定生成格式；round-trip 与 renderManagedBlock 对偶）。 */
export function parseManagedEntries(body) {
  const raw = {};
  let cur = null;
  for (const rawLine of body.split(/\r?\n/)) {
    const line = rawLine.replace(/\r$/, '');
    const idM = /^- id:\s*(\S+)\s*$/.exec(line);
    if (idM) { cur = {}; raw[idM[1]] = cur; continue; }
    if (cur === null) continue;
    const kvM = /^\s+(provider|model|reasoningEffort|maxTokens):\s*(.+?)\s*$/.exec(line);
    if (kvM) {
      if (kvM[1] === 'maxTokens') {
        const n = Number(kvM[2]);
        if (Number.isInteger(n) && n >= 1) cur.maxTokens = n;
      } else {
        cur[kvM[1]] = yamlUnquote(kvM[2]);
      }
    }
  }
  const out = {};
  for (const id of TARGET_IDS) {
    const e = raw[id];
    if (e !== undefined && typeof e.provider === 'string' && e.provider !== '' && typeof e.model === 'string' && e.model !== '') {
      const v = { provider: e.provider, model: e.model };
      if (typeof e.reasoningEffort === 'string' && e.reasoningEffort !== '') v.reasoningEffort = e.reasoningEffort;
      if (typeof e.maxTokens === 'number') v.maxTokens = e.maxTokens;
      out[id] = v;
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// 冲突扫描：托管块之外针对目标 id 的用户条目
// ---------------------------------------------------------------------------

/** 扫描托管块之外的 tool-subagent / tool-subagent-fork 条目（含 insert 子项与引号形态），返回 [{line, id}]。 */
export function findExternalTargetEntries(text) {
  const block = findManagedBlock(text);
  const hasRange = block !== null && block.broken !== true;
  const out = [];
  let pos = 0;
  let lineNo = 0;
  while (pos <= text.length) {
    const nl = text.indexOf('\n', pos);
    const end = nl === -1 ? text.length : nl;
    lineNo += 1;
    let line = text.slice(pos, end);
    if (line.endsWith('\r')) line = line.slice(0, -1);
    const m = /^\s*(?:-\s+)?id:\s*['"]?(tool-subagent(?:-fork)?)['"]?\s*(?:#.*)?$/.exec(line);
    if (m !== null) {
      const inBlock = hasRange && pos >= block.start && pos < block.end;
      if (!inBlock) out.push({ line: lineNo, id: m[1] });
    }
    if (nl === -1) break;
    pos = nl + 1;
  }
  return out;
}

// ---------------------------------------------------------------------------
// 设置校验 / 写入变换（纯函数：text in, text out）
// ---------------------------------------------------------------------------

/** 校验并规整一次 set 的载荷：provider/model 必填，reasoningEffort/maxTokens 可选。 */
export function normalizeSettings(input) {
  if (typeof input !== 'object' || input === null) return { ok: false, error: '设置必须是对象' };
  const provider = typeof input.provider === 'string' ? input.provider.trim() : '';
  const model = typeof input.model === 'string' ? input.model.trim() : '';
  if (provider === '') return { ok: false, error: '缺少 provider' };
  if (model === '') return { ok: false, error: '缺少 model' };
  if (/[\r\n#]/.test(provider + model)) return { ok: false, error: 'provider/model 含非法字符' };
  const value = { provider, model };
  if (input.reasoningEffort !== undefined && input.reasoningEffort !== null && String(input.reasoningEffort).trim() !== '') {
    const eff = String(input.reasoningEffort).trim();
    if (/[\r\n#]/.test(eff)) return { ok: false, error: 'reasoningEffort 含非法字符' };
    value.reasoningEffort = eff;
  }
  if (input.maxTokens !== undefined && input.maxTokens !== null && input.maxTokens !== '') {
    const n = Number(input.maxTokens);
    if (!Number.isInteger(n) || n < 1) return { ok: false, error: 'maxTokens 必须是 ≥1 的整数' };
    value.maxTokens = n;
  }
  return { ok: true, value };
}

/** 写入/更新一个目标的托管条目。返回 { ok, changed, text?, error?, block? }；块外字节不变。 */
export function upsertManaged(text, toolId, settings) {
  if (!TARGET_IDS.includes(toolId)) return { ok: false, changed: false, error: '未知目标 id：' + (toolId === '' ? '(空)' : toolId) };
  const norm = normalizeSettings(settings);
  if (!norm.ok) return { ok: false, changed: false, error: norm.error };
  const block = findManagedBlock(text);
  if (block !== null && block.broken === true) return { ok: false, changed: false, error: block.reason };
  const conflicts = findExternalTargetEntries(text);
  if (conflicts.length > 0) {
    const detail = conflicts.map((c) => c.id + '（第 ' + c.line + ' 行）').join('、');
    return { ok: false, changed: false, error: '拒绝写入：cordis.patch.yml 托管块之外已存在针对 ' + detail + ' 的条目；为避免 duplicate loader entry id，请先手工删除或迁移该条目，再用本插件管理。' };
  }
  const eol = text === '' ? '\n' : nlOf(text);
  const entries = block !== null ? parseManagedEntries(block.body) : {};
  entries[toolId] = norm.value;
  const blockText = renderManagedBlock(entries, eol);
  let next;
  if (block !== null) {
    next = text.slice(0, block.start) + blockText + text.slice(block.end);
  } else if (text === '') {
    next = blockText;
  } else {
    const base = text.endsWith('\n') ? text : text + eol;
    next = base + eol + blockText;
  }
  if (next === text) return { ok: true, changed: false, text, block: blockText };
  return { ok: true, changed: true, text: next, block: blockText };
}

/** 移除一个目标的托管条目；最后一个条目移除时整块（含追加时引入的前置空行）一并回收。 */
export function removeManaged(text, toolId) {
  if (!TARGET_IDS.includes(toolId)) return { ok: false, changed: false, error: '未知目标 id：' + (toolId === '' ? '(空)' : toolId) };
  const block = findManagedBlock(text);
  if (block === null) return { ok: false, changed: false, error: '该目标当前未被本插件托管（无托管块）：' + toolId };
  if (block.broken === true) return { ok: false, changed: false, error: block.reason };
  const entries = parseManagedEntries(block.body);
  if (entries[toolId] === undefined) return { ok: false, changed: false, error: '该目标当前未被本插件托管：' + toolId };
  delete entries[toolId];
  const eol = nlOf(text);
  let next;
  if (Object.keys(entries).length === 0) {
    let start = block.start;
    if (text.slice(0, start).endsWith(eol + eol)) start -= eol.length;
    next = text.slice(0, start) + text.slice(block.end);
  } else {
    next = text.slice(0, block.start) + renderManagedBlock(entries, eol) + text.slice(block.end);
  }
  return { ok: true, changed: true, text: next };
}

/**
 * 文本是否不含任何 YAML 实际内容（只有空行/注释行）。内核对存在的 profile 补丁
 * 文件要求必须是顶层数组（null 即启动失败），而缺省文件 = 无用户层、正常启动；
 * 因此 reset 清空后若命中此判定，调用方应删除文件而非写回。
 */
export function isYamlEmpty(text) {
  if (typeof text !== 'string') return true;
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line !== '' && !line.startsWith('#')) return false;
  }
  return true;
}

// ---------------------------------------------------------------------------
// settings.yaml 模型目录（只读，行级缩进扫描）
// ---------------------------------------------------------------------------

function assignModelKey(model, key, rawVal) {
  const val = yamlUnquote(rawVal);
  if (key === 'id') model.id = val;
  else if (key === 'name') model.name = val;
  else if (key === 'contextWindow') { const n = Number(val); if (Number.isFinite(n)) model.contextWindow = n; }
  else if (key === 'maxTokens') { const n = Number(val); if (Number.isFinite(n)) model.maxTokens = n; }
}

/**
 * 解析 llm-pi-ai.providers 子树 → { providers: [{ name, models: [{ id, name, contextWindow,
 * maxTokens, efforts }] }] }。efforts = reasoningEfforts 映射的键列表（保持原序）；
 * 兼容 "- name:" 先于 "id:" 的条目顺序与 "off:" 空值。
 */
export function parseCatalog(text) {
  const providers = [];
  if (typeof text !== 'string' || text === '') return { providers };
  let inRoot = false;
  let inProviders = false;
  let provider = null;
  let inModels = false;
  let model = null;
  let inEfforts = false;
  const flushModel = () => {
    if (model !== null && provider !== null && typeof model.id === 'string' && model.id !== '') provider.models.push(model);
    model = null;
  };
  const flushProvider = () => {
    if (provider !== null) { flushModel(); providers.push(provider); }
    provider = null; inModels = false; inEfforts = false;
  };
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.replace(/\r$/, '');
    if (line.trim() === '') continue;
    const ind = (line.match(/^ */) || [''])[0].length;
    if (!inRoot) { if (/^llm-pi-ai:\s*(#.*)?$/.test(line)) inRoot = true; continue; }
    if (ind === 0) { flushProvider(); break; }
    if (!inProviders) { if (ind === 2 && /^ {2}providers:\s*(#.*)?$/.test(line)) inProviders = true; continue; }
    if (ind === 2) { flushProvider(); inProviders = false; continue; }
    if (ind === 4) {
      flushProvider();
      const m = /^ {4}([^\s:]+):\s*(#.*)?$/.exec(line);
      if (m !== null) provider = { name: m[1], models: [] };
      continue;
    }
    if (provider === null) continue;
    if (ind === 6) {
      const isModels = /^ {6}models:\s*(#.*)?$/.test(line);
      if (!isModels) { flushModel(); inEfforts = false; }
      inModels = isModels;
      continue;
    }
    if (!inModels) continue;
    if (ind === 8) {
      const m = /^ {8}- ([^\s:]+):\s*(.*?)\s*$/.exec(line);
      if (m !== null) {
        flushModel();
        inEfforts = false;
        model = { id: '', name: '', contextWindow: null, maxTokens: null, efforts: [] };
        assignModelKey(model, m[1], m[2]);
      }
      continue;
    }
    if (model === null) continue;
    if (ind === 10) {
      const m = /^ {10}([^\s:]+):\s*(.*?)\s*$/.exec(line);
      if (m !== null) {
        inEfforts = m[1] === 'reasoningEfforts';
        if (!inEfforts) assignModelKey(model, m[1], m[2]);
      }
      continue;
    }
    if (ind >= 12 && inEfforts) {
      const m = /^ +([^\s:]+):\s*(.*?)\s*$/.exec(line);
      if (m !== null) model.efforts.push(m[1]);
      continue;
    }
  }
  flushProvider();
  return { providers };
}

// ---------------------------------------------------------------------------
// 备份与原子写（与 dsh-plugin-manager 同款，后缀域 .bak-sm- / .tmp-sm-）
// ---------------------------------------------------------------------------

/** 写前备份：<file>.bak-sm-<epoch>；同一毫秒内 epoch 自增保证唯一。返回备份路径。 */
export function backupFile(file) {
  let ts = Date.now();
  while (existsSync(file + '.bak-sm-' + ts)) ts += 1;
  const bak = file + '.bak-sm-' + ts;
  copyFileSync(file, bak);
  return bak;
}

/** 清理旧备份：保留最近 keep 份（按 epoch 倒序），返回删除数量。 */
export function cleanupBackups(file, keep = 10) {
  const dir = dirname(file);
  const prefix = basename(file) + '.bak-sm-';
  let entries = [];
  try { entries = readdirSync(dir); } catch { return 0; }
  const list = entries
    .filter((n) => n.startsWith(prefix) && /^\d+$/.test(n.slice(prefix.length)))
    .map((n) => ({ n, ts: Number(n.slice(prefix.length)) }))
    .sort((a, b) => b.ts - a.ts);
  let removed = 0;
  for (const e of list.slice(keep)) {
    try { rmSync(join(dir, e.n), { force: true }); removed += 1; } catch { /* 占用中跳过 */ }
  }
  return removed;
}

/** 原子写：同目录临时文件 + rename（Windows 下 rename 覆盖目标）。 */
export function atomicWrite(file, text) {
  const tmp = file + '.tmp-sm-' + Date.now();
  writeFileSync(tmp, text, 'utf8');
  try {
    renameSync(tmp, file);
  } catch (e) {
    try { rmSync(tmp, { force: true }); } catch { /* ignore */ }
    throw e;
  }
  return tmp;
}
