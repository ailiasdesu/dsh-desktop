
import {
  MANAGED_BEGIN, MANAGED_END, TARGETS, TARGET_IDS,
  findManagedBlock, renderManagedBlock, parseManagedEntries,
  findExternalTargetEntries, normalizeSettings, upsertManaged, removeManaged,
  parseCatalog, backupFile, cleanupBackups, atomicWrite, isYamlEmpty,
} from '../lib/store.js';
import { kernelCatalog, mergeCatalogs } from '../lib/catalog.js';
import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const NL = String.fromCharCode(10);
const CRLF = String.fromCharCode(13) + String.fromCharCode(10);
let failed = 0;
function assert(cond, label) {
  if (cond) console.log('  PASS  ' + label);
  else { failed += 1; console.log('  FAIL  ' + label); }
}

const work = join(tmpdir(), 'dsh-sm-test-' + Date.now());
mkdirSync(work, { recursive: true });

// ── fixture：仿真实 ~/.dsh/profiles/web/cordis.patch.yml（注释 + insert 块 + !!js 表达式，结尾换行）──
const FIXTURE = [
  '# Your patch layer for this dsh profile, applied after every bundle layer:',
  '# a top-level YAML array of loader patch entries.',
  '',
  '# oh-my-dsh plugins (source-built, linked into profile node_modules)',
  '- insert:',
  '    - id: omd-apply-patch',
  "      name: '@oh-my-dsh/apply-patch'",
  '    - id: omd-checkpoint',
  "      name: '@oh-my-dsh/checkpoint'",
  '',
  '# Web host port override',
  '- id: webserver',
  "  name: '@deepseek-ai/dsh-host-webserver'",
  '  config:',
  "    host: !!js ctx.webStartup.host ?? '127.0.0.1'",
  '    port: !!js ctx.webStartup.port ?? 3379',
  '',
].join(NL);

console.log('== 1. TARGETS 契约 ==');
assert(TARGETS.length === 2 && TARGET_IDS.join(',') === 'tool-subagent,tool-subagent-fork', 'two targets in order');
assert(TARGETS[0].toolName === 'subagent' && TARGETS[0].mode === 'continuable', 'subagent continuable');
assert(TARGETS[1].toolName === 'subagent_fork' && TARGETS[1].mode === 'one-shot', 'subagent_fork one-shot');

console.log('== 2. upsert 到空文件 ==');
const e1 = upsertManaged('', 'tool-subagent', { provider: 'gorouter', model: 'claude-opus-5', reasoningEffort: 'high' });
assert(e1.ok && e1.changed, 'empty upsert ok');
assert(e1.text.startsWith(MANAGED_BEGIN) && e1.text.endsWith(MANAGED_END + NL), 'block only, sentinel wrapped');
const e1e = parseManagedEntries(findManagedBlock(e1.text).body);
assert(e1e['tool-subagent'].provider === 'gorouter' && e1e['tool-subagent'].model === 'claude-opus-5' && e1e['tool-subagent'].reasoningEffort === 'high', 'roundtrip parse');
assert(e1e['tool-subagent'].maxTokens === undefined, 'maxTokens absent when unset');

console.log('== 3. upsert 追加到 fixture：块外字节不变 ==');
const a1 = upsertManaged(FIXTURE, 'tool-subagent', { provider: 'gorouter', model: 'claude-opus-5', reasoningEffort: 'high' });
assert(a1.ok && a1.changed, 'append ok');
assert(a1.text.slice(0, FIXTURE.length) === FIXTURE, 'prefix byte-identical');
assert(a1.text.split(MANAGED_BEGIN).length === 2 && a1.text.split(MANAGED_END).length === 2, 'exactly one sentinel pair');
assert(a1.text.indexOf(NL + NL + MANAGED_BEGIN) === FIXTURE.length - 1, 'one blank line separates block');

console.log('== 4. 幂等：同值再写 changed=false ==');
const a2 = upsertManaged(a1.text, 'tool-subagent', { provider: 'gorouter', model: 'claude-opus-5', reasoningEffort: 'high' });
assert(a2.ok && a2.changed === false, 'same value → no change');
assert(a2.text === a1.text, 'text untouched');

