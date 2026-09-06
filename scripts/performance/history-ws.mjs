import {createRequire} from 'node:module';
import {join} from 'node:path';
import {install} from './official.mjs';
const WebSocket=createRequire(join(install,'kernel/package.json'))('ws');
const origin=process.env.DSH_NATIVE_WS_ORIGIN;
const cookie=process.env.DSH_NATIVE_WS_COOKIE;
const result=await new Promise((resolve,reject)=>{
  const ws=new WebSocket(origin.replace(/^http/,'ws')+'/api/remote.mux',{headers:{cookie},maxPayload:256*1024*1024});
  const timer=setTimeout(()=>{ws.terminate();reject(new Error('History snapshot timeout'));},60000);
  let settled=false;
  function finish(error,value){
    if(settled)return;settled=true;clearTimeout(timer);ws.close();
    if(error)reject(error);else resolve(value);
  }
  ws.on('open',()=>ws.send(JSON.stringify({type:'open',streamId:'history',endpoint:'session/follow',payload:{args:{request:{address:{kind:'session',sessionId:'perf-huge-0'},maxMessages:1}}}})));
  ws.on('error',error=>finish(error));
  ws.on('close',()=>{if(!settled)finish(new Error('History stream closed before snapshot'));});
  ws.on('message',data=>{
    const message=JSON.parse(data);const value=message.value;
    if(message.type==='error'||message.type==='failure')return finish(new Error('History RPC failed: '+String(message.error?.message??message.code??'unknown')));
    if(message.streamId==='history'&&value?.type==='snapshot')finish(undefined,{headerId:value.header?.id,cursor:value.cursor,records:value.records?.length,payloadBytes:data.length});
  });
});
if(result.headerId!=='perf-huge-0'||!result.records)throw new Error('Wrong history snapshot');
console.log(JSON.stringify(result));
