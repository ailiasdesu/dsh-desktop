import {lookup} from 'node:dns/promises';
import {isIP} from 'node:net';
import ipaddr from '../kernel/node_modules/ipaddr.js/lib/ipaddr.js';
import {Config,HttpFetchProvider,LOCAL_FETCH_PROVIDER_ID} from '../kernel/node_modules/@deepseek-ai/dsh-web-fetch-http/lib/index.js';
import {WebError} from '../kernel/node_modules/@deepseek-ai/dsh-web/lib/index.js';
export {Config};
export const name='web-fetch-http';
export const inject=['web'];
export function isFakeAddress(address) {
  if(isIP(address)===4){const n=address.split('.').map(Number);return n[0]===198&&(n[1]===18||n[1]===19);}
  return address.toLowerCase().startsWith('fdfe:dcba:9876:');
}
export function publicV4Answers(payload) {
  if(payload?.Status!==0)throw new WebError('Public DNS lookup failed','WEB_PROVIDER_ERROR');
  const answers=(payload.Answer??[]).filter(a=>a.type===1);
  if(!answers.length)throw new WebError('Public DNS returned no IPv4 addresses','WEB_PROVIDER_ERROR');
  return answers.map(a=>{
    if(isIP(a.data)!==4||ipaddr.parse(a.data).range()!=='unicast')throw new WebError('DNS returned a non-public destination','WEB_BLOCKED_URL');
    return {address:a.data,family:4};
  });
}
async function publicDns(hostname,signal) {
  if(isIP(hostname)||!hostname.includes('.')||/\.(localhost|local|internal|lan)$/i.test(hostname))throw new WebError('Not a public DNS hostname','WEB_BLOCKED_URL');
  const url=new URL('https://cloudflare-dns.com/dns-query');
  url.searchParams.set('name',hostname);url.searchParams.set('type','A');
  const response=await fetch(url,{headers:{accept:'application/dns-json'},signal:AbortSignal.any([signal,AbortSignal.timeout(10000)])});
  if(!response.ok)throw new WebError('Public DNS service unavailable','WEB_PROVIDER_ERROR');
  return publicV4Answers(await response.json());
}
export class ProxyDnsFetchProvider {
  id=LOCAL_FETCH_PROVIDER_ID;
  constructor(config,resolveSystem=hostname=>lookup(hostname,{all:true}),resolvePublic=publicDns){
    this.original=new HttpFetchProvider(config);
    this.fallback=new HttpFetchProvider(config,resolvePublic);
    this.resolveSystem=resolveSystem;
  }
  available(){return true;}
  async fetch(request,signal=AbortSignal.timeout(30000)){
    try{return await this.original.fetch(request,signal);}
    catch(error){
      if(error?.code!=='WEB_BLOCKED_URL')throw error;
      const hostname=new URL(request.url).hostname;
      if(isIP(hostname.replace(/^\[|\]$/g,'')))throw error;
      const answers=await this.resolveSystem(hostname);
      // Only a complete, recognized fake-IP answer set permits a second resolution.
      if(!answers.length||!answers.every(a=>isFakeAddress(a.address)))throw error;
      // The official transport still pins the validated address, checks TLS,
      // restricts redirects, and enforces time/body limits. No private range is allowed.
      return this.fallback.fetch(request,signal);
    }
  }
}
export function apply(ctx,config){ctx.web.registerFetchProvider(new ProxyDnsFetchProvider(config));}
