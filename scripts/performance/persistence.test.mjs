import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm,writeFile,readFile,mkdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve,dirname} from 'node:path';
import {zstdCompressSync,constants} from 'node:zlib';
import {backend,moduleOf} from './official.mjs';
import {NativeClient} from '../../desktop/native/client.mjs';
import {nativePersistenceClass} from '../../desktop/native/persistence.mjs';
import {AddonReadTransport} from '../../desktop/native/addon.mjs';

const zstd=text=>zstdCompressSync(text,{params:{[constants.ZSTD_c_checksumFlag]:1}});
const versions={jsonl:'0.1.2-rc.1',session:'0.1.2-rc.1',persistence:'0.1.2-rc.1'};
async function fixture(t,options={}){
  const root=await mkdtemp(join(tmpdir(),'dsh-native-persist-'));
  const b=await backend(join(root,'sessions'));
  const [{Context},sessionApi,{JsonlSessionPersistence}]=await Promise.all([moduleOf('cordis'),moduleOf('dsh-session'),moduleOf('dsh-session-persistence-jsonl')]);
  const client=process.env.DSH_PERF_TRANSPORT==='addon'&&!options.executable?
    new AddonReadTransport(resolve('native-addon/target/release/dsh_native_reader.node')):
    new NativeClient({executable:options.executable??resolve('native-helper/target/release/dsh-native-helper.exe'),cache:join(root,'cache')});
  const Native=nativePersistenceClass({JsonlSessionPersistence,sessionApi,client,versions:options.versions??versions,minCompressedBytes:0});
  const ctx=new Context();await ctx.plugin(sessionApi.SessionStore);await ctx.plugin(Native,{root:join(root,'sessions')});
  const session=sessionApi.Session.create('a');
  session.append('turn/start',{turn:1});
  session.append('user/message',{content:[{type:'text',text:'original 中文'}],source:{kind:'user'}},{surfaceOp:'append'});
  session.append('turn/end',{turn:1,reason:{kind:'completed'}});
  await b.persistence.create(session.header);await b.persistence.append('a',session.snapshotEvents());
  const path=b.persistence.locate(session.header).path;
  const artifact=await b.persistence.readRaw('a');
  const header=JSON.parse(artifact.content.split('\n')[0]);
  const rows=session.snapshotEvents().map(e=>structuredClone(e));
  async function rewrite(nextHeader=header,nextRows=rows){
    await writeFile(path,Buffer.concat([zstd(JSON.stringify(nextHeader)+'\n'),zstd(nextRows.map(r=>typeof r==='string'?r:JSON.stringify(r)).join('\n')+'\n')]));
  }
  t.after(async()=>{await ctx.fiber.dispose();await b.close();await rm(root,{recursive:true,force:true});});
  return {root,b,ctx,client,native:ctx.sessionPersistence,path,header,rows,rewrite,sessionApi};
}
async function outcome(promise){
  try{return {ok:true,value:await promise};}
  catch(error){return {ok:false,name:error.name,message:error.message,location:error.location};}
}

test('public loadStored and borrowSession use native reads with independent graphs',async t=>{
  const f=await fixture(t);
  const official=await f.b.persistence.loadStored('a');
  const native=await f.native.loadStored('a');assert.deepEqual(native,official);
  const next=await f.native.loadStored('a');assert.notEqual(next.events,native.events);assert.notEqual(next.events[1].data,native.events[1].data);
  const lease=await f.native.borrowSession('a');assert.equal(lease.inspection.events.length,3);lease[Symbol.dispose]();
  assert(f.native.nativeMetrics.hits>=3);
  assert.equal((await f.native.inspect('a')).events[1].data.content[0].text,'original 中文');
});

test('multi-frame Unicode lines and packed rows use official codecs losslessly',async t=>{
  const f=await fixture(t);
  const packed=[
    {type:'text-chunks',seq0:0,time0:1000,data:{turn:1,step:1,index:0,texts:['中','文','hello'],dt:[1,-1]}},
    {type:'reasoning-chunks',seq0:3,time0:1003,data:{turn:1,step:1,index:1,texts:['a','b'],dt:[1]}},
    {type:'tool-call-chunks',seq0:5,time0:1005,data:{turn:1,step:1,index:2,id:'call-x',name:'read',args:['{','}'],dt:[1]}},
    {type:'user/message',seq:7,time:1010,data:{content:[{type:'text',text:'中文'.repeat(100000)}],source:{kind:'user'}},surfaceOp:'append',sourceEventSeqs:[[0,6]]},
  ];
  const body=Buffer.from(packed.map(r=>JSON.stringify(r)).join('\n')+'\n');
  await writeFile(f.path,Buffer.concat([zstd(JSON.stringify(f.header)+'\n'),zstd(body.subarray(0,12345)),zstd(body.subarray(12345))]));
  assert.deepEqual(await f.native.loadStored('a'),await f.b.persistence.loadStored('a'));
  assert.equal(f.native.nativeMetrics.hits,1);
});

