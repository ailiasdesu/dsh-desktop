import {readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {performance,monitorEventLoopDelay} from 'node:perf_hooks';
import assert from 'node:assert/strict';
import {moduleOf} from './official.mjs';
import {NativeClient} from '../../desktop/native/client.mjs';
import {nativePersistenceClass} from '../../desktop/native/persistence.mjs';
import {AddonReadTransport} from '../../desktop/native/addon.mjs';

const [mode,output]=process.argv.slice(2);assert(['official','native','addon'].includes(mode));
const root=resolve(process.env.DSH_PERF_FIXTURES??'target/performance-entropy');
const manifest=JSON.parse(await readFile(join(root,'manifest.json'),'utf8'));
const [{Context,Service},sessionApi,{JsonlSessionPersistence}]=await Promise.all([moduleOf('cordis'),moduleOf('dsh-session'),moduleOf('dsh-session-persistence-jsonl')]);
const client=mode==='addon'?new AddonReadTransport(resolve('native-addon/target/release/dsh_native_reader.node')):
  new NativeClient({executable:resolve('native-helper/target/release/dsh-native-helper.exe'),cache:join(root,'native'),timeoutMs:120000});
const minimum=Number(process.env.DSH_PERF_MIN_COMPRESSED_BYTES??0);
const Native=nativePersistenceClass({JsonlSessionPersistence,sessionApi,client,Service,
  versions:{session:'0.1.2-rc.1',jsonl:'0.1.2-rc.1',persistence:'0.1.2-rc.1'},minCompressedBytes:minimum,measure:true});
const ctx=new Context();await ctx.plugin(sessionApi.SessionStore);
await ctx.plugin(mode!=='official'?Native:JsonlSessionPersistence,{root:join(root,'sessions')});
const lag=monitorEventLoopDelay({resolution:10});lag.enable();
const result={mode,processCold:true,diskCacheControlled:false,reads:[]};
try{
  for(const entry of manifest.sessions){
    const started=performance.now();
    const source=await ctx.sessionPersistence.inspect(entry.id);
    assert.equal(source.events.length,entry.events);
    assert.equal(source.events.at(-1).type,'turn/end');
    result.reads.push({id:entry.id,label:entry.label,ms:performance.now()-started,compressedBytes:entry.compressedBytes});
  }
  result.memory=process.memoryUsage();
  result.metrics=ctx.sessionPersistence.nativeMetrics;
  if(mode!=='official')assert.equal(result.metrics.hits,manifest.sessions.filter(s=>s.compressedBytes>=minimum).length);
}finally{
  result.eventLoopDelay={p95Ms:lag.percentile(95)/1e6,maxMs:lag.max/1e6};lag.disable();
  await ctx.fiber.dispose();await client.close();
}
await writeFile(output,JSON.stringify(result,null,2));
console.log(JSON.stringify(result));
