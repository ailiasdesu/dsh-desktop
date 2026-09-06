import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {runInNewContext} from 'node:vm';
import {nativeGate,nativeOverlay} from './native-overlay.mjs';
import {moduleOf} from './official.mjs';
test('managed overlay preserves configuration and leaves unsupported kernels on original backend',async t=>{
  const root=await mkdtemp(join(tmpdir(),'dsh-native-gate-'));t.after(()=>rm(root,{recursive:true,force:true}));
  await mkdir(join(root,'native'),{recursive:true});await writeFile(join(root,'native/dsh_native_reader.node'),'fixture');
  const kernel=join(root,'kernel');await mkdir(join(kernel,'lib'),{recursive:true});
  const cli={name:'@deepseek-ai/dsh',version:'0.1.2-rc.1'};await writeFile(join(kernel,'package.json'),JSON.stringify(cli));
  for(const name of ['dsh-session','dsh-session-persistence','dsh-session-persistence-jsonl']){
    const dir=join(kernel,'node_modules/@deepseek-ai',name);await mkdir(dir,{recursive:true});await writeFile(join(dir,'package.json'),JSON.stringify({version:'0.1.2-rc.1'}));
  }
  const context={process:{platform:'win32',argv:['node',join(kernel,'lib/bin.js')],env:{},getBuiltinModule:process.getBuiltinModule}};
  const gate=nativeGate(root);assert.equal(runInNewContext(gate,context),true);
  const overlay=join(root,'native.yml');await writeFile(overlay,nativeOverlay(root));
  const boot=await moduleOf('dsh-app-boot');
  const config={root:'Z:/custom-storage',preparedSessionCacheSize:3,compression:'none',packChunks:false};
  const entries=boot.composeEntries([[{insert:[{id:'session-persistence-jsonl',name:'@deepseek-ai/dsh-session-persistence-jsonl',config}]}],boot.loadOverlayPatches('dsh',overlay)]);
  assert.deepEqual(entries[0].config,config);
  assert.equal(runInNewContext(entries[0].disabled.__jsExpr,context),true);
  cli.version='0.1.3-alpha.1';await writeFile(join(kernel,'package.json'),JSON.stringify(cli));
  assert.equal(runInNewContext(entries[0].disabled.__jsExpr,context),false);
  for(const entry of entries.slice(1))assert.equal(runInNewContext(entry.disabled.__jsExpr,context),true);
  cli.version='0.1.2-rc.1';await writeFile(join(kernel,'package.json'),JSON.stringify(cli));
  context.process.env.DSH_DESKTOP_NATIVE_DISABLED='1';assert.equal(runInNewContext(gate,context),false);
});
