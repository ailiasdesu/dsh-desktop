/**
 * @dsh-external/dsh-plugin-manager —— bundles 文本级精确编辑核心（零依赖，纯函数）。
 *
 * 红线承诺：profile manifest（package.json 的 dsh.profile.bundles 数组）只做
 * 行级/括号内文本编辑，绝不全量 JSON 重序列化——文件里除 bundles 外的任何内容
 * （字段顺序、缩进、其他段）字节级保持不变。cordis.patch.yml 只读（见 parsePatchInserts）。
 */
import { copyFileSync, existsSync, readdirSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// ---------------------------------------------------------------------------
// 保护清单：移除/停用这些 bundle 会导致 profile 无法引导（内核宿主依赖）
// ---------------------------------------------------------------------------
export const PROTECTED_BUNDLES = ['@deepseek-ai/dsh-base', '@deepseek-ai/dsh-web-app'];

function nlOf(text) {
  return text.includes('\r\n') ? '\r\n' : '\n';
}

/**
 * 定位 bundles 数组段（支持多行规范形与单行形）。
 * 返回 null 表示结构不认识（此时绝不做任何编辑）。
 */
export function findBundlesBlock(text) {
  const lines = text.split(/\r?\n/);
  let blockStart = -1;
  let head = null;
  for (let i = 0; i < lines.length; i++) {
    const m = /"bundles"\s*:\s*\[/.exec(lines[i]);
    if (m) { blockStart = i; head = lines[i]; break; }
  }
  if (blockStart === -1 || head === null) return null;
  const open = head.indexOf('[');
  const after = head.slice(open + 1);
  if (after.trim() !== '') {
    // 单行形: "bundles": ["a","b"]
    const close = head.indexOf(']', open);
    if (close === -1) return null;
    const raw = head.slice(open + 1, close);
    const items = raw.split(',').map((s) => s.trim()).filter((s) => s !== '').map((s) => s.replace(/^"|"$/g, ''));
    return { mode: 'single', lineIndex: blockStart, items, open, close };
  }
  // 多行形
  const items = [];
  let closeIdx = -1;
  for (let i = blockStart + 1; i < lines.length; i++) {
    if (/^\s*\],?\s*$/.test(lines[i])) { closeIdx = i; break; }
    const m = /^(\s*)"([^"]+)"(,?)\s*$/.exec(lines[i]);
    if (m) {
      items.push({ name: m[2], lineIndex: i, indent: m[1], comma: m[3] === ',' });
    } else if (lines[i].trim() !== '') {
      return null; // 数组中出现非纯字符串项：结构不认识，拒绝编辑
    }
  }
  if (closeIdx === -1) return null;
  return {
    mode: 'multi',
    lineIndex: blockStart,
    items,
    closeIdx,
    closeIndent: (lines[closeIdx].match(/^\s*/) || [''])[0],
  };
}

/** 列出 bundles 数组里的名字（只读）。 */
export function listBundles(text) {
  const block = findBundlesBlock(text);
  if (!block) return [];
  return block.mode === 'single' ? [...block.items] : block.items.map((i) => i.name);
}

