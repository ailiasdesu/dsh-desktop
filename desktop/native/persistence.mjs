import {stat} from 'node:fs/promises';
import {relative,resolve} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {performance} from 'node:perf_hooks';
import {setImmediate} from 'node:timers/promises';

const VERSION='0.1.2-rc.1';
const revisionOf=s=>[s.dev,s.ino,s.size,s.mtimeNs,s.ctimeNs].join(':');
const natural=n=>Number.isSafeInteger(n)&&n>=0&&!Object.is(n,-0);

function storageHeader(line,version){
  if(!line||typeof line!=='object'||Array.isArray(line)||line.type!=='session'||line.version!==version
    ||typeof line.id!=='string'||!natural(line.createdAt)||!natural(line.delegationDepth)
    ||line.seedLength!==undefined&&!natural(line.seedLength)
    ||line.origin!==undefined&&line.origin!=='subagent'
    ||line.agentPreset!==undefined&&typeof line.agentPreset!=='string'
    ||Object.hasOwn(line,'sandboxMode')||Object.hasOwn(line,'approvalPolicy'))throw new Error('NATIVE_HEADER_MISS');
  return {meta:{version:line.version,id:line.id,createdAt:line.createdAt,
    ...(line.cwd!==undefined?{cwd:line.cwd}:{}),...(line.parentSession!==undefined?{parentSession:line.parentSession}:{}),
    isSeeded:line.seedLength!==undefined,...(line.origin!==undefined?{origin:line.origin}:{}),
    delegationDepth:line.delegationDepth,...(line.agentPreset!==undefined?{agentPreset:line.agentPreset}:{})},
    inheritedEventCount:line.seedLength??0};
}

/** Framing only; packed rows and provenance remain decoded by official exports. */
class EventScanner {
  fragments=[];fragmentBytes=0;events=[];header;frames=0;decodedBytes=0;
  constructor(sessionApi){this.api=sessionApi;}
  accept(progress){
    if(progress.frame!==this.frames)throw new Error('NATIVE_FRAME_ORDER');
    if(progress.end){
      if(this.frames===0&&(!this.header||this.fragmentBytes))throw new Error('NATIVE_HEADER_FRAME_MISS');
      this.frames++;return;
    }
    if(!Buffer.isBuffer(progress.data))throw new Error('NATIVE_INVALID_CHUNK');
    const bytes=progress.data;this.decodedBytes+=bytes.length;
    let start=0,end;
    while((end=bytes.indexOf(10,start))>=0){
      const tail=bytes.subarray(start,end);
      const line=this.fragments.length?Buffer.concat([...this.fragments,tail],this.fragmentBytes+tail.length):tail;
      this.fragments=[];this.fragmentBytes=0;this.line(line);start=end+1;
    }
    if(start<bytes.length){const fragment=bytes.subarray(start);this.fragments.push(fragment);this.fragmentBytes+=fragment.length;}
  }
  line(bytes){
    let row=JSON.parse(bytes.toString('utf8'));
    if(!this.header){this.header=storageHeader(row,this.api.SESSION_FORMAT_VERSION);return;}
    if(this.frames===0)throw new Error('NATIVE_HEADER_FRAME_MISS');
    if(!row||typeof row!=='object'||Array.isArray(row))throw new Error('NATIVE_RECORD_MISS');
    if(row.sourceEventSeqs!==undefined){
      if(!Number.isSafeInteger(row.seq)||row.seq<0)throw new Error('NATIVE_RECORD_MISS');
      row={...row,sourceEventSeqs:this.api.decodeSeqRanges(row.sourceEventSeqs,row.seq)};
    }
    for(const event of this.api.decodeStorageRecord(row)){
      if(event.seq!==this.events.length)throw new Error('NATIVE_SEQUENCE_MISS');
      this.events.push(event);
    }
  }
  finish(result){
    if(!this.header||this.fragmentBytes||result.frames!==this.frames||result.decoded_bytes!==this.decodedBytes)throw new Error('NATIVE_INCOMPLETE_STREAM');
    return {...this.header,events:this.events};
  }
}

/** Only the public backend read seam is overridden. Writes, crash recovery,
 * preparation ownership and final replay validation stay in the base class. */
