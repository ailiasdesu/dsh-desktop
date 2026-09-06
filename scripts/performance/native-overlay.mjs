import {pathToFileURL} from 'node:url';
import {join,resolve} from 'node:path';
import {writeFile} from 'node:fs/promises';
import {readFileSync} from 'node:fs';

export function nativeGate(resourceRoot){
  const addon=join(resourceRoot,'native','dsh_native_reader.node');
  return readFileSync(new URL('../../desktop/native/activation.js',import.meta.url),'utf8').trim().replace('__NATIVE_ADDON__',JSON.stringify(addon));
}
export function nativeOverlay(resourceRoot){
  const gate=nativeGate(resourceRoot);
  return [
    '# Desktop-owned optional native read acceleration. Remove this overlay to use the original backend.',
    '- id: session-persistence-jsonl',
    "  name: '@deepseek-ai/dsh-session-persistence-jsonl'",
    `  disabled: !!js ${JSON.stringify(gate)}`,
    '- insert:',
    '    - id: desktop-native-persistence',
    `      name: ${JSON.stringify(pathToFileURL(join(resourceRoot,'desktop/native/persistence-plugin.mjs')).href)}`,
    `      disabled: !!js ${JSON.stringify('!'+gate)}`,
    '    - id: desktop-native-tools',
    `      name: ${JSON.stringify(pathToFileURL(join(resourceRoot,'desktop/native/tools-plugin.mjs')).href)}`,
    `      disabled: !!js ${JSON.stringify('!'+gate)}`,
    '',
  ].join('\n');
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  await writeFile(resolve(process.argv[3]),nativeOverlay(resolve(process.argv[2])));
}
