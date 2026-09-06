import {mkdir,writeFile,rename,unlink,stat} from 'node:fs/promises';
import {createReadStream,createWriteStream} from 'node:fs';
import {pipeline} from 'node:stream/promises';
import {resolve,join,dirname} from 'node:path';
import {zstdCompressSync,constants} from 'node:zlib';
import assert from 'node:assert/strict';
import {NativeClient} from '../../desktop/native/client.mjs';
import {backend,install} from './official.mjs';

const sourceRoot=resolve('target/performance-huge/sessions');
const source=join(sourceRoot,'_no-cwd/perf-huge-0/session.jsonl.zstd');
const root=resolve('target/native-runtime/custom-sessions');
const workspace=join(install,'repair-checks/acceptance/workspaces/one');
const b=await backend(root);
const metadata=b.ctx.sessions.prepare('perf-huge-0',{meta:{cwd:workspace}}).header;
const destination=b.persistence.locate(metadata).path;
try{
  try{if(process.argv.includes('--refresh'))throw Object.assign(new Error('Refresh fixture'),{code:'ENOENT'});await stat(destination);console.log('Workspace history fixture already prepared');}
  catch(error){
    if(error.code!=='ENOENT')throw error;
    const client=new NativeClient({executable:resolve('native/dsh-native-helper.exe'),cache:resolve('target/native-runtime/prepare-cache')});
    const abort=new AbortController();const chunks=[];let offset;
    try{
      await client.request({op:'read_zstd',root:sourceRoot,path:'_no-cwd/perf-huge-0/session.jsonl.zstd',max_bytes:1024*1024},{signal:abort.signal,onProgress:progress=>{
        if(progress.frame!==0)return;
        if(progress.end){offset=progress.compressed_end;abort.abort(new Error('Header captured'));}
        else chunks.push(Buffer.from(progress.data));
      }});
    }catch(error){if(!abort.signal.aborted)throw error;}
    finally{await client.close();}
    assert(offset>0&&chunks.length);
    const header=JSON.parse(Buffer.concat(chunks).toString('utf8'));header.cwd=workspace;
    await mkdir(dirname(destination),{recursive:true});const staged=destination+'.staged';
    await writeFile(staged,zstdCompressSync(JSON.stringify(header)+'\n',{params:{[constants.ZSTD_c_checksumFlag]:1}}));
    await pipeline(createReadStream(source,{start:offset}),createWriteStream(staged,{flags:'a'}));
    await rename(staged,destination);
    console.log('Workspace history fixture prepared with unchanged event frames');
  }
  // Remove only the specifically named earlier test copy, not any user history.
  const old=join(root,'_no-cwd/perf-huge-0/session.jsonl.zstd');
  await unlink(old).catch(error=>{if(error.code!=='ENOENT')throw error;});
}finally{await b.close();}
