import {resolve, join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {readFile} from 'node:fs/promises';

export const install = resolve(process.env.DSH_INSTALL_PATH ?? 'C:/Users/34021/AppData/Local/DSH Desktop');
export const moduleOf = name => import(pathToFileURL(join(install, 'kernel/node_modules/@deepseek-ai', name, 'lib/index.js')));
export async function backend(root, cacheSize = 5) {
  const version = JSON.parse(await readFile(join(install, 'kernel/package.json'), 'utf8')).version;
  if (version !== '0.1.2-rc.1') throw new Error(`Benchmark fixture adapter not verified for ${version}`);
  const [{Context}, {SessionStore}, {JsonlSessionPersistence}] = await Promise.all([
    moduleOf('cordis'), moduleOf('dsh-session'), moduleOf('dsh-session-persistence-jsonl'),
  ]);
  const ctx = new Context();
  await ctx.plugin(SessionStore);
  await ctx.plugin(JsonlSessionPersistence, {root, preparedSessionCacheSize: cacheSize});
  return {ctx, persistence: ctx.sessionPersistence, close: () => ctx.fiber.dispose()};
}