console.log('== 5. 更新与第二目标 ==');
const a3 = upsertManaged(a1.text, 'tool-subagent', { provider: 'ddshub', model: 'claude-fable-5', reasoningEffort: 'max', maxTokens: 64000 });
assert(a3.ok && a3.changed, 'update ok');
assert(a3.text.slice(0, FIXTURE.length) === FIXTURE, 'update keeps prefix bytes');
assert(a3.text.split(MANAGED_BEGIN).length === 2, 'still one block');
const a3e = parseManagedEntries(findManagedBlock(a3.text).body);
assert(a3e['tool-subagent'].provider === 'ddshub' && a3e['tool-subagent'].maxTokens === 64000, 'updated values');
const a4 = upsertManaged(a3.text, 'tool-subagent-fork', { provider: 'opencode-go', model: 'deepseek-v4-flash' });
assert(a4.ok && a4.changed, 'fork upsert ok');
const a4e = parseManagedEntries(findManagedBlock(a4.text).body);
assert(a4e['tool-subagent'] !== undefined && a4e['tool-subagent-fork'] !== undefined, 'both entries present');
assert(a4e['tool-subagent-fork'].reasoningEffort === undefined, 'fork effort absent (跟随继承)');
assert(a4.text.indexOf('- id: tool-subagent' + NL) < a4.text.indexOf('- id: tool-subagent-fork' + NL), 'deterministic order');
assert(a4.text.split(MANAGED_BEGIN).length === 2, 'two entries share one block');

console.log('== 6. 移除与整块回收（字节级 roundtrip）==');
const r1 = removeManaged(a4.text, 'tool-subagent');
assert(r1.ok && r1.changed, 'remove subagent ok');
const r1e = parseManagedEntries(findManagedBlock(r1.text).body);
assert(r1e['tool-subagent'] === undefined && r1e['tool-subagent-fork'] !== undefined, 'fork survives');
assert(r1.text.slice(0, FIXTURE.length) === FIXTURE, 'remove keeps prefix bytes');
const r2 = removeManaged(r1.text, 'tool-subagent-fork');
assert(r2.ok && r2.changed, 'remove fork ok');
assert(r2.text === FIXTURE, 'block + separator fully reclaimed: byte-identical roundtrip');
assert(findManagedBlock(r2.text) === null, 'no block left');
const r3 = removeManaged(r2.text, 'tool-subagent');
assert(r3.ok === false, 'remove on unmanaged refused');
const r4 = removeManaged(a4.text, 'tool-subagent-fork');
const r5 = removeManaged(r4.text, 'tool-subagent');
assert(r5.ok && r5.text === FIXTURE, 'reverse-order removal also roundtrips');

console.log('== 7. 空文件 roundtrip ==');
const q1 = upsertManaged('', 'tool-subagent-fork', { provider: 'p', model: 'm' });
const q2 = removeManaged(q1.text, 'tool-subagent-fork');
assert(q2.ok && q2.text === '', 'empty-file roundtrip to empty string');

