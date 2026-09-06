import {join} from 'node:path';
import {AddonReadTransport} from './addon.mjs';
import {nativePersistenceClass} from './persistence.mjs';
import {officialModules,resourceRoot} from './runtime.mjs';

export const name='desktop-native-persistence';
export const inject=['loader','sessions'];
export async function apply(ctx){
  // The managed patch disables this exact original entry only on a verified
  // kernel with a present addon. Reuse its evaluated config, including custom
  // roots, compression, packing and cache policies; never reconstruct defaults.
  const source=[...ctx.loader.entries()].find(e=>e.options.id==='session-persistence-jsonl'
    &&e.options.name==='@deepseek-ai/dsh-session-persistence-jsonl');
  if(!source?.disabled)return;
  const modules=await officialModules();
  const config=modules.interpolate(source.context,source.options.config??{});
  const client=new AddonReadTransport(join(resourceRoot,'native','dsh_native_reader.node'));
  const Native=nativePersistenceClass({...modules,client});
  await ctx.plugin(Native,config);
}
