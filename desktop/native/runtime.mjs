import {readFile} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {homedir} from 'node:os';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {NativeClient} from './client.mjs';

export const resourceRoot=fileURLToPath(new URL('../../',import.meta.url));
export function kernelRoot(){return dirname(dirname(resolve(process.argv[1])));}
const helpers=new Map();
export async function officialModules(){
  const root=kernelRoot();
  const packages={jsonl:'dsh-session-persistence-jsonl',session:'dsh-session',persistence:'dsh-session-persistence'};
  const versions={};
  for(const [key,name] of Object.entries(packages))versions[key]=JSON.parse(await readFile(join(root,'node_modules/@deepseek-ai',name,'package.json'),'utf8')).version;
  const moduleOf=name=>import(pathToFileURL(join(root,'node_modules/@deepseek-ai',name,'lib/index.js')));
  const [{JsonlSessionPersistence},sessionApi,query,loader,{Service}]=await Promise.all([
    moduleOf(packages.jsonl),moduleOf(packages.session),moduleOf('dsh-session-query'),moduleOf('cordis-plugin-loader'),moduleOf('cordis')]);
  return {root,versions,JsonlSessionPersistence,sessionApi,query,Service,interpolate:loader.interpolate};
}
export function acquireHelper(){
  const cache=join(process.env.DSH_HOME??join(homedir(),'.dsh'),'cache','desktop-native');
  let entry=helpers.get(cache);
  if(!entry){entry={client:new NativeClient({executable:join(resourceRoot,'native','dsh-native-helper.exe'),cache}),refs:0};helpers.set(cache,entry);}
  entry.refs++;let released=false;
  return {client:entry.client,async release(){
    if(released)return;released=true;
    if(--entry.refs===0){helpers.delete(cache);await entry.client.close();}
  }};
}