console.log('== 8. 冲突条目拒写 ==');
const CONFLICT_TOP = FIXTURE + ['- id: tool-subagent', "  name: '@deepseek-ai/dsh-tool-subagent'", '  config:', '    agentOptions:', "      provider: 'x'", ''].join(NL);
const c1 = upsertManaged(CONFLICT_TOP, 'tool-subagent', { provider: 'p', model: 'm' });
assert(c1.ok === false && c1.error.indexOf('tool-subagent') >= 0 && c1.error.indexOf('第 17 行') >= 0, 'top-level external entry refused with line no');
const c1b = upsertManaged(CONFLICT_TOP, 'tool-subagent-fork', { provider: 'p', model: 'm' });
assert(c1b.ok === false, 'any external target entry blocks all writes');
const CONFLICT_INSERT = FIXTURE + ['- insert:', '    - id: tool-subagent-fork', "      name: 'x'", ''].join(NL);
const c2 = upsertManaged(CONFLICT_INSERT, 'tool-subagent-fork', { provider: 'p', model: 'm' });
assert(c2.ok === false && c2.error.indexOf('tool-subagent-fork') >= 0, 'insert-child entry refused');
const CONFLICT_QUOTED = FIXTURE + "- id: 'tool-subagent'" + NL;
const c3 = upsertManaged(CONFLICT_QUOTED, 'tool-subagent', { provider: 'p', model: 'm' });
assert(c3.ok === false, 'quoted id refused');
const NEARMISS = FIXTURE + ['- id: tool-subagent-x', '- id: xtool-subagent', '- id: tool-subagent-forked', ''].join(NL);
const c4 = upsertManaged(NEARMISS, 'tool-subagent', { provider: 'p', model: 'm' });
assert(c4.ok === true, 'near-miss ids do not false-positive');
assert(findExternalTargetEntries(a4.text).length === 0, 'managed-block ids excluded from conflict scan');
const MIXED = a4.text + '- id: tool-subagent' + NL;
const c5 = findExternalTargetEntries(MIXED);
assert(c5.length === 1 && c5[0].id === 'tool-subagent', 'only external occurrence reported');
assert(upsertManaged(MIXED, 'tool-subagent', { provider: 'p', model: 'm' }).ok === false, 'mixed managed+external refused');

console.log('== 9. 哨兵破损拒写 ==');
const BROKEN = FIXTURE + MANAGED_BEGIN + NL + '- id: tool-subagent' + NL;
const b1 = upsertManaged(BROKEN, 'tool-subagent', { provider: 'p', model: 'm' });
assert(b1.ok === false && b1.error.indexOf('哨兵') >= 0, 'BEGIN without END refused');
const b2 = removeManaged(BROKEN, 'tool-subagent');
assert(b2.ok === false, 'remove on broken refused');
const DOUBLE = a1.text + NL + a1.text;
assert(upsertManaged(DOUBLE, 'tool-subagent', { provider: 'p', model: 'm' }).ok === false, 'duplicated sentinels refused');

console.log('== 10. normalizeSettings 校验 ==');
assert(normalizeSettings({ model: 'm' }).ok === false, 'missing provider refused');
assert(normalizeSettings({ provider: 'p' }).ok === false, 'missing model refused');
assert(normalizeSettings({ provider: 'p', model: 'm', maxTokens: 0 }).ok === false, 'maxTokens 0 refused');
assert(normalizeSettings({ provider: 'p', model: 'm', maxTokens: 1.5 }).ok === false, 'maxTokens 1.5 refused');
assert(normalizeSettings({ provider: 'p', model: 'm', maxTokens: '64000' }).value.maxTokens === 64000, 'numeric string accepted');
assert(normalizeSettings({ provider: 'p', model: 'm', reasoningEffort: '  high ' }).value.reasoningEffort === 'high', 'effort trimmed');
assert(normalizeSettings({ provider: 'p', model: 'm', reasoningEffort: '' }).value.reasoningEffort === undefined, 'empty effort absent');
assert(normalizeSettings({ provider: 'p#q', model: 'm' }).ok === false, 'hash in provider refused');
assert(upsertManaged(FIXTURE, 'nope', { provider: 'p', model: 'm' }).ok === false, 'unknown target id refused');

console.log('== 11. CRLF 保持 ==');
const FIXTURE_CRLF = FIXTURE.split(NL).join(CRLF);
const w1 = upsertManaged(FIXTURE_CRLF, 'tool-subagent', { provider: 'p', model: 'm' });
assert(w1.ok && w1.text.slice(0, FIXTURE_CRLF.length) === FIXTURE_CRLF, 'CRLF prefix byte-identical');
assert(w1.text.indexOf(CRLF + '- id: tool-subagent' + CRLF) >= 0, 'block rendered with CRLF');
const w2 = removeManaged(w1.text, 'tool-subagent');
assert(w2.ok && w2.text === FIXTURE_CRLF, 'CRLF roundtrip byte-identical');

