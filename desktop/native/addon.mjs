import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);

/** Optional Node-API read transport; no changes to official kernel packages. */
export class AddonReadTransport {
  #module;#readers=new Set();#running=new Set();#closed=false;
  constructor(path){this.path=path;}
  request(operation,options={}){
    if(this.#closed)return Promise.reject(new Error('NATIVE_READER_CLOSED'));
    if(this.#running.size>=4)return Promise.reject(new Error('NATIVE_READER_QUEUE_FULL'));
    const work=this.#read(operation,options);this.#running.add(work);
    const done=()=>this.#running.delete(work);work.then(done,done);return work;
  }
  async #read(operation,{signal,onProgress,timeoutMs=120000}){
    signal?.throwIfAborted();
    if(operation.op!=='read_zstd'||typeof onProgress!=='function')throw new Error('NATIVE_READ_OPERATION_REQUIRED');
    this.#module??=require(this.path);
    if(this.#module.protocolVersion()!==1)throw new Error('UNSUPPORTED_NATIVE_READER');
    const reader=new this.#module.NativeReader(operation.root,operation.path,operation.max_bytes);
    this.#readers.add(reader);
    let timedOut=false;
    const timer=setTimeout(()=>{timedOut=true;reader.close();},timeoutMs);timer.unref();
    const abort=()=>reader.close();signal?.addEventListener('abort',abort,{once:true});
    const readNext=()=>reader.next().then(value=>({value}),error=>({error}));
    let pending;
    try{
      pending=readNext();
      for(;;){
        signal?.throwIfAborted();if(timedOut)throw new Error('NATIVE_TIMEOUT');
        const observed=await pending;if(observed.error)throw observed.error;
        const batch=observed.value;
        signal?.throwIfAborted();if(timedOut)throw new Error('NATIVE_TIMEOUT');
        pending=batch.done?undefined:readNext();
        for(const part of batch.parts){
          const result=onProgress(part.end?{frame:part.frame,end:true}:{frame:part.frame,data:part.data});
          if(result?.then)await result;
          signal?.throwIfAborted();if(timedOut)throw new Error('NATIVE_TIMEOUT');
          if(this.#closed)throw new Error('NATIVE_READER_CLOSED');
        }
        signal?.throwIfAborted();if(timedOut)throw new Error('NATIVE_TIMEOUT');
        if(this.#closed)throw new Error('NATIVE_READER_CLOSED');
        if(batch.done)return {frames:batch.frames,decoded_bytes:batch.decodedBytes,compressed_bytes:batch.compressedBytes};
      }
    }catch(error){if(signal?.aborted)throw signal.reason;if(timedOut)throw new Error('NATIVE_TIMEOUT');throw error;}
    finally{clearTimeout(timer);signal?.removeEventListener('abort',abort);reader.close();if(pending)await pending;this.#readers.delete(reader);}
  }
  async close(){
    this.#closed=true;for(const reader of this.#readers)reader.close();
    await Promise.allSettled(this.#running);
  }
}
