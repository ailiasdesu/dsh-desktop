import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {NativeClient} from '../../desktop/native/client.mjs';
import {SessionTextIndex} from '../../desktop/native/session-text.mjs';
import {backend,moduleOf} from './official.mjs';

async function fixture(t){
  const root=await mkdtemp(join(tmpdir(),'dsh-session-text-'));
  const b=await backend(join(root,'sessions'));
  const {SqliteSessionQueryEngine}=await moduleOf('dsh-session-query-sqlite');
  const {extractSessionEventText}=await moduleOf('dsh-session-query');
  const {Session}=await moduleOf('dsh-session');
  await b.ctx.plugin(SqliteSessionQueryEngine,{path:join(root,'unused.sqlite'),openAt:'never'});
  const client=new NativeClient({executable:resolve('native-helper/target/release/dsh-native-helper.exe'),cache:join(root,'native')});
  const options={client,sessions:b.ctx.sessions,persistence:b.persistence,
    observe:(...args)=>b.ctx.sessionQuery.observeSession(...args),extractText:extractSessionEventText,kernelVersion:'0.1.2-rc.1'};
  const index=new SessionTextIndex(options);
  t.after(async()=>{await client.close();await b.close();await rm(root,{recursive:true,force:true});});
  return {b,Session,index,options,client};
}
function turn(session,text){
  const n=session.seq/3+1;
  session.append('turn/start',{turn:n});
  session.append('user/message',{content:[{type:'text',text}],source:{kind:'user'}},{surfaceOp:'append'});
  session.append('turn/end',{turn:n,reason:{kind:'completed'}});
}
test('official persisted events, Unicode search, cache revision and helper failure fallback',async t=>{
  const {b,Session,index,client}=await fixture(t);
  const session=Session.create('a');turn(session,'Apple 中文');
  await b.persistence.create(session.header);await b.persistence.append('a',session.snapshotEvents());
  const first=await index.search('a','apple');assert.equal(first.engine,'rust');assert.equal(first.hits[0].seq,1);
  assert.deepEqual((await index.search('a','中文')).hits,first.hits);
  turn(session,'pear 中文');await b.persistence.append('a',session.snapshotEvents(3));
  assert.equal((await index.search('a','中文')).hits.length,2);
  await client.close();
  const fallback=await index.search('a','中文');assert.equal(fallback.engine,'official-fallback');assert.equal(fallback.hits.length,2);
});
test('live appends are incremental and unknown kernel bypasses the helper',async t=>{
  const {b,index,options,client}=await fixture(t);
  const session=b.ctx.sessions.create('live');turn(session,'first');
  assert.equal((await index.search('live','first')).engine,'rust');
  turn(session,'second');
  assert.equal((await index.search('live','second')).hits[0].seq,4);
  assert.equal((await client.request({op:'index_state',session:'live'})).documents,2);
  const next=new SessionTextIndex({...options,kernelVersion:'0.1.3-alpha.1'});
  assert.equal((await next.search('live','first')).engine,'official-fallback');
});

for(const change of ['evict','replace'])test(`concurrent cache ${change} with no hits falls back to authoritative history`,async t=>{
  const {b,index,client}=await fixture(t);const session=b.ctx.sessions.create('race');turn(session,'find-me');
  assert.equal((await index.search('race','find-me')).hits.length,1);
  const competitor=new NativeClient({executable:client.executable,cache:client.cache});
  try {
  const original=client.request.bind(client);let changed=false;
  client.request=async(op,opts)=>{
    if(op.op==='search'&&!changed){
      changed=true;
      if(change==='evict')await competitor.request({op:'index_delete',session:'race'});
      else await competitor.importDocuments({session:'race',revision:'foreign',documents:[{seq:1,text:'unrelated'}]});
    }
    return original(op,opts);
  };
  const result=await index.search('race','find-me');assert.equal(result.hits.length,1);assert.equal(result.engine,'official-fallback');
  }finally{await competitor.close();}
});