console.log('== 12. YAML 引号与特殊值 ==');
const s1 = upsertManaged('', 'tool-subagent', { provider: 'openrouter', model: 'anthropic/claude-fable-5', reasoningEffort: "o'brien" });
assert(s1.text.indexOf("      model: 'anthropic/claude-fable-5'") >= 0, 'slash model single-quoted');
assert(s1.text.indexOf("      reasoningEffort: 'o''brien'") >= 0, 'single quote escaped by doubling');
const s1e = parseManagedEntries(findManagedBlock(s1.text).body);
assert(s1e['tool-subagent'].model === 'anthropic/claude-fable-5' && s1e['tool-subagent'].reasoningEffort === "o'brien", 'quoted values roundtrip');

console.log('== 13. parseCatalog（含 name-先行 / off 空值 / 无 efforts / 噪声键）==');
const SETTINGS = [
  'ui-onboarding:',
  '  welcomeNoticeVersion: 2026-08-13.1',
  'agent-default-model:',
  '  provider: vision-toolkit-xiaol',
  '  model: claude-opus-5',
  'llm-pi-ai:',
  '  providers:',
  '    opencode-go:',
  '      models:',
  '        - id: minimax-m3',
  '          name: MiniMax-M3',
  '          contextWindow: 1000000',
  '          maxTokens: 131072',
  '        - id: deepseek-v4-flash',
  '          name: DeepSeek V4 Flash',
  '          contextWindow: 1000000',
  '          maxTokens: 384000',
  '      apiKeyEnv: OPENCODE_GO_API_KEY',
  '    openrouter:',
  '      reasoning: max',
  '      models:',
  '        - id: ai21/jamba-large-1.7',
  '          name: "AI21: Jamba Large 1.7"',
  '          contextWindow: 256000',
  '          maxTokens: 4096',
  '        - name: Ox Alpha',
  '          id: stealth/ox-alpha',
  '          contextWindow: 1048576',
  '          maxTokens: 131072',
  '          input:',
  '            - text',
  '            - image',
  '          reasoningEfforts:',
  '            off:',
  '            minimal: minimal',
  '            low: low',
  '            medium: medium',
  '            high: high',
  '            xhigh: xhigh',
  '            max: max',
  '      apiKeyEnv: OPENROUTER_API_KEY',
  '    gorouter:',
  '      apiKeyEnv: GOROUTER_API_KEY',
  '      api: anthropic-messages',
  '      baseURL: https://gorouter.app',
  '      models:',
  '        - id: claude-opus-5',
  '          compat:',
  '            forceAdaptiveThinking: true',
  '          name: Claude Opus 5',
  '          contextWindow: 1000000',
  '          maxTokens: 65536',
  '          reasoningEfforts:',
  '            off: off',
  '            minimal: minimal',
  '            low: low',
  '            medium: medium',
  '            high: high',
  '            xhigh: xhigh',
  '            max: max',
  'agent-loop:',
  '  maxParallelToolCalls: 50',
  '',
].join(NL);
const cat = parseCatalog(SETTINGS);
assert(cat.providers.length === 3, '3 providers, got ' + cat.providers.length);
assert(cat.providers.map((p) => p.name).join(',') === 'opencode-go,openrouter,gorouter', 'provider order preserved');
const og = cat.providers[0];
assert(og.models.length === 2 && og.models[0].id === 'minimax-m3' && og.models[1].id === 'deepseek-v4-flash', 'opencode-go models');
assert(og.models[0].efforts.length === 0, 'no reasoningEfforts → empty efforts');
assert(og.models[0].contextWindow === 1000000 && og.models[0].maxTokens === 131072, 'numbers parsed');
const or = cat.providers[1];
assert(or.models.length === 2, 'openrouter 2 models (reasoning: max 噪声键不吞 models)');
assert(or.models[0].name === 'AI21: Jamba Large 1.7', 'double-quoted name unquoted');
assert(or.models[1].id === 'stealth/ox-alpha' && or.models[1].name === 'Ox Alpha', 'name-first item parsed');
assert(or.models[1].efforts.join(',') === 'off,minimal,low,medium,high,xhigh,max', 'efforts keys in order incl. off with empty value');
const gr = cat.providers[2];
assert(gr.models.length === 1 && gr.models[0].id === 'claude-opus-5', 'gorouter model (compat 嵌套不干扰)');
assert(gr.models[0].efforts.length === 7, 'gorouter efforts 7');
assert(parseCatalog('').providers.length === 0, 'empty settings → empty catalog');
assert(parseCatalog('foo:' + NL + '  bar: 1' + NL).providers.length === 0, 'no llm-pi-ai → empty catalog');

