
/**
 * @dsh-external/dsh-plugin-manager —— host 插件：插件管理 REST API。
 *
 * 路由前缀 /plugin-manager/api/（同源本机 Web UI；origin 围栏防跨站滥用）：
 *   POST /plugin-manager/api/list    -> { ok, profile:{dir,name}, bundles:[], installed:[], patches:[] }
 *   POST /plugin-manager/api/toggle  { name, enabled } -> { ok, needRestart, message, backup }
 *   POST /plugin-manager/api/add     { path }          -> { ok, needRestart, message, backup }
 *   POST /plugin-manager/api/remove  { name }          -> { ok, needRestart, message, backup }
 *
 * 写操作契约：写前备份（<manifest>.bak-pm-<epoch>，保留最近 10 份）+ 临时文件原子写；
 * 所有编辑均为 profile manifest 的 bundles 数组文本级精确编辑（不重序列化整个文件）。
 * cordis.patch.yml 只读（parsePatchInserts 仅展示）。core bundles 受保护。
 */
import { existsSync, readFileSync, statSync } from 'node:fs';
import { basename, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { addBundle, atomicWrite, backupFile, cleanupBackups, isPluginLike, listBundles, listInstalledDirs, parsePatchInserts, readPackageMeta, removeBundle, resolveModuleDir, toggleBundle } from './bundle-store.js';

export const name = '@dsh-external/dsh-plugin-manager';
export const inject = ['webServer'];

const API = '/plugin-manager/api';
const MAX_BODY = 1024 * 1024;

function json(res, status, body) {
  res.statusCode = status;
  res.setHeader('content-type', 'application/json; charset=utf-8');
  res.setHeader('cache-control', 'no-store');
  res.end(JSON.stringify(body));
}

function requestBody(req) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on('data', (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY) { req.destroy(); reject(new Error('request body too large')); return; }
      chunks.push(chunk);
    });
    req.on('end', () => {
      try {
        const text = Buffer.concat(chunks).toString('utf8');
        resolve(text === '' ? {} : JSON.parse(text));
      } catch { reject(new Error('invalid JSON body')); }
    });
    req.on('error', reject);
  });
}

/** 本机 origin 围栏：无 Origin 或 loopback origin 放行（与内核 /api 语义对齐，防第三方页面 CSRF）。 */
function isLoopbackOrigin(origin) {
  if (origin === undefined || origin === null || origin === '') return true;
  try {
    const u = new URL(origin);
    return u.hostname === '127.0.0.1' || u.hostname === 'localhost' || u.hostname === '[::1]' || /^127\.[0-9]+\.[0-9]+\.[0-9]+$/.test(u.hostname);
  } catch { return false; }
}

