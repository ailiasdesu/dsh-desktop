import json
import math
import os
from pathlib import Path
import subprocess
import re
import sys
import time
import psutil

repo=Path(__file__).resolve().parents[2]
label=sys.argv[1] if len(sys.argv)>1 else 'history-stream'
assert re.fullmatch(r'[a-z0-9-]+',label)
out=repo/'docs/performance/results'/label;out.mkdir(parents=True,exist_ok=True)
node=Path(os.environ.get('DSH_INSTALL_PATH','C:/Users/34021/AppData/Local/DSH Desktop'))/'runtime/node.exe'
flags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0
runs=[]
candidate=os.environ.get('DSH_PERF_TRANSPORT','native')
assert candidate in ['native','addon']
for iteration in range(5):
    for mode in ([candidate,'official'] if iteration%2==0 else ['official',candidate]):
        target=out/f'{mode}-{iteration}.json'
        with (out/f'{mode}-{iteration}.log').open('w',encoding='utf-8') as log:
            child=subprocess.Popen([str(node),'--max-old-space-size=768',str(repo/'scripts/performance/history-sample.mjs'),mode,str(target)],cwd=repo,stdout=log,stderr=subprocess.STDOUT,creationflags=flags)
            process=psutil.Process(child.pid);peak=0;started=time.monotonic()
            while child.poll() is None:
                if time.monotonic()-started>120:
                    for p in process.children(recursive=True):p.kill()
                    child.kill();child.wait();raise RuntimeError('History benchmark timeout')
                try:
                    total=0
                    for p in [process]+process.children(recursive=True):
                        try:
                            m=p.memory_info();total+=getattr(m,'private',m.vms)
                        except psutil.NoSuchProcess:pass
                    peak=max(peak,total)
                except psutil.NoSuchProcess:break
                time.sleep(.02)
            assert child.wait()==0,f'Failed {target}'
        result=json.loads(target.read_text());result['peakTotalPrivateMiB']=peak/2**20
        target.write_text(json.dumps(result,separators=(',',':'))+'\n',encoding='utf-8');runs.append(result)
        print(f'{mode} {iteration}: peak total private {peak/2**20:.1f} MiB',flush=True)
def pct(v,p):return sorted(v)[math.ceil(len(v)*p)-1]
summary={'fixture':'entropy-varied synthetic small/medium/large histories','variants':[]}
for mode in ['official',candidate]:
    selected=[r for r in runs if r['mode']==mode]
    summary['variants'].append({'mode':mode,'peakTotalPrivateMiB':pct([r['peakTotalPrivateMiB'] for r in selected],.5),
        'largeP95Ms':pct([x['ms'] for r in selected for x in r['reads'] if x['label'] in ['large','huge']],.95),
        'eventLoopP95Ms':pct([r['eventLoopDelay']['p95Ms'] for r in selected],.5),
        'eventLoopMaxMs':pct([r['eventLoopDelay']['maxMs'] for r in selected],.5)})
(out/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8');print(json.dumps(summary,indent=2))