console.log('== 13b. isYamlEmpty（reset 删除文件判定）==');
assert(isYamlEmpty('') === true, 'empty text yaml-empty');
assert(isYamlEmpty('# only comments' + NL + NL + '  # more' + NL) === true, 'comments-only yaml-empty');
assert(isYamlEmpty('- id: x' + NL) === false, 'entry not yaml-empty');
assert(isYamlEmpty('# c' + NL + '- id: x' + NL) === false, 'mixed not yaml-empty');
assert(isYamlEmpty(FIXTURE) === false, 'fixture not yaml-empty');

console.log('== 14. 备份与轮替（保留最近 10 份）==');
const target = join(work, 'cordis.patch.yml');
writeFileSync(target, FIXTURE, 'utf8');
for (let i = 0; i < 12; i++) { backupFile(target); }
const removed = cleanupBackups(target, 10);
const baks = readdirSync(work).filter((n) => n.indexOf('.bak-sm-') >= 0);
assert(baks.length === 10, 'kept 10 backups, got ' + baks.length + ' (removed=' + removed + ')');
const newest = baks.map((n) => Number(n.slice(n.indexOf('.bak-sm-') + 8))).sort((a, b) => b - a)[0];
assert(readFileSync(target + '.bak-sm-' + newest, 'utf8') === FIXTURE, 'backup content equals original');

console.log('== 15. 原子写无残留 ==');
atomicWrite(target, a4.text);
assert(readFileSync(target, 'utf8') === a4.text, 'atomic write applied');
assert(readdirSync(work).filter((n) => n.indexOf('.tmp-sm-') >= 0).length === 0, 'no tmp leftovers');

console.log('== 16. 自定义直填（目录外任意串）set 往返 ==');
const pin = upsertManaged(FIXTURE, 'tool-subagent', { provider: 'deepseek-official', model: 'deepseek-v4-flash-vision-exp', reasoningEffort: 'high' });
assert(pin.ok && pin.changed, 'builtin provider pinned (主用例：目录外内置提供商)');
const pinE = parseManagedEntries(findManagedBlock(pin.text).body);
assert(pinE['tool-subagent'].provider === 'deepseek-official' && pinE['tool-subagent'].model === 'deepseek-v4-flash-vision-exp' && pinE['tool-subagent'].reasoningEffort === 'high', 'headline values roundtrip');
assert(pin.text.slice(0, FIXTURE.length) === FIXTURE, 'pin keeps prefix bytes');
const weird = { provider: 'my provider: v2', model: '模型/α β', reasoningEffort: "ultra 'x' 强" };
const wf = upsertManaged('', 'tool-subagent-fork', weird);
assert(wf.ok, 'arbitrary strings accepted (space/colon/CJK/quote)');
const wfE = parseManagedEntries(findManagedBlock(wf.text).body);
assert(wfE['tool-subagent-fork'].provider === weird.provider && wfE['tool-subagent-fork'].model === weird.model && wfE['tool-subagent-fork'].reasoningEffort === weird.reasoningEffort, 'arbitrary strings roundtrip');
assert(normalizeSettings({ provider: '  ', model: 'm' }).ok === false, 'whitespace-only provider refused (空串校验兜底)');
assert(normalizeSettings({ provider: 'p', model: '  ' }).ok === false, 'whitespace-only model refused');

