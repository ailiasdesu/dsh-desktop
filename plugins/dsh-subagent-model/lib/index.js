/**
 * @dsh-external/dsh-subagent-model —— host 插件：子代理模型设置 REST API。
 *
 * 路由前缀 /subagent-model/api/（同源本机 Web UI；origin 围栏防跨站滥用）：
 *   GET|POST /subagent-model/api/list  -> { ok, profile, patchFile, settingsFile, targets, catalog, conflicts, blockBroken }
 *   POST     /subagent-model/api/set   { id, provider, model, reasoningEffort?, maxTokens? } -> { ok, needRestart, message, backup, block }
 *   POST     /subagent-model/api/reset { id } -> { ok, needRestart, message, backup }
 *
 * 写操作契约：唯一写入目标是 <profile>/cordis.patch.yml 的哨兵托管块；写前备份
 * （<file>.bak-sm-<epoch>，保留最近 10 份）+ 临时文件原子写；块外内容字节级不变。
 * settings.yaml 只读（模型目录 settings 侧）；内核侧目录经 ctx.get('llm') 实时枚举
 * （与官方模型选择器同源，见 catalog.js），二者合并去重进 /list 的 catalog。
 * profile package.json 不属于本插件（那是 plugin-manager 的领域）。
 */
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { basename, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  TARGETS, atomicWrite, backupFile, cleanupBackups, findExternalTargetEntries,
  findManagedBlock, isYamlEmpty, parseCatalog, parseManagedEntries, removeManaged, upsertManaged,
} from './store.js';
import { kernelCatalog, mergeCatalogs } from './catalog.js';

export const name = '@dsh-external/dsh-subagent-model';
export const inject = ['webServer'];

const API = '/subagent-model/api';
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
  try { profileDir = fileURLToPath(new URL(ctx.baseUrl ?? new URL('file:///' + process.cwd() + '/', 'file:///'))); } catch { /* fallback below */ }
  if (profileDir === null || profileDir === '' || !existsSync(join(profileDir, 'cordis.yml'))) {
    const home = process.env.DSH_HOME && process.env.DSH_HOME.trim() !== '' ? process.env.DSH_HOME : join(process.env.USERPROFILE ?? process.env.HOME ?? '', '.dsh');
    profileDir = join(home, 'profiles', 'web');
  }
  const patchPath = join(profileDir, 'cordis.patch.yml');
  const settingsPath = join(profileDir, '..', '..', 'settings.yaml');

  function patchText() { try { return readFileSync(patchPath, 'utf8'); } catch { return ''; } }
  function settingsText() { try { return readFileSync(settingsPath, 'utf8'); } catch { return ''; } }

  async function buildList() {
    const text = patchText();
    const block = findManagedBlock(text);
    const broken = block !== null && block.broken === true;
    const entries = block !== null && !broken ? parseManagedEntries(block.body) : {};
    const targets = TARGETS.map((t) => ({
      id: t.id,
      toolName: t.toolName,
      mode: t.mode,
      managed: entries[t.id] !== undefined,
      effective: entries[t.id] ?? null,
    }));
    return {
      ok: true,
      profile: { dir: profileDir, name: basename(profileDir) },
      patchFile: patchPath,
      settingsFile: settingsPath,
      blockBroken: broken ? block.reason : null,
      targets,
      conflicts: findExternalTargetEntries(text),
      catalog: await buildCatalog(),
    };
  }

  /** 目录 = 内核 llm 实时枚举（与官方模型选择器同源）∪ settings.yaml（去重合并，见 catalog.js）。 */
  async function buildCatalog() {
    const kernel = await kernelCatalog(ctx);
    const settings = parseCatalog(settingsText());
    return {
      providers: mergeCatalogs(settings.providers, kernel.providers),
      kernelAvailable: kernel.available,
      kernelFailures: kernel.failures,
    };
  }

  /**
   * 统一写流程：变换（纯函数）→ 备份（保留最近 10）→ 原子写。
   * deleteWhenYamlEmpty：变换结果只剩空行/注释时删除文件（备份保留）——内核要求
   * 存在的 profile 补丁必须是顶层数组，缺省文件才等于“无用户层”（loadProfile:564）。
   */
  function applyPatchWrite(mutate, deleteWhenYamlEmpty = false) {
    const before = patchText();
    const r = mutate(before);
    if (!r.ok) return { ok: false, message: r.error };
    if (!r.changed) return { ok: true, needRestart: false, message: '内容无变化，未写入' };
    let bak = null;
    if (existsSync(patchPath)) {
      bak = backupFile(patchPath);
      cleanupBackups(patchPath, 10);
    }
    if (deleteWhenYamlEmpty && isYamlEmpty(r.text)) {
      try { rmSync(patchPath, { force: true }); } catch (e) {
        return { ok: false, message: '删除失败：' + String(e && e.message ? e.message : e) + (bak !== null ? '（备份保留于 ' + basename(bak) + '）' : '') };
      }
      return {
        ok: true,
        needRestart: true,
        message: '托管块已移除；文件仅剩注释/空白（内核要求存在的补丁文件必须是 YAML 数组），已删除 ' + basename(patchPath) + (bak !== null ? '，备份 ' + basename(bak) : '') + '；重启 DSH 后生效',
        backup: bak !== null ? basename(bak) : null,
        block: null,
      };
    }
    try { atomicWrite(patchPath, r.text); } catch (e) {
      return { ok: false, message: '写入失败：' + String(e && e.message ? e.message : e) + (bak !== null ? '（备份保留于 ' + basename(bak) + '）' : '') };
    }
    return {
      ok: true,
      needRestart: true,
      message: '已写入 ' + basename(patchPath) + (bak !== null ? '，备份 ' + basename(bak) : '') + '；重启 DSH 后生效',
      backup: bak !== null ? basename(bak) : null,
      block: r.block ?? null,
    };
  }

  function handleSet(res, body) {
    const id = typeof body.id === 'string' ? body.id : '';
    const out = applyPatchWrite((text) => upsertManaged(text, id, body));
    return json(res, 200, { ...out, id });
  }

  function handleReset(res, body) {
    const id = typeof body.id === 'string' ? body.id : '';
    const out = applyPatchWrite((text) => removeManaged(text, id), true);
    return json(res, 200, { ...out, id });
  }

  const routes = [
    { path: API + '/list', methods: ['GET', 'POST'], run: async (_req, res) => json(res, 200, await buildList()) },
    { path: API + '/set', methods: ['POST'], run: (_req, res, body) => handleSet(res, body) },
    { path: API + '/reset', methods: ['POST'], run: (_req, res, body) => handleReset(res, body) },
  ];

  ctx.effect(() => {
    const disposers = routes.map((route) => ctx.webServer.register({
      kind: 'exact',
      path: route.path,
      handler: async (req, res) => {
        try {
          if (!isLoopbackOrigin(req.headers.origin)) return json(res, 403, { ok: false, message: 'origin 未被允许' });
          if (!route.methods.includes(req.method ?? 'GET')) return json(res, 405, { ok: false, message: '方法不允许：' + req.method });
          const body = req.method === 'POST' ? await requestBody(req) : {};
          return await route.run(req, res, body);
        } catch (e) {
          return json(res, 200, { ok: false, message: String(e && e.message ? e.message : e) });
        }
      },
    }));
    return () => { for (const d of disposers) d(); };
  }, 'subagent-model: routes');
}
