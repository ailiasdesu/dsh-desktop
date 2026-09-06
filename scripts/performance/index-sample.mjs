import {readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {performance} from 'node:perf_hooks';
import assert from 'node:assert/strict';
import {backend,moduleOf} from './official.mjs';
import {NativeClient} from '../../desktop/native/client.mjs';
import {SessionTextIndex} from '../../desktop/native/session-text.mjs';

const [mode,output]=process.argv.slice(2);
assert(['prepare','native','official'].includes(mode));
const root=resolve('target/performance-fixtures');
const manifest=JSON.parse(await readFile(join(root,'manifest.json'),'utf8'));
const {SqliteSessionQueryEngine}=await moduleOf('dsh-session-query-sqlite');
const {extractSessionEventText}=await moduleOf('dsh-session-query');
const b=await backend(join(root,'sessions'));
await b.ctx.plugin(SqliteSessionQueryEngine,{path:join(root,'unused-query.sqlite'),openAt:'never'});
const client=new NativeClient({executable:resolve('native-helper/target/release/dsh-native-helper.exe'),
  cache:join(root,'native-text'),timeoutMs:60000});
const index=new SessionTextIndex({client,sessions:b.ctx.sessions,persistence:b.persistence,
  observe:(...args)=>b.ctx.sessionQuery.observeSession(...args),extractText:extractSessionEventText,
  kernelVersion:mode==='official'?'force-official-fallback':'0.1.2-rc.1'});
const result={mode,diskCacheControlled:false,coldProcess:true,queries:[]};
try{
  const entries=manifest.sessions.filter(s=>s.label==='large');
  for(let round=0;round<(mode==='prepare'?1:2);round++){
    for(const entry of entries){
      const query=`fixture ${entry.id} turn 1023 `;
      const started=performance.now();
      const hits=await index.search(entry.id,query);
      assert.equal(hits.hits.length,1);assert.equal(hits.hits[0].seq,3070);
      assert.equal(hits.engine,mode==='official'?'official-fallback':'rust');
      result.queries.push({session:entry.id,round,ms:performance.now()-started});
    }
  }
  result.memory=process.memoryUsage();
}finally{await client.close();await b.close();}
await writeFile(output,JSON.stringify(result,null,2));
console.log(`${mode}: ${result.queries.length} exact-result queries`);