export function nativePersistenceClass({JsonlSessionPersistence,sessionApi,versions,client,Service,
  minCompressedBytes=32*1024*1024,maxDecodedBytes=8*1024*1024*1024,measure=false}){
  const compatible=['jsonl','session','persistence'].every(key=>versions[key]===VERSION);
  const init=Service?.init??Symbol('unused native initialization');
  return class NativeJsonlSessionPersistence extends JsonlSessionPersistence {
    nativeMetrics={attempts:0,hits:0,fallbacks:0,skipped:0};
    nativeSizeHints=new Map();
    constructor(ctx,config){
      super(ctx,config);this.nativeRoot=resolve(config.root);
      this.nativeHintsReady=compatible&&minCompressedBytes>0
        ?this.listSnapshots().catch(()=>{/* a hint is not an authoritative read */}):Promise.resolve();
      ctx.on('session/disposed',session=>{this.nativeSizeHints.delete(session.id);},{global:true});
      ctx.effect(()=>()=>client.close(),'desktop-native persistence helper');
    }
    async listSnapshots(...args){
      const snapshots=await super.listSnapshots(...args);
      const hints=new Map();
      for(const snapshot of snapshots){
        const bytes=Number(String(snapshot.revision).split(':')[2]);
        if(Number.isSafeInteger(bytes)&&bytes>=0)hints.set(snapshot.header.id,bytes);
      }
      this.nativeSizeHints=hints;
      return snapshots;
    }
    async [init](){await super[init]?.();await this.nativeHintsReady;}
    async loadStored(id,signal){
      if(!compatible||this.config.compression==='none')return super.loadStored(id,signal);
      if(this.nativeSizeHints.has(id)&&this.nativeSizeHints.get(id)<minCompressedBytes){
        this.nativeMetrics.skipped++;return super.loadStored(id,signal);
      }
      let scanner;
      const started=performance.now();let parseMs=0;
      try{
        signal?.throwIfAborted();
        const before=await super.readStoredRevision(id,signal);
        if(before===undefined)return super.loadStored(id,signal);
        const physicalSize=Number(String(before).split(':')[2]);
        if(Number.isSafeInteger(physicalSize)&&physicalSize>=0){
          this.nativeSizeHints.set(id,physicalSize);
          if(physicalSize<minCompressedBytes){this.nativeMetrics.skipped++;return super.loadStored(id,signal);}
        }
        const header=(await super.list(signal)).find(header=>header.id===id);
        if(!header)return super.loadStored(id,signal);
        const location=super.locate(header);
        const beforeFile=await stat(location.path,{bigint:true});
        if(!location.path.endsWith('.zstd')||beforeFile.size<BigInt(minCompressedBytes)){
          this.nativeMetrics.skipped++;return super.loadStored(id,signal);
        }
        if(revisionOf(beforeFile)!==before)throw new Error('NATIVE_REVISION_MISS');
        this.nativeMetrics.attempts++;
        scanner=new EventScanner(sessionApi);
        const streamStart=performance.now();
        let deadline=performance.now()+16;
        const result=await client.request({op:'read_zstd',root:this.nativeRoot,
          path:relative(this.nativeRoot,location.path),max_bytes:maxDecodedBytes},{signal,timeoutMs:120000,
          onProgress:progress=>{
            signal?.throwIfAborted();const parseStart=measure?performance.now():0;scanner.accept(progress);
            if(measure)parseMs+=performance.now()-parseStart;
            if(performance.now()>=deadline)return setImmediate().then(()=>{deadline=performance.now()+16;});
          }});
        const prefix=scanner.finish(result);
        const streamEnd=performance.now();
        if(prefix.meta.id!==id||!isDeepStrictEqual(prefix.meta,header))throw new Error('NATIVE_IDENTITY_MISS');
        const afterFile=await stat(location.path,{bigint:true});
        const after=await super.readStoredRevision(id,signal);
        if(before!==after||revisionOf(afterFile)!==before)throw new Error('NATIVE_REVISION_MISS');
        this.nativeMetrics.hits++;
        if(measure)this.nativeMetrics.lastRead={metadataMs:streamStart-started,streamMs:streamEnd-streamStart,parseMs,totalMs:performance.now()-started};
        return {...prefix,revision:before};
      }catch(error){
        // Release unaccepted graphs before the official fallback allocates its source.
        scanner=undefined;
        signal?.throwIfAborted();this.nativeMetrics.fallbacks++;
        return super.loadStored(id,signal);
      }
    }
  };
}
