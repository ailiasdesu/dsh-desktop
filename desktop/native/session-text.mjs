import {randomUUID} from 'node:crypto';

/** Independent literal event-text index. Official code owns event decoding and
 * text extraction; this does not replace official ranked full-text search. */
export class SessionTextIndex {
  #epochs=new WeakMap();
  #tail=Promise.resolve();
  constructor({client,sessions,persistence,observe,extractText,kernelVersion}){
    Object.assign(this,{client,sessions,persistence,observe,extractText,kernelVersion});
  }
  #stamp(source,cursor,revision){
    return {kernel:this.kernelVersion,extractor:'official-event-text-v1',source,cursor,revision};
  }
  #liveStamp(session){
    if(!this.#epochs.has(session))this.#epochs.set(session,randomUUID());
    return this.#stamp('live',session.seq-1,this.#epochs.get(session));
  }
  #compatible(value){return value?.kernel===this.kernelVersion&&value.extractor==='official-event-text-v1';}
  async #current(id){
    const live=this.sessions.get(id);
    if(live)return this.#liveStamp(live);
    // Public concrete-backend method, enabled only for the verified kernel.
    if(typeof this.persistence.readStoredRevision!=='function')throw new Error('NATIVE_REVISION_UNAVAILABLE');
    return this.#stamp('disk',undefined,await this.persistence.readStoredRevision(id));
  }
  async #refresh(id,signal){
    const state=await this.client.request({op:'index_state',session:id},{signal});
    let previous;
    try{previous=state&&JSON.parse(state.revision);}catch{/* foreign cache -> full rebuild */}
    const current=await this.#current(id);
    if(this.#compatible(previous)&&previous.source===current.source&&previous.revision===current.revision
      &&(current.source==='disk'||previous.cursor===current.cursor))return state.revision;
    signal?.throwIfAborted();
    const live=this.sessions.get(id);
    const observation=live?undefined:await this.observe(id,{signal,projectionMode:'none'});
    try{
      const stamp=live?this.#liveStamp(live):this.#stamp('disk',observation.cursor,observation.revision);
      if(stamp.revision===undefined)throw new Error('NATIVE_REVISION_UNAVAILABLE');
      const incremental=live&&this.#compatible(previous)&&previous.source==='live'
        &&previous.revision===stamp.revision&&Number.isSafeInteger(previous.cursor)&&previous.cursor<stamp.cursor;
      const from=incremental?previous.cursor+1:0;
      const documents=[];
      for(let seq=from;seq<=stamp.cursor;seq++){
        signal?.throwIfAborted();
        const event=live?live.eventAt(seq):observation.events[seq];
        if(!event||event.seq!==seq)throw new Error('NATIVE_SOURCE_SEQUENCE_CHANGED');
        const text=this.extractText(event);
        if(text)documents.push({seq,text});
      }
      const revision=JSON.stringify(stamp);
      await this.client.importDocuments({session:id,revision,documents,
        ...(incremental?{baseRevision:state.revision,fromSeq:from}:{})},{signal});
      return revision;
    }finally{observation?.[Symbol.dispose]();}
  }
  async #fallback(id,query,limit,signal){
    const observation=await this.observe(id,{signal,projectionMode:'none'});
    try{
      const hits=[];let has_more=false;
      const needle=query.toLowerCase();
      for(const event of observation.events){
        signal?.throwIfAborted();
        const text=this.extractText(event);
        if(!text.toLowerCase().includes(needle))continue;
        if(hits.length===limit){has_more=true;break;}
        hits.push({session:id,seq:event.seq,preview:Array.from(text).slice(0,240).join('')});
      }
      return {hits,has_more,engine:'official-fallback'};
    }finally{observation[Symbol.dispose]();}
  }
  search(id,query,{limit=20,signal}={}){
    if(typeof id!=='string'||!id||typeof query!=='string'||!query||query.length>1024
      ||!Number.isSafeInteger(limit)||limit<1||limit>100)return Promise.reject(new Error('INVALID_SEARCH'));
    const run=async()=>{
      signal?.throwIfAborted();
      if(this.kernelVersion!=='0.1.2-rc.1')return this.#fallback(id,query,limit,signal);
      try{
        for(let attempt=0;attempt<2;attempt++){
          const revision=await this.#refresh(id,signal);
          const result=await this.client.request({op:'search',session:id,query,limit},{signal});
          const after=await this.#current(id);
          const indexed=JSON.parse(revision);
          if(after.source!==indexed.source||after.revision!==indexed.revision
            ||after.source==='live'&&after.cursor!==indexed.cursor)continue;
          if(result.hits.some(hit=>hit.revision!==revision))continue;
          return {hits:result.hits.map(({revision,...hit})=>hit),has_more:result.has_more,engine:'rust'};
        }
      }catch(error){
        if(signal?.aborted)throw signal.reason;
        // Derived cache failure never changes or prevents official reads.
      }
      return this.#fallback(id,query,limit,signal);
    };
    const result=this.#tail.then(run);this.#tail=result.catch(()=>{});return result;
  }
}
