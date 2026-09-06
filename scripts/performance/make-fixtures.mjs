import {mkdir, writeFile, stat} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {createHash} from 'node:crypto';
import {backend, moduleOf} from './official.mjs';

const root = resolve(process.argv[2] ?? 'target/performance-fixtures');
await mkdir(root, {recursive:true});
const marker = join(root, 'manifest.json');
try { await stat(marker); throw new Error('Fixtures already exist; reuse the manifest or select a new fixture root'); }
catch (error) { if (error.code !== 'ENOENT') throw error; }
const {Session} = await moduleOf('dsh-session');
const b = await backend(join(root, 'sessions'));
const manifest = {kind:'dsh-performance-synthetic-v1', sessions:[], createdAt:new Date().toISOString()};
try {
  // Six 16 MiB histories exercise the default five-entry retention policy.
  // Small/medium variants are retained for subsequent tool and page benchmarks.
  for (const [label, turns, chars, copies] of [['small',16,512,1],['medium',256,4096,1],['large',1024,16384,6]]) {
    for (let copy=0; copy<copies; copy++) {
      const id=`perf-${label}-${copy}`;
      const session=Session.create(id);
      await b.persistence.create(session.header);
      let bytes=0;
      for (let i=0;i<turns;i++) {
        const block=createHash('sha256').update(`${id}:${i}`).digest('hex');
        const content=`fixture ${id} turn ${i} `+block.repeat(Math.ceil(chars/block.length)).slice(0,chars);
        session.append('turn/start',{turn:i+1});
        session.append('user/message',{content:[{type:'text',text:content}],source:{kind:'user'}},{surfaceOp:'append'});
        session.append('turn/end',{turn:i+1,reason:{kind:'completed'}});
        bytes+=Buffer.byteLength(content);
      }
      await b.persistence.append(id,session.snapshotEvents());
      const location=b.persistence.locate(session.header);
      manifest.sessions.push({id,label,events:session.seq,textBytes:bytes,compressedBytes:(await stat(location.path)).size});
      console.log(`${id}: ${session.seq} events, ${bytes} text bytes`);
    }
  }
} finally { await b.close(); }
await writeFile(marker,JSON.stringify(manifest,null,2));