export function apply(ctx) {
  // profile 目录 = Loader baseUrl（boot 时指向 <profile>/cordis.yml 所在目录）
  let profileDir = null;
  try { profileDir = fileURLToPath(new URL(ctx.baseUrl ?? new URL('file:///' + process.cwd() + '/' , 'file:///'))); } catch { /* fallback below */ }
  if (profileDir === null || profileDir === '' || !existsSync(join(profileDir, 'cordis.yml'))) {
    const home = process.env.DSH_HOME && process.env.DSH_HOME.trim() !== '' ? process.env.DSH_HOME : join(process.env.USERPROFILE ?? process.env.HOME ?? '', '.dsh');
    profileDir = join(home, 'profiles', 'web');
  }
  const manifestPath = join(profileDir, 'package.json');
  const patchPath = join(profileDir, 'cordis.patch.yml');

  function manifestText() { return readFileSync(manifestPath, 'utf8'); }
  function patchText() { try { return readFileSync(patchPath, 'utf8'); } catch { return ''; } }
  function hasManifest() { return existsSync(manifestPath); }

  function buildList() {
    if (!hasManifest()) return { ok: false, code: 'no-manifest', message: '未找到 profile manifest：' + manifestPath };
    const text = manifestText();
    const bundleNames = listBundles(text);
    const bundleRows = bundleNames.map((b) => {
      const dir = resolveModuleDir(profileDir, b);
      const meta = dir ? readPackageMeta(dir) : { name: b, description: null, version: null };
      return { name: b, kind: 'bundle', enabled: true, exists: dir !== null, path: dir, meta };
    });
    const patchRows = parsePatchInserts(patchText()).map((p) => {
      const dir = resolveModuleDir(profileDir, p.name);
      const meta = dir ? readPackageMeta(dir) : { name: p.name, description: null, version: null };
      return { id: p.id, name: p.name, kind: 'patch', enabled: true, exists: dir !== null, path: dir, patchFile: patchPath, meta };
    });
    const installedRows = [];
    for (const [n, dir] of listInstalledDirs(profileDir, bundleNames)) {
      if (!isPluginLike(dir)) continue;
      installedRows.push({ name: n, kind: 'installed', enabled: false, exists: true, path: dir, meta: readPackageMeta(dir) });
    }
    installedRows.sort((a, b) => a.name.localeCompare(b.name));
    return {
      ok: true,
      profile: { dir: profileDir, name: basename(profileDir) },
      manifest: manifestPath,
      patchFile: patchPath,
      bundles: bundleRows,
      installed: installedRows,
      patches: patchRows,
    };
  }

  /** 统一写流程：备份（保留最近 10）+ 原子写。mutate(text) -> {ok, changed, text?, error?} */
  function applyWrite(mutate) {
    if (!hasManifest()) return { ok: false, message: '未找到 profile manifest：' + manifestPath };
    const r = mutate(manifestText());
    if (!r.ok || !r.changed) return { ok: false, message: r.error ?? '未发生变更' };
    const bak = backupFile(manifestPath);
    const removed = cleanupBackups(manifestPath, 10);
    try { atomicWrite(manifestPath, r.text); } catch (e) {
      return { ok: false, message: '写入失败：' + String(e?.message ?? e) + '（备份保留于 ' + basename(bak) + '）' };
    }
    return {
      ok: true,
      needRestart: true,
      message: '已写入 ' + basename(manifestPath) + '，备份 ' + basename(bak) + (removed > 0 ? '（清理旧备份 ' + removed + ' 份）' : '') + '；重启 DSH 后生效',
      backup: basename(bak),
    };
  }

  function handleAdd(res, body) {
    const p = typeof body.path === 'string' ? body.path : '';
    if (p === '') return json(res, 200, { ok: false, message: '缺少 path（需为插件目录的绝对路径）' });
    if (!isAbsolute(p)) return json(res, 200, { ok: false, message: 'path 必须是绝对路径：' + p });
    if (!existsSync(p) || !statSync(p).isDirectory()) return json(res, 200, { ok: false, message: '目录不存在：' + p });
    if (!existsSync(join(p, 'lib', 'index.js'))) return json(res, 200, { ok: false, message: '目录下未找到 lib/index.js（不是 DSH 插件）：' + p });
    const meta = readPackageMeta(p);
    const name = meta.name;
    if (typeof name !== 'string' || name === '') return json(res, 200, { ok: false, message: '未能在 ' + p + ' 的 package.json 中读取 name' });
    const out = applyWrite((text) => addBundle(text, name));
    if (out.ok && resolveModuleDir(profileDir, name) === null) {
      out.message += '（提醒：' + name + ' 当前不可从 profile node_modules 解析；若重启后未加载，请先把它链接进 profile——见 README 安装一节）';
    }
    return json(res, 200, { ...out, name, path: p });
  }

  function handleRemove(res, body) {
    const bname = typeof body.name === 'string' ? body.name : '';
    if (bname === '') return json(res, 200, { ok: false, message: '缺少 name' });
    const out = applyWrite((text) => removeBundle(text, bname));
    return json(res, 200, { ...out, name: bname });
  }

  function handleToggle(res, body) {
    const bname = typeof body.name === 'string' ? body.name : '';
    if (bname === '') return json(res, 200, { ok: false, message: '缺少 name' });
    const enabled = body.enabled === true;
    const out = applyWrite((text) => toggleBundle(text, bname, enabled));
    if (out.ok && enabled && resolveModuleDir(profileDir, bname) === null) {
      out.message += '（提醒：' + bname + ' 当前不可从 profile node_modules 解析；若重启后未加载，请先把它链接进 profile）';
    }
    return json(res, 200, { ...out, name: bname, enabled });
  }

  const routes = [
    { path: API + '/list', run: (_req, res) => json(res, 200, buildList()) },
    { path: API + '/toggle', run: (_req, res, body) => handleToggle(res, body) },
    { path: API + '/add', run: (_req, res, body) => handleAdd(res, body) },
    { path: API + '/remove', run: (_req, res, body) => handleRemove(res, body) },
  ];

  ctx.effect(() => {
    const disposers = routes.map((route) => ctx.webServer.register({
      kind: 'exact',
      path: route.path,
      handler: async (req, res) => {
        try {
          if (!isLoopbackOrigin(req.headers.origin)) return json(res, 403, { ok: false, message: 'origin 未被允许' });
          const body = req.method === 'POST' ? await requestBody(req) : {};
          return route.run(req, res, body);
        } catch (e) {
          return json(res, 200, { ok: false, message: String(e?.message ?? e) });
        }
      },
    }));
    return () => { for (const d of disposers) d(); };
  }, 'plugin-manager: routes');
}