console.log('== 17. mergeCatalogs（内核∪settings 去重合并）==');
const kern = [
  { name: 'deepseek-official', models: [{ id: 'deepseek-v4-flash-vision-exp', name: 'DeepSeek V4 Flash Vision (exp)', efforts: ['low', 'high'] }] },
  { name: 'gorouter', models: [{ id: 'claude-opus-5', name: 'Claude Opus 5', efforts: ['off', 'max'] }] },
];
const merged = mergeCatalogs(cat.providers, kern);
assert(merged.map((p) => p.name).join(',') === 'deepseek-official,gorouter,opencode-go,openrouter', 'kernel first, settings-only appended, dedupe by name');
const mGr = merged.find((p) => p.name === 'gorouter');
assert(mGr.source === 'kernel+settings', 'both-source provider labeled');
assert(mGr.models.length === 1, 'same model id deduped');
assert(mGr.models[0].efforts.join(',') === 'off,max,minimal,low,medium,high,xhigh', 'efforts union keeps kernel order then settings extras');
assert(mGr.models[0].contextWindow === 1000000 && mGr.models[0].maxTokens === 65536, 'settings numbers backfill kernel entry');
const mDs = merged.find((p) => p.name === 'deepseek-official');
assert(mDs.source === 'kernel' && mDs.models[0].id === 'deepseek-v4-flash-vision-exp', 'kernel-only provider present');
const mOg = merged.find((p) => p.name === 'opencode-go');
assert(mOg.source === 'settings' && mOg.models.length === 2, 'settings-only provider intact');
assert(mergeCatalogs([], []).length === 0, 'empty merge');
assert(mergeCatalogs(cat.providers, []).length === 3, 'kernel absent → settings only');

console.log('== 18. kernelCatalog（伪 ctx.llm 枚举 + 容错）==');
const fakeLlm = {
  listProviders() { return [{ id: 'deepseek-official', name: 'DeepSeek' }, { id: 'broken', name: 'Broken' }]; },
  async listModels(id) {
    if (id === 'broken') throw new Error('adapter offline');
    return [{ id: 'deepseek-v4-flash-vision-exp', name: 'DeepSeek V4 Flash Vision (exp)' }, { id: 'no-efforts' }];
  },
  async resolveModelInfo(_id, modelId) {
    if (modelId === 'no-efforts') return {};
    return { reasoning: { efforts: [{ id: 'low', name: 'Low' }, { id: 'high', name: 'High' }], defaultEffort: 'low' } };
  },
};
const kc = await kernelCatalog({ get: (k) => (k === 'llm' ? fakeLlm : undefined) });
assert(kc.available === true && kc.providers.length === 1, 'available; broken provider dropped to failures');
assert(kc.providers[0].name === 'deepseek-official' && kc.providers[0].models.length === 2, 'models mapped');
assert(kc.providers[0].models[0].efforts.join(',') === 'low,high', 'reasoning efforts ids extracted');
assert(kc.providers[0].models[1].efforts.length === 0 && kc.providers[0].models[1].name === 'no-efforts', 'no reasoning → empty efforts; name falls back to id');
assert(kc.failures.length === 1 && kc.failures[0].provider === 'broken', 'per-provider failure captured');
const kcNo = await kernelCatalog({ get: () => undefined });
assert(kcNo.available === false && kcNo.providers.length === 0, 'llm service absent → unavailable');
const kcNoGet = await kernelCatalog({});
assert(kcNoGet.available === false, 'ctx without get() → unavailable');

console.log('== 19. client.js 静态断言（自定义直填接线）==');
const clientSrc = readFileSync(new URL('../lib/client.js', import.meta.url), 'utf8');
assert(clientSrc.indexOf('__sm-custom__') >= 0, 'CUSTOM sentinel present');
assert(clientSrc.split('自定义…').length - 1 >= 3, 'custom option appended in all three dropdown paths');
assert(clientSrc.indexOf('resolveSel') >= 0 && clientSrc.indexOf('cancelCustom') >= 0, 'resolve/cancel helpers wired');
assert(clientSrc.indexOf('kernelFailures') >= 0 && clientSrc.indexOf('kernelAvailable') >= 0, 'kernel catalog status surfaced');

rmSync(work, { recursive: true, force: true });
console.log(failed === 0 ? 'ALL TESTS PASSED' : failed + ' TEST(S) FAILED');
process.exit(failed === 0 ? 0 : 1);