/** 新增一个 bundle（文本级：插入一行到 ] 前面；必要时补齐最后一项的逗号）。 */
export function addBundle(text, name) {
  if (typeof name !== 'string' || name === '') return { ok: false, changed: false, error: 'bundle name 不能为空' };
  const block = findBundlesBlock(text);
  if (!block) return { ok: false, changed: false, error: 'profile manifest 中找不到可识别的 bundles 数组段' };
  const names = block.mode === 'single' ? block.items : block.items.map((i) => i.name);
  if (names.includes(name)) return { ok: false, changed: false, error: '已在 bundles 中：' + name };
  if (block.mode === 'single') {
    const inner = [...block.items, name].map((n) => JSON.stringify(n)).join(',');
    return { ok: true, changed: true, text: text.slice(0, block.open + 1) + inner + text.slice(block.close) };
  }
  const lines = text.split(/\r?\n/);
  const eol = nlOf(text);
  const indent = block.items.length > 0 ? block.items[0].indent : block.closeIndent + '  ';
  let newComma = false;
  if (block.items.length > 0) {
    const last = block.items[block.items.length - 1];
    newComma = last.comma;
    if (!last.comma) {
      lines[last.lineIndex] = lines[last.lineIndex].replace(/(")\s*$/, '",');
    }
  }
  lines.splice(block.closeIdx, 0, indent + JSON.stringify(name) + (newComma ? ',' : ''));
  return { ok: true, changed: true, text: lines.join(eol) };
}

/** 移除一个 bundle（文本级：删行；若它是最后一项且无逗号，则修复新末项的逗号）。 */
export function removeBundle(text, name) {
  if (typeof name !== 'string' || name === '') return { ok: false, changed: false, error: 'bundle name 不能为空' };
  if (PROTECTED_BUNDLES.includes(name)) {
    return { ok: false, changed: false, error: '核心 bundle（' + name + '）受保护，不可移除' };
  }
  const block = findBundlesBlock(text);
  if (!block) return { ok: false, changed: false, error: 'profile manifest 中找不到可识别的 bundles 数组段' };
  if (block.mode === 'single') {
    if (!block.items.includes(name)) return { ok: false, changed: false, error: '不在 bundles 中：' + name };
    const inner = block.items.filter((n) => n !== name).map((n) => JSON.stringify(n)).join(',');
    return { ok: true, changed: true, text: text.slice(0, block.open + 1) + inner + text.slice(block.close) };
  }
  const item = block.items.find((i) => i.name === name);
  if (!item) return { ok: false, changed: false, error: '不在 bundles 中：' + name };
  const lines = text.split(/\r?\n/);
  const eol = nlOf(text);
  const wasLast = item.lineIndex === block.items[block.items.length - 1].lineIndex;
  const removedHadComma = item.comma;
  lines.splice(item.lineIndex, 1);
  if (wasLast && !removedHadComma) {
    // 规范风格（末项无逗号）：新末项必须去掉逗号
    const remaining = block.items.filter((i) => i.name !== name);
    if (remaining.length > 0) {
      let li = remaining[remaining.length - 1].lineIndex;
      if (li > item.lineIndex) li -= 1;
      lines[li] = lines[li].replace(/,\s*$/, '');
    }
  }
  return { ok: true, changed: true, text: lines.join(eol) };
}

/** 启/停一个 bundle（enabled=true 不存在则插入；enabled=false 存在则删除行）。 */
export function toggleBundle(text, name, enabled) {
  return enabled ? addBundle(text, name) : removeBundle(text, name);
}

// ---------------------------------------------------------------------------
// 备份与原子写
// ---------------------------------------------------------------------------

/** 写前备份：<file>.bak-pm-<epoch>；返回备份路径。 */
export function backupFile(file) {
  let ts = Date.now();
  while (existsSync(file + '.bak-pm-' + ts)) ts += 1; // 同一毫秒内的连续写：epoch 自增，保证唯一且可轮替
  const bak = file + '.bak-pm-' + ts;
  copyFileSync(file, bak);
  return bak;
}

/** 清理旧备份：保留最近 keep 份（按 epoch 倒序），返回删除数量。 */
export function cleanupBackups(file, keep = 10) {
  const dir = dirname(file);
  const prefix = basename(file) + '.bak-pm-';
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
  const tmp = file + '.tmp-pm-' + Date.now();
  writeFileSync(tmp, text, 'utf8');
  try {
    renameSync(tmp, file);
  } catch (e) {
    try { rmSync(tmp, { force: true }); } catch { /* ignore */ }
    throw e;
  }
  return tmp;
}
// ---------------------------------------------------------------------------
// 解析与元数据（只读）
// ---------------------------------------------------------------------------

/** 从 profile 目录解析 bundle 包的真实目录：装入闭包（安装）→ profile node_modules → 平面 fallback。 */
export function resolveModuleDir(profileDir, name) {
  const req = createRequire(pathToFileURL(join(profileDir, 'package.json')));
  try {
    const p = req.resolve(name + '/package.json');
    return dirname(fileURLToPath(p));
  } catch { /* 部分包 exports 未暴露 package.json */ }
  try {
    let cur = fileURLToPath(req.resolve(name));
    for (let i = 0; i < 8; i++) {
      if (existsSync(join(cur, 'package.json'))) return cur;
      const parent = dirname(cur);
      if (parent === cur) break;
      cur = parent;
    }
  } catch { /* 安装闭包不可达 */ }
  for (const base of [join(profileDir, 'node_modules'), join(profileDir, '..', 'node_modules')]) {
    const d = join(base, name);
    if (existsSync(join(d, 'package.json'))) return d;
  }
  return null;
}

/** 包元数据：package.json 的 name/description/version；缺 description 时取 README 首行。 */
export function readPackageMeta(dir) {
  if (typeof dir !== 'string' || dir === '') return { name: null, description: null, version: null };
  try {
    const pj = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    return {
      name: typeof pj.name === 'string' ? pj.name : null,
      description: typeof pj.description === 'string' ? pj.description : null,
      version: typeof pj.version === 'string' ? pj.version : null,
    };
  } catch { /* 无 package.json 或坏 JSON */ }
  try {
    const readme = readFileSync(join(dir, 'README.md'), 'utf8');
    const first = readme.split(/\r?\n/).map((l) => l.trim()).find((l) => l !== '');
    return { name: null, description: first && first.length > 200 ? first.slice(0, 200) : first, version: null };
  } catch {
    return { name: null, description: null, version: null };
  }
}

/**
 * 只读解析 cordis.patch.yml：仅提取 insert 块内的插件条目 {id, name}。
 * 不会、也绝不用于写回该文件。
 */
export function parsePatchInserts(text) {
  const lines = text.split(/\r?\n/);
  const out = [];
  let inInsert = false;
  let insertIndent = 0;
  for (let i = 0; i < lines.length; i++) {
    if (!inInsert) {
      const il = /^(\s*)-\s+insert:\s*(#.*)?$/.exec(lines[i]);
      if (il) { inInsert = true; insertIndent = il[1].length; }
      continue;
    }
    if (lines[i].trim() === '') continue;
    const idM = /^(\s*)-\s+id:\s*['"]?([^'\"\s]+)/.exec(lines[i]);
    if (idM) {
      if (idM[1].length <= insertIndent) { inInsert = false; continue; }
      const id = idM[2];
      let mod = null;
      for (let j = i + 1; j < Math.min(i + 4, lines.length); j++) {
        const n = /^\s*name:\s*['"]?([^'\"\s]+)/.exec(lines[j]);
        if (n) { mod = n[1]; break; }
      }
      out.push({ id, name: mod ?? id });
      continue;
    }
    const other = /^(\s*)-\s+/.exec(lines[i]);
    if (other && other[1].length <= insertIndent) inInsert = false;
  }
  return out;
}

/** 判定目录是否为 DSH 形态插件（lib/index.js、lib/client.js、cordis.patch.yml 或 package.json 含 dsh 元数据）。 */
export function isPluginLike(dir) {
  try {
    if (existsSync(join(dir, 'lib', 'index.js'))) return true;
    if (existsSync(join(dir, 'lib', 'client.js'))) return true;
    if (existsSync(join(dir, 'cordis.patch.yml'))) return true;
    const pj = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    return typeof pj === 'object' && pj !== null && pj.dsh !== undefined;
  } catch { return false; }
}

/**
 * 枚举 profile 下已安装但未在 bundles 中的插件目录（bundle 本身不在此列；只收 DSH 形态插件）。
 * 排除内核安装闭包内的包（realpath 位于 dsh 安装根之下）——避免把内核自带 dsh-* 全家当"未启用插件"列出。
 */
export function listInstalledDirs(profileDir, exclude = []) {
  let kernelRoot = null;
  try {
    const basePkg = resolveModuleDir(profileDir, '@deepseek-ai/dsh-base');
    if (basePkg) kernelRoot = dirname(dirname(realpathSync(basePkg))); // realpath 后取安装根 = <dsh install>/node_modules
  } catch { /* 内核不可解析则不过滤 */ }
  const roots = [join(profileDir, 'node_modules'), join(profileDir, '..', 'node_modules')];
  const found = new Map();
  const skip = new Set(['.bin', '.pnpm', '.cache', 'cache', '.yarn']);
  const isDirLike = (d) => d.isDirectory() || d.isSymbolicLink(); // junction/symlink（pnpm link 与 mklink /J 均此形态）
  for (const root of roots) {
    let entries = [];
    try { entries = readdirSync(root, { withFileTypes: true }); } catch { continue; }
    for (const e of entries) {
      if (!isDirLike(e) || skip.has(e.name)) continue;
      if (e.name.startsWith('@')) {
        let sub = [];
        try { sub = readdirSync(join(root, e.name), { withFileTypes: true }); } catch { continue; }
        for (const s of sub) {
          if (!isDirLike(s) || s.name.startsWith('.')) continue;
          const name = e.name + '/' + s.name;
          if (!found.has(name) && !exclude.includes(name)) found.set(name, join(root, e.name, s.name));
        }
      } else {
        if (!found.has(e.name) && !exclude.includes(e.name)) found.set(e.name, join(root, e.name));
      }
    }
  }
  // 过滤：只保留 DSH 形态插件（lib/index.js、lib/client.js、cordis.patch.yml 或 package.json 含 dsh 元数据），
  // 并排除内核安装闭包内的包（realpath 位于 dsh 安装根之下）
  for (const [name, dir] of found) {
    if (!isPluginLike(dir)) { found.delete(name); continue; }
    if (kernelRoot !== null) {
      try {
        const real = realpathSync(dir);
        if (real.startsWith(kernelRoot)) { found.delete(name); continue; }
      } catch { /* realpath 失败保留 */ }
    }
  }
  return found;
}
