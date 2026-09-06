import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {Session as InspectorSession} from 'node:inspector';
import {writeFile} from 'node:fs/promises';
export const name='native-runtime-acceptance-driver';
export const inject=['webServer','agents','agentPresets','sessionPersistence','sessions'];
export function apply(ctx){
  if(!process.env.DSH_NATIVE_ACCEPTANCE_HOME||resolve(process.env.DSH_HOME)!==resolve(process.env.DSH_NATIVE_ACCEPTANCE_HOME))throw new Error('Acceptance home mismatch');
  const handles=new Map();let calls=0;
  let profiler;
  const profileCommand=method=>new Promise((resolve,reject)=>profiler.post(method,(error,result)=>error?reject(error):resolve(result)));
  ctx.effect(()=>ctx.webServer.register({kind:'exact',path:'/native-acceptance',handler:async(req,res)=>{
    if(req.headers['x-native-acceptance']!==process.env.DSH_NATIVE_ACCEPTANCE_TOKEN){res.writeHead(403);res.end();return;}
    try{
      let raw='';for await(const chunk of req){raw+=chunk;if(raw.length>1024*1024)throw new Error('Request too large');}
      const body=JSON.parse(raw||'{}');let value;
      if(body.action==='profile-start'){
        if(!process.env.DSH_NATIVE_PROFILE_RESULT)throw new Error('Profile output was not configured');
        profiler=new InspectorSession();profiler.connect();await profileCommand('Profiler.enable');await profileCommand('Profiler.start');value=true;
      }else if(body.action==='profile-stop'){
        const result=await profileCommand('Profiler.stop');profiler.disconnect();profiler=undefined;
        await writeFile(process.env.DSH_NATIVE_PROFILE_RESULT,JSON.stringify(result.profile));value={samples:result.profile.samples?.length};
      }else if(body.action==='status')value={native:!!ctx.sessionPersistence.nativeMetrics,metrics:ctx.sessionPersistence.nativeMetrics,config:ctx.sessionPersistence.config};
      else if(body.action==='inspect'){
        const source=await ctx.sessionPersistence.inspect('perf-huge-0');
        value={events:source.events.length,last:source.events.at(-1)?.type,originalPrefixLast:source.events[12287]?.type,
          metadataEventTypes:source.events.slice(12288).map(e=>e.type),
          native:!!ctx.sessionPersistence.nativeMetrics,metrics:ctx.sessionPersistence.nativeMetrics};
      }else if(body.action==='tool'){
        const id='session-native-tool-'+randomUUID();calls++;
        const h=await ctx.agents.create({sessionId:id,meta:{cwd:process.env.DSH_NATIVE_ACCEPTANCE_WORKSPACE,agentPreset:'standard'},agentOptions:{provider:'deepseek-official',model:'deepseek-v4-flash'},setup:actx=>ctx.agentPresets.mount(actx,'standard').then(()=>{})});
        handles.set(id,h);
        const session=h.agent.session;
        session.append('turn/start',{turn:1});
        session.append('user/message',{content:[{type:'text',text:'NATIVE_ACCEPTANCE_NEEDLE 中文'}],source:{kind:'user'}},{surfaceOp:'append'});
        session.append('turn/end',{turn:1,reason:{kind:'completed'}});
        value=await ctx.agents.withInitiator(h.agent,()=>h.agent.ctx.tools.execute({callId:'native-test-'+calls,name:'desktop_session_search',arguments:{query:'NATIVE_ACCEPTANCE_NEEDLE'},agent:h.agent,signal:AbortSignal.timeout(30000)}));
        await h.dispose();handles.delete(id);
      }else throw new Error('Unknown acceptance action');
      res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({ok:true,value}));
    }catch(error){res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({ok:false,error:String(error)}));}
  }}));
  ctx.effect(()=>async()=>{for(const h of handles.values())await h.dispose();});
  ctx.effect(()=>()=>profiler?.disconnect());
}
