import {SessionTextIndex} from './session-text.mjs';
import {officialModules,acquireHelper} from './runtime.mjs';

export const name='desktop-native-tools';
export const inject=['tools','sessions','sessionPersistence','sessionQuery'];
export async function apply(ctx){
  const modules=await officialModules();
  const helper=acquireHelper();ctx.effect(()=>()=>helper.release(),'desktop native tool helper');
  const index=new SessionTextIndex({client:helper.client,sessions:ctx.sessions,persistence:ctx.sessionPersistence,
    observe:(...args)=>ctx.sessionQuery.observeSession(...args),extractText:modules.query.extractSessionEventText,kernelVersion:modules.versions.session});
  ctx.effect(()=>ctx.tools.register({
    name:'desktop_session_search',
    description:'在当前 DSH 会话的历史事件文本中查找关键词，大小写不敏感的字面匹配；返回事件序号和有界预览。包括官方可搜索的消息/工具文本，不调用模型，也不修改原始会话。',
    parameters:{type:'object',properties:{query:{type:'string'},limit:{type:'integer',minimum:1,maximum:100}},required:['query'],additionalProperties:false},
    output:{schema:{type:'string'},render:(_args,value)=>[{type:'text',text:value}]},
    async execute(args,exec){
      if(!exec.agent?.session)throw new Error('A current calling session is required');
      return JSON.stringify(await index.search(exec.agent.session.id,args.query,{limit:args.limit??20,signal:exec.signal}));
    },
  }),'desktop native session search');
}
