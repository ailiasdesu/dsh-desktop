import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {resolve,join} from 'node:path';
import {setTimeout} from 'node:timers/promises';
import {NativeClient} from '../../desktop/native/client.mjs';

const executable=resolve('native-helper/target/release/dsh-native-helper.exe');
async function fixture(t,options={}){
  const root=await mkdtemp(join(tmpdir(),'dsh-native-client-'));
  const client=new NativeClient({executable,cache:join(root,'cache'),...options});
  t.after(async()=>{await client.close();await rm(root,{recursive:true,force:true});});
  return {root,client};
}

test('real helper lazy start, Unicode files, bounded import and incremental replacement',async t=>{
  const {root,client}=await fixture(t);
  assert.equal(client.pid,undefined);
  assert.equal((await client.request({op:'hello'})).protocol,1);
  await writeFile(join(root,'中文.txt'),'hello');
  assert.equal((await client.request({op:'hash_file',root,path:'中文.txt'})).bytes,5);
  const documents=Array.from({length:1800},(_,seq)=>({seq,text:`note-${seq} `+'中文'.repeat(800)}));
  await client.importDocuments({session:'a',revision:'r1',documents});
  const result=await client.request({op:'search',query:'note-1799 ',session:'a',limit:5});
  assert.equal(result.hits[0].seq,1799);
  await client.importDocuments({session:'a',revision:'r2',baseRevision:'r1',fromSeq:1799,documents:[{seq:1800,text:'replacement'}]});
  assert.deepEqual((await client.request({op:'search',query:'note-1799 ',limit:5})).hits,[]);
  assert.equal((await client.request({op:'index_state',session:'a'})).documents,1800);
});

test('idle process exits and committed index is reusable after lazy restart',async t=>{
  const {client}=await fixture(t,{idleMs:50});
  await client.importDocuments({session:'a',revision:'r1',documents:[{seq:1,text:'durable'}]});
  const pid=client.pid;
  for(let i=0;i<20&&client.pid;i++)await setTimeout(20);
  assert.equal(client.pid,undefined);
  assert.equal((await client.request({op:'index_state',session:'a'})).revision,'r1');
  assert.notEqual(client.pid,pid);
});

test('invalid revision, oversized frame and aborted requests do not publish partial state',async t=>{
  const {client}=await fixture(t);
  await client.importDocuments({session:'a',revision:'r1',documents:[{seq:1,text:'old'}]});
  await assert.rejects(client.importDocuments({session:'a',revision:'r2',baseRevision:'wrong',fromSeq:2,documents:[]}),/REVISION_MISMATCH/);
  const controller=new AbortController();controller.abort(new Error('fixture cancellation'));
  await assert.rejects(client.request({op:'hello'},{signal:controller.signal}),/fixture cancellation/);
  await assert.rejects(client.request({op:'search',query:'x'.repeat(3*1024*1024),limit:1}),/REQUEST_TOO_LARGE/);
  assert.equal((await client.request({op:'index_state',session:'a'})).revision,'r1');
});

test('startup failure rejects promptly and close prevents restart',async t=>{
  const {client}=await fixture(t,{executable:resolve('target/does-not-exist.exe')});
  await assert.rejects(client.request({op:'hello'}),/ENOENT/);
  await client.close();
  await assert.rejects(client.request({op:'hello'}),/CLOSED/);
});

test('in-flight abort ends a native operation and leaves the client restartable',async t=>{
  const {client}=await fixture(t);
  await client.request({op:'hello'});
  const controller=new AbortController();
  const request=client.request({op:'hello'},{signal:controller.signal});
  controller.abort(new Error('cancel in flight'));
  await assert.rejects(request,/cancel in flight/);
  assert.equal((await client.request({op:'hello'})).protocol,1);
});
