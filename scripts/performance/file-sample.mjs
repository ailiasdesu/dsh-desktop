import {mkdir,open,readFile,writeFile} from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
import {performance} from 'node:perf_hooks';
import assert from 'node:assert/strict';
import {NativeClient} from '../../desktop/native/client.mjs';

const [mode,output]=process.argv.slice(2);
const root=resolve('target/performance-files');await mkdir(root,{recursive:true});
const file=join(root,'fixture.bin');
if(mode==='prepare'){
  const chunk=Buffer.alloc(256*1024);
  for(let i=0;i<chunk.length;i++)chunk[i]=(i*17+43)%256;
  const hash=createHash('sha256');const fd=await open(file,'w');
  try{for(let i=0;i<1024;i++){await fd.write(chunk);hash.update(chunk);}}finally{await fd.close();}
  await writeFile(join(root,'manifest.json'),JSON.stringify({bytes:256*1024*1024,sha256:hash.digest('hex')}));
  console.log('Prepared 256 MiB synthetic file');
}else{
  assert(['buffer','stream','native'].includes(mode));
  const expected=JSON.parse(await readFile(join(root,'manifest.json'),'utf8'));
  const client=new NativeClient({executable:resolve('native-helper/target/release/dsh-native-helper.exe'),cache:join(root,'native')});
  const ms=[];
  try{
    for(let i=0;i<3;i++){
      const started=performance.now();let actual;
      if(mode==='buffer')actual=createHash('sha256').update(await readFile(file)).digest('hex');
      else if(mode==='stream'){
        const hash=createHash('sha256');
        for await(const chunk of createReadStream(file,{highWaterMark:128*1024}))hash.update(chunk);
        actual=hash.digest('hex');
      }else actual=(await client.request({op:'hash_file',root,path:'fixture.bin'})).sha256;
      assert.equal(actual,expected.sha256);ms.push(performance.now()-started);
    }
  }finally{await client.close();}
  await writeFile(output,JSON.stringify({mode,bytes:expected.bytes,ms,memory:process.memoryUsage()},null,2));
}
