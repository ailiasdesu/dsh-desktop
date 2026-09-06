import {readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {performance,monitorEventLoopDelay} from 'node:perf_hooks';
import {setImmediate} from 'node:timers/promises';
import assert from 'node:assert/strict';
import {backend} from './official.mjs';

const root=resolve(process.argv[2]);
const size=Number(process.argv[3]);
const output=resolve(process.argv[4]);
assert([1,2,5].includes(size));
const manifest=JSON.parse(await readFile(join(root,'manifest.json'),'utf8'));
assert.equal(manifest.kind,'dsh-performance-synthetic-v1');
const b=await backend(join(root,'sessions'),size);
const lag=monitorEventLoopDelay({resolution:10});lag.enable();
const sample=()=>({...process.memoryUsage(),resource:process.resourceUsage()});
const result={cacheSize:size,processCold:true,diskCache:'uncontrolled/shared OS cache',phases:[],before:sample()};
try {
  for (const phase of ['cold-sequence','hot-last','hot-pair','hot-five','revisit-sequence']) {
    const times=[];
    const entries=manifest.sessions.filter(s=>s.label==='large');
    const selected=phase==='hot-last'?Array(5).fill(entries.at(-1)):
      phase==='hot-pair'?Array.from({length:6},(_,i)=>entries.at(i%2===0?-2:-1)):
      phase==='hot-five'?Array.from({length:10},(_,i)=>entries[i%5+1]):entries;
    for (const expected of selected) {
      const start=performance.now();
      const source=await b.persistence.inspect(expected.id);
      assert.equal(source.events.length,expected.events);
      assert.equal(source.events.at(-1).type,'turn/end');
      assert.equal(source.events[1].data.content[0].text.startsWith(`fixture ${expected.id} turn 0 `),true);
      times.push(performance.now()-start);
      await setImmediate();
    }
    // Forced GC is only a retained-object diagnostic, not a production latency claim.
    global.gc?.();
    await setImmediate();
    result.phases.push({phase,ms:times,memoryAfterDiagnosticGc:sample()});
  }
} finally {
  result.eventLoopDelay={p95Ms:lag.percentile(95)/1e6,maxMs:lag.max/1e6};
  lag.disable();await b.close();
}
await writeFile(output,JSON.stringify(result,null,2));
console.log(JSON.stringify({cacheSize:size,phases:result.phases.map(p=>({phase:p.phase,ms:p.ms,heapMiB:p.memoryAfterDiagnosticGc.heapUsed/2**20}))}));