test('header-only, seeded and unknown headers preserve official outcomes',async t=>{
  const f=await fixture(t);
  await writeFile(f.path,zstd(JSON.stringify(f.header)+'\n'));
  assert.deepEqual(await f.native.loadStored('a'),await f.b.persistence.loadStored('a'));
  for(const extra of [{seedLength:0},{seedLength:2},{seedLength:999},{version:99},{sandboxMode:'workspace-write'}]){
    await f.rewrite({...f.header,...extra});
    assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
  }
});

test('malformed rows, seq gaps, incomplete frames and checksums fall back without repair writes',async t=>{
  const f=await fixture(t);
  for(const rows of [[...f.rows,'{broken'],[f.rows[0],'{broken',f.rows[2]],[f.rows[0],{...f.rows[1],seq:8},f.rows[2]]]){
    await f.rewrite(f.header,rows);const before=await readFile(f.path);
    assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
    assert.deepEqual(await readFile(f.path),before);
  }
  await f.rewrite();const full=await readFile(f.path);
  for(const cut of [1,4,9,Math.floor(full.length/3)]){
    await writeFile(f.path,full.subarray(0,full.length-cut));
    assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
  }
  const corrupt=Buffer.from(full);corrupt[corrupt.length-1]^=255;await writeFile(f.path,corrupt);
  assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
  assert(f.native.nativeMetrics.fallbacks>0);
});

test('first frame with body, missing newline and duplicate IDs use official validation',async t=>{
  const f=await fixture(t);
  await writeFile(f.path,zstd([JSON.stringify(f.header),...f.rows.map(r=>JSON.stringify(r))].join('\n')+'\n'));
  assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
  await writeFile(f.path,Buffer.concat([zstd(JSON.stringify(f.header)+'\n'),zstd(JSON.stringify(f.rows[0]))]));
  assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
  await f.rewrite();
  const duplicate=join(f.root,'sessions','other-project','a','session.jsonl.zstd');
  await mkdir(dirname(duplicate),{recursive:true});await writeFile(duplicate,await readFile(f.path));
  assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
});

test('missing helper, unknown kernel and caller cancellation preserve the official boundary',async t=>{
  const f=await fixture(t,{executable:resolve('target/not-present.exe')});
  assert.deepEqual(await f.native.loadStored('a'),await f.b.persistence.loadStored('a'));
  assert.equal(f.native.nativeMetrics.fallbacks,1);
  const signal=AbortSignal.abort(new Error('fixture abort'));
  await assert.rejects(f.native.loadStored('a',signal),/fixture abort/);
  const next=await fixture(t,{versions:{...versions,session:'0.1.3-alpha.1'}});
  assert.deepEqual(await next.native.loadStored('a'),await next.b.persistence.loadStored('a'));
  assert.equal(next.native.nativeMetrics.attempts,0);assert.equal(next.client.pid,undefined);
});

test('all skippable frame magics and the unused descriptor bit preserve official refusal',async t=>{
  const f=await fixture(t);const original=await readFile(f.path);
  for(let magic=0x50;magic<=0x5f;magic++){
    await writeFile(f.path,Buffer.concat([original,Buffer.from([magic,0x2a,0x4d,0x18,0,0,0,0])]));
    const before=await readFile(f.path);const hits=f.native.nativeMetrics.hits;
    assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
    assert.equal(f.native.nativeMetrics.hits,hits);assert.deepEqual(await readFile(f.path),before);
  }
  const body=zstd(f.rows.map(r=>JSON.stringify(r)).join('\n')+'\n');body[4]|=0x10;
  await writeFile(f.path,Buffer.concat([zstd(JSON.stringify(f.header)+'\n'),body]));
  assert.deepEqual(await outcome(f.native.loadStored('a')),await outcome(f.b.persistence.loadStored('a')));
});

test('replacement and append during native transfer never publish a stale revision',async t=>{
  for(const change of ['replace','append']){
    const f=await fixture(t);const request=f.client.request.bind(f.client);let changed=false;
    f.client.request=(operation,options)=>request(operation,{...options,onProgress:async progress=>{
      await options.onProgress(progress);
      if(!changed&&progress.data){
        changed=true;
        const rows=structuredClone(f.rows);
        if(change==='replace'){rows[1].data.content[0].text='replacement 中文';await f.rewrite(f.header,rows);}
        else{
          const extra={type:'turn/start',seq:3,time:12345,data:{turn:2}};
          const file=await readFile(f.path);await writeFile(f.path,Buffer.concat([file,zstd(JSON.stringify(extra)+'\n')]));
        }
      }
    }});
    assert.deepEqual(await f.native.loadStored('a'),await f.b.persistence.loadStored('a'));
    assert.equal(f.native.nativeMetrics.hits,0);assert.equal(f.native.nativeMetrics.fallbacks,1);
  }
});
