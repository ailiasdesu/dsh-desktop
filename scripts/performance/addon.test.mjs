import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {resolve,join} from 'node:path';
import {zstdCompressSync} from 'node:zlib';
import {setTimeout as delay} from 'node:timers/promises';
import {AddonReadTransport} from '../../desktop/native/addon.mjs';
const path=resolve('native-addon/target/release/dsh_native_reader.node');
const {NativeReader}=createRequire(import.meta.url)(path);
async function fixture(t,body=Buffer.from('header\n')){
  const root=await mkdtemp(join(tmpdir(),'dsh-addon-test-'));
  await writeFile(join(root,'log.zstd'),zstdCompressSync(body));
  const transport=new AddonReadTransport(path);
  t.after(async()=>{await transport.close();await rm(root,{recursive:true,force:true});});
  return {root,transport,operation:{op:'read_zstd',root,path:'log.zstd',max_bytes:64*1024*1024}};
}
test('queued next is exclusive; close is idempotent and interrupts work',async t=>{
  const {root}=await fixture(t,Buffer.alloc(12*1024*1024,120));
  const reader=new NativeReader(root,'log.zstd',64*1024*1024);
  const pending=reader.next();assert.throws(()=>reader.next(),/BUSY/);
  await pending;
  const next=reader.next();reader.close();reader.close();
  await assert.rejects(next,/ABORTED/);assert.throws(()=>reader.next(),/CLOSED/);
});
test('external buffers retain their bytes through subsequent reads and GC',async t=>{
  const {root}=await fixture(t,Buffer.alloc(20*1024*1024,113));
  const reader=new NativeReader(root,'log.zstd',64*1024*1024);t.after(()=>reader.close());
  const first=await reader.next();const expected=first.parts.map(p=>Buffer.from(p.data));
  for(let i=0;i<3;i++){await reader.next();global.gc?.();}
  first.parts.forEach((part,i)=>assert.deepEqual(part.data,expected[i]));
});
for(const asyncAbort of [false,true])test(`final frame callback cancellation rejects (${asyncAbort?'async':'sync'})`,async t=>{
  const {transport,operation}=await fixture(t);const c=new AbortController();
  await assert.rejects(transport.request(operation,{signal:c.signal,onProgress:progress=>{
    if(!progress.end)return;
    if(!asyncAbort){c.abort(new Error('end abort'));return;}
    return delay(5).then(()=>c.abort(new Error('end abort')));
  }}),/end abort/);
});
test('timeout during final callback rejects and transport closes cleanly',async t=>{
  const {transport,operation}=await fixture(t);
  await assert.rejects(transport.request(operation,{timeoutMs:20,onProgress:p=>p.end?delay(50):undefined}),/TIMEOUT/);
});
