import {spawn} from 'node:child_process';
import {textPreview} from './text.mjs';

const MAX_FRAME=2*1024*1024;
const MAX_RESPONSE=2*1024*1024;

function splitDocument(document){
  if(Buffer.byteLength(JSON.stringify(document))<=512*1024)return [document];
  // Normalize before splitting so Unicode contextual lowercase is identical
  // to a whole-text query. Overlap exceeds the public query's 1024 code units.
  const text=document.text.toLowerCase();
  const preview=textPreview(document.text);
  const parts=[];let start=0,part=0;
  while(start<text.length){
    let end=Math.min(start+64*1024,text.length);
    if(end<text.length&&text.charCodeAt(end-1)>=0xd800&&text.charCodeAt(end-1)<=0xdbff)end--;
    parts.push({seq:document.seq,part:part++,text:text.slice(start,end),preview});
    if(end===text.length)break;
    start=end-2048;
    if(text.charCodeAt(start)>=0xdc00&&text.charCodeAt(start)<=0xdfff)start--;
  }
  return parts;
}

/** One lazy desktop-owned process; no user text is interpreted as a command. */
export class NativeClient {
  #child; #pending=new Map(); #next=0; #buffer=Buffer.alloc(0); #binary; #idle; #closed=false;
  #imports=Promise.resolve();
  #retiring=new Set();
  constructor({executable,cache,idleMs=30000,timeoutMs=15000}) {
    this.executable=executable;this.cache=cache;this.idleMs=idleMs;this.timeoutMs=timeoutMs;
  }
  get pid(){return this.#child?.pid;}
  #start(){
    if(this.#closed)throw new Error('NATIVE_CLIENT_CLOSED');
    clearTimeout(this.#idle);
    if(this.#child)return;
    const child=spawn(this.executable,['--cache',this.cache],{windowsHide:true,stdio:['pipe','pipe','ignore']});
    this.#child=child;this.#buffer=Buffer.alloc(0);
    child.on('error',error=>this.#stop(error,child));
    child.on('exit',(code,signal)=>this.#stop(new Error(`NATIVE_EXIT: ${code ?? signal}`),child));
    child.stdin.on('error',error=>this.#stop(error,child));
    child.stdout.on('data',chunk=>{
      if(this.#child!==child)return;
      if(this.#buffer.length+chunk.length>MAX_RESPONSE){this.#stop(new Error('NATIVE_RESPONSE_TOO_LARGE'),child);return;}
      this.#buffer=this.#buffer.length?Buffer.concat([this.#buffer,chunk]):chunk;
      child.stdout.pause();
      this.#consume(child).catch(error=>this.#stop(error,child));
    });
  }
  async #consume(child){
      let end;
      while(this.#child===child){
        if(this.#binary){
          if(!this.#buffer.length)break;
          const {id,frame}=this.#binary;
          const length=Math.min(this.#buffer.length,this.#binary.length);
          const bytes=this.#buffer.subarray(0,length);this.#buffer=this.#buffer.subarray(length);
          this.#binary.length-=length;if(!this.#binary.length)this.#binary=undefined;
          const pending=this.#pending.get(id);
          if(!pending?.onProgress)throw new Error('NATIVE_UNEXPECTED_BINARY');
          const accepted=pending.onProgress({frame,data:bytes});
          if(accepted?.then)await accepted;
          continue;
        }
        end=this.#buffer.indexOf(10);if(end<0)break;
        const line=this.#buffer.subarray(0,end);this.#buffer=this.#buffer.subarray(end+1);
        let response;
        try{response=JSON.parse(line.toString('utf8'));}catch{this.#stop(new Error('NATIVE_INVALID_RESPONSE'),child);return;}
        const pending=this.#pending.get(response.id);
        if(!pending || typeof response.ok!=='boolean'){this.#stop(new Error('NATIVE_RESPONSE_ID_MISMATCH'),child);return;}
        if(response.binary!==undefined){
          if(!response.ok||!pending.onProgress||!Number.isSafeInteger(response.binary)||response.binary<1||response.binary>128*1024)throw new Error('NATIVE_INVALID_BINARY');
          this.#binary={id:response.id,frame:response.frame,length:response.binary};continue;
        }
        if(response.progress!==undefined){
          if(!response.ok||!pending.onProgress)throw new Error('NATIVE_UNEXPECTED_PROGRESS');
          const accepted=pending.onProgress(response.progress);
          if(accepted?.then)await accepted;
          continue;
        }
        this.#pending.delete(response.id);clearTimeout(pending.timer);pending.cleanup();
        if(response.ok)pending.resolve(response.value);else pending.reject(new Error(response.error));
      }
      this.#scheduleIdle();
      if(this.#child===child)child.stdout.resume();
  }
  #scheduleIdle(){
    clearTimeout(this.#idle);
    if(this.#pending.size || !this.#child)return;
    this.#idle=setTimeout(()=>this.#retire(),this.idleMs);
    this.#idle.unref();
  }
  #stop(error,child=this.#child){
    if(child!==this.#child)return;
    clearTimeout(this.#idle);this.#child=undefined;this.#buffer=Buffer.alloc(0);this.#binary=undefined;
    child?.kill();
    for(const pending of this.#pending.values()){
      clearTimeout(pending.timer);pending.cleanup();pending.reject(error);
    }
    this.#pending.clear();
  }
  #retire(){
    const child=this.#child;
    if(!child||this.#pending.size)return Promise.resolve();
    clearTimeout(this.#idle);this.#child=undefined;this.#buffer=Buffer.alloc(0);this.#binary=undefined;
    // EOF lets SQLite checkpoint/close. Killing an idle writer forces costly
    // WAL recovery at the next start and defeats cold-search latency.
    const stopped=new Promise(resolve=>{
      const timer=setTimeout(()=>child.kill(),5000);timer.unref();
      const done=()=>{clearTimeout(timer);resolve();};
      child.once('exit',done);child.once('error',done);child.stdin.end();
    });
    this.#retiring.add(stopped);
    stopped.finally(()=>this.#retiring.delete(stopped));
    return stopped;
  }
  request(operation,{signal,onProgress,timeoutMs=this.timeoutMs}={}){
    if(signal?.aborted)return Promise.reject(signal.reason ?? new Error('NATIVE_ABORTED'));
    if(this.#pending.size>=8)return Promise.reject(new Error('NATIVE_QUEUE_FULL'));
    const id=++this.#next;
    const data=JSON.stringify({...operation,id,version:1})+'\n';
    if(Buffer.byteLength(data)>MAX_FRAME)return Promise.reject(new Error('NATIVE_REQUEST_TOO_LARGE'));
    try{this.#start();}catch(error){return Promise.reject(error);}
    const child=this.#child;
    return new Promise((resolve,reject)=>{
      const abort=()=>this.#stop(signal.reason ?? new Error('NATIVE_ABORTED'),child);
      const timer=setTimeout(()=>this.#stop(new Error('NATIVE_TIMEOUT'),child),timeoutMs);
      this.#pending.set(id,{resolve,reject,timer,onProgress,cleanup:()=>signal?.removeEventListener('abort',abort)});
      signal?.addEventListener('abort',abort,{once:true});
      child.stdin.write(data,error=>{if(error)this.#stop(error,child);});
    });
  }
  /** Serial transaction, bounded batches; old committed revision survives any failure. */
  importDocuments({session,revision,documents,baseRevision,fromSeq},{signal}={}){
    const run=async()=>{
      signal?.throwIfAborted();
      const parts=documents.flatMap(splitDocument);
      await this.request({op:'index_begin',session,revision,expected_documents:parts.length,
        base_revision:baseRevision,from_seq:fromSeq},{signal});
      try{
        let batch=[],bytes=0;
        for(const document of parts){
          const size=Buffer.byteLength(JSON.stringify(document))+1;
          if(size>1024*1024)throw new Error('NATIVE_DOCUMENT_TOO_LARGE');
          if(batch.length>=512||bytes+size>1024*1024){
            await this.request({op:'index_append',session,documents:batch},{signal});batch=[];bytes=0;
          }
          batch.push(document);bytes+=size;
        }
        if(batch.length)await this.request({op:'index_append',session,documents:batch},{signal});
        return await this.request({op:'index_commit',session},{signal});
      }catch(error){
        // Kill clears TEMP staging even after cancellation/transport failure;
        // never restart merely to send abort to a different process.
        this.#stop(error);throw error;
      }
    };
    const result=this.#imports.then(run);
    this.#imports=result.catch(()=>{});return result;
  }
  async close(){
    this.#closed=true;
    if(this.#pending.size===0){await this.#retire();await Promise.all(this.#retiring);return;}
    const child=this.#child;
    const stopped=child && child.exitCode===null && child.signalCode===null
      ?new Promise(resolve=>{child.once('exit',resolve);child.once('error',resolve);}):Promise.resolve();
    this.#stop(new Error('NATIVE_CLIENT_CLOSED'));
    await stopped;
    await Promise.all(this.#retiring);
  }
}
