
import {
  listBundles, addBundle, removeBundle, toggleBundle, findBundlesBlock,
  backupFile, cleanupBackups, atomicWrite, resolveModuleDir, readPackageMeta,
  parsePatchInserts, listInstalledDirs, PROTECTED_BUNDLES,
} from '../lib/bundle-store.js';
import { existsSync, readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const NL = String.fromCharCode(10);
let failed = 0;
function assert(cond, label) {
  if (cond) console.log('  PASS  ' + label);
  else { failed += 1; console.log('  FAIL  ' + label); }
}

const work = join(tmpdir(), 'dsh-pm-test-' + Date.now());
mkdirSync(work, { recursive: true });

// ── fixture：真实结构的多行 profile manifest（仿 ~/.dsh/profiles/web/package.json）──
const FIXTURE = [
  '{',
  '  "name": "dsh-profile-web",',
  '  "private": true,',
  '  "dependencies": {',
  '    "some-plugin": "file:../foo"',
  '  },',
  '  "dsh": {',
  '    "profile": {',
  '      "bundles": [',
  '        "@deepseek-ai/dsh-base",',
  '        "@deepseek-ai/dsh-web-app",',
  '        "@nanmicoder/dsh-agent-teams",',
  '        "@dsh-external/workflow",',
  '        "dsh-memory-evolve",',
  '        "@dsh-external/dsh-session-manager"',
  '      ]',
  '    }',
  '  }',
  '}',
].join(NL);
const manifest = join(work, 'package.json');
writeFileSync(manifest, FIXTURE, 'utf8');

console.log('== 1. listBundles ==');
const names = listBundles(readFileSync(manifest, 'utf8'));
assert(names.length === 6, 'list length 6, got ' + names.length);
assert(names[0] === '@deepseek-ai/dsh-base' && names[5] === '@dsh-external/dsh-session-manager', 'order preserved');
assert(readFileSync(manifest, 'utf8') === FIXTURE, 'list is read-only (byte-identical)');

console.log('== 2. addBundle (middle/last & formatting) ==');
let r1 = addBundle(readFileSync(manifest, 'utf8'), '@dsh-external/dsh-plugin-manager');
assert(r1.ok && r1.changed, 'add returns ok');
const t1 = r1.text;
const j1 = JSON.parse(t1);
assert(j1.dsh.profile.bundles.length === 7, 'parsed bundles length 7');
assert(j1.dsh.profile.bundles[6] === '@dsh-external/dsh-plugin-manager', 'new bundle is last');
const beforeLines = FIXTURE.split(NL);
const afterLines = t1.split(NL);
assert(afterLines.length === beforeLines.length + 1, 'only one line added');
assert(afterLines[0] === beforeLines[0] && afterLines[1] === beforeLines[1] && afterLines[2] === beforeLines[2], 'header lines untouched');
assert(afterLines[afterLines.length - 1] === beforeLines[beforeLines.length - 1], 'tail untouched');
assert(afterLines[12] === beforeLines[12], 'dependencies(dsh:) untouched');

console.log('== 3. 重复添加拒绝 ==');
const r2 = addBundle(t1, '@dsh-external/dsh-plugin-manager');
assert(!r2.ok && !r2.changed, 'duplicate add rejected');

console.log('== 4. removeBundle（中间项）==');
const t2 = removeBundle(t1, '@nanmicoder/dsh-agent-teams');
assert(t2.ok, 'remove middle ok');
const j2 = JSON.parse(t2.text);
assert(j2.dsh.profile.bundles.length === 6, 'parsed length 6 after remove');
assert(!j2.dsh.profile.bundles.includes('@nanmicoder/dsh-agent-teams'), 'middle item gone');
assert(j2.dsh.profile.bundles[0] === '@deepseek-ai/dsh-base', 'order preserved');

console.log('== 5. removeBundle（最后一项且无逗号）==');
const t3 = removeBundle(t2.text, '@dsh-external/dsh-plugin-manager');
const j3 = JSON.parse(t3.text);
assert(j3.dsh.profile.bundles.length === 5, 'length 5 after last remove');
const l3 = t3.text.split(NL);
const lastItemLine = l3[l3.length - 4];
assert(!lastItemLine.trim().endsWith(','), 'new last item has no trailing comma: ' + lastItemLine.trim());

console.log('== 6. toggle off/on 往返 ==');
const off = toggleBundle(readFileSync(manifest, 'utf8'), '@dsh-external/dsh-session-manager', false);
assert(off.ok, 'toggle off ok');
assert(!JSON.parse(off.text).dsh.profile.bundles.includes('@dsh-external/dsh-session-manager'), 'off removes from bundles');
const on = toggleBundle(off.text, '@dsh-external/dsh-session-manager', true);
assert(on.ok, 'toggle on ok');
const jOn = JSON.parse(on.text);
assert(jOn.dsh.profile.bundles.includes('@dsh-external/dsh-session-manager'), 'on restores bundle');
assert(jOn.dsh.profile.bundles.length === 6, 'roundtrip length 6');

console.log('== 7. 保护清单 ==');
const pr = removeBundle(readFileSync(manifest, 'utf8'), '@deepseek-ai/dsh-base');
assert(!pr.ok && pr.error.includes('受保护'), 'dsh-base remove refused');
const pr2 = toggleBundle(readFileSync(manifest, 'utf8'), '@deepseek-ai/dsh-web-app', false);
assert(!pr2.ok, 'dsh-web-app toggle-off refused');
assert(PROTECTED_BUNDLES.length === 2, 'protected list has 2 core bundles');

console.log('== 8. 单行形 bundles 数组 ==');
const SINGLE = '{"name":"x","dsh":{"profile":{"bundles":["a","b"]}},"t":1}';
const s1 = addBundle(SINGLE, 'c');
assert(s1.ok, 'single-line add ok');
assert(JSON.parse(s1.text).dsh.profile.bundles.join(',') === 'a,b,c', 'single-line add content');
const s2 = removeBundle(s1.text, 'a');
assert(s2.ok, 'single-line remove ok');
assert(JSON.parse(s2.text).dsh.profile.bundles.join(',') === 'b,c', 'single-line remove content');
assert(JSON.parse(s2.text).t === 1, 'single-line other field preserved');
assert(JSON.parse(s2.text).name === 'x', 'single-line name preserved');
const SINGLE2 = '{"dsh":{"profile":{"bundles":["x"]}}}';
const z = addBundle(SINGLE2, 'y');
assert(z.ok && JSON.parse(z.text).dsh.profile.bundles.length === 2, 'single-line add to 1-item array');

console.log('== 9. 不认识的结构拒绝编辑 ==');
const WEIRD = '{"dsh":{"profile":{"bundles":[]}}}';
const w1 = addBundle(WEIRD, 'a');
assert(w1.ok, 'empty single-line array add ok');
const WEIRD2 = '{"dsh":{"profile":{"bundles":{"nested":1}}}}';
const w2 = addBundle(WEIRD2, 'a');
assert(!w2.ok, 'non-array bundles refused');
const NOMANIFEST = '{"name":"x","dsh":{"profile":{}}}';
assert(addBundle(NOMANIFEST, 'a').ok === false, 'missing bundles section refused');
// 多行形空数组
const EMPTYMULTI = ['{', '  "dsh": {', '    "profile": {', '      "bundles": [', '      ]', '    }', '  }', '}'].join(NL);
const w3 = addBundle(EMPTYMULTI, 'a');
assert(w3.ok && JSON.parse(w3.text).dsh.profile.bundles.length === 1, 'empty multiline array add ok');

console.log('== 10. 备份与轮替（保留最近 10 份）==');
for (let i = 0; i < 12; i++) { backupFile(manifest); }
const removed = cleanupBackups(manifest, 10);
const afterBak = readdirSync(work).filter((n) => n.indexOf('.bak-pm-') >= 0);
assert(afterBak.length === 10, 'kept 10 backups, got ' + afterBak.length + ' (removed=' + removed + ')');
const newest = afterBak.slice().sort().reverse()[0];
assert(readFileSync(join(work, newest), 'utf8') === FIXTURE, 'backup content equals original');

console.log('== 11. 原子写 ==');
const target = join(work, 'atomic.json');
writeFileSync(target, FIXTURE, 'utf8');
atomicWrite(target, JSON.stringify({ ok: 1 }));
assert(JSON.parse(readFileSync(target, 'utf8')).ok === 1, 'atomic write applied');
assert(readdirSync(work).filter((n) => n.indexOf('.tmp-pm-') >= 0).length === 0, 'no tmp leftovers');

console.log('== 12. resolveModuleDir（临时 node_modules 链路）==');
const prof = join(work, 'profile');
mkdirSync(join(prof, 'node_modules', '@scope', 'pkg-a'), { recursive: true });
mkdirSync(join(prof, 'node_modules', 'plainpkg'), { recursive: true });
writeFileSync(join(prof, 'package.json'), '{}', 'utf8');
writeFileSync(join(prof, 'node_modules', '@scope', 'pkg-a', 'package.json'), JSON.stringify({ name: '@scope/pkg-a', description: 'A' }), 'utf8');
mkdirSync(join(prof, 'node_modules', '@scope', 'pkg-a', 'lib'), { recursive: true });
writeFileSync(join(prof, 'node_modules', '@scope', 'pkg-a', 'lib', 'index.js'), 'export const a = 1;', 'utf8');
writeFileSync(join(prof, 'node_modules', 'plainpkg', 'package.json'), JSON.stringify({ name: 'plainpkg' }), 'utf8');
mkdirSync(join(prof, 'node_modules', 'plainpkg', 'lib'), { recursive: true });
writeFileSync(join(prof, 'node_modules', 'plainpkg', 'lib', 'index.js'), 'export const a = 1;', 'utf8');
// 非插件形态（仅有 package.json 无 lib/ 无 dsh 元数据）：应被 isPluginLike 过滤
mkdirSync(join(prof, 'node_modules', 'library-only'), { recursive: true });
writeFileSync(join(prof, 'node_modules', 'library-only', 'package.json'), JSON.stringify({ name: 'library-only' }), 'utf8');
const ra = resolveModuleDir(prof, '@scope/pkg-a');
assert(ra !== null && ra.endsWith(join('@scope', 'pkg-a')), 'scope resolve found');
const rb = resolveModuleDir(prof, 'plainpkg');
assert(rb !== null && rb.endsWith('plainpkg'), 'plain resolve found');
const rc = resolveModuleDir(prof, 'no-such-pkg');
assert(rc === null, 'missing resolve returns null');
const meta = readPackageMeta(ra);
assert(meta.name === '@scope/pkg-a' && meta.description === 'A', 'readPackageMeta works');

console.log('== 13. parsePatchInserts（只读）==');
const PATCH = [
  '# comment',
  '- insert:',
  '    - id: omd-apply-patch',
  "      name: '@oh-my-dsh/apply-patch'",
  '    - id: omd-checkpoint',
  "      name: '@oh-my-dsh/checkpoint'",
  '- id: webserver',
  '  config:',
  '    port: 3379',
  '- insert:',
  '    - id: openbiliclaw',
  "      name: '@openbiliclaw/dsh-plugin'",
].join(NL);
const inserts = parsePatchInserts(PATCH);
assert(inserts.length === 3, '3 insert rows parsed, got ' + inserts.length);
assert(inserts[0].id === 'omd-apply-patch' && inserts[0].name === '@oh-my-dsh/apply-patch', 'row1');
assert(inserts[1].id === 'omd-checkpoint' && inserts[1].name === '@oh-my-dsh/checkpoint', 'row2');
assert(inserts[2].id === 'openbiliclaw' && inserts[2].name === '@openbiliclaw/dsh-plugin', 'row3');
assert(!inserts.some((i) => i.id === 'webserver'), 'non-insert row excluded (webserver)');

console.log('== 14. listInstalledDirs ==');
const installed = listInstalledDirs(prof, []);
assert(installed.has('@scope/pkg-a') && installed.get('@scope/pkg-a').endsWith(join('node_modules', '@scope', 'pkg-a')), 'scoped found');
assert(installed.has('plainpkg'), 'plain found');
assert(!installed.has('library-only'), 'non-plugin-like filtered out');
const excluded = listInstalledDirs(prof, ['plainpkg']);
assert(!excluded.has('plainpkg'), 'excluded filtered');

rmSync(work, { recursive: true, force: true });
console.log(failed === 0 ? 'ALL TESTS PASSED' : failed + ' TEST(S) FAILED');
process.exit(failed === 0 ? 0 : 1);
