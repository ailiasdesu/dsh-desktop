"""Compare identical literal searches, including every child in memory totals."""
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import re
import time
import psutil

repo=Path(__file__).resolve().parents[2]
label=sys.argv[1] if len(sys.argv)>1 else 'index'
assert re.fullmatch(r'[a-z0-9-]+',label)
out=repo/'docs/performance/results'/label
out.mkdir(parents=True,exist_ok=True)
node=Path(os.environ.get('DSH_INSTALL_PATH','C:/Users/34021/AppData/Local/DSH Desktop'))/'runtime/node.exe'
def run(mode, name):
    path=out/f'{name}.json'
    with (out/f'{name}.log').open('w',encoding='utf-8') as log:
        child=subprocess.Popen([str(node),'--max-old-space-size=512',str(repo/'scripts/performance/index-sample.mjs'),mode,str(path)],
            cwd=repo,stdout=log,stderr=subprocess.STDOUT,creationflags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0)
        process=psutil.Process(child.pid)
        peak=0;started=time.monotonic()
        while child.poll() is None:
            if time.monotonic()-started>120:
                for p in process.children(recursive=True):p.kill()
                child.kill();child.wait();raise RuntimeError('Index benchmark timeout')
            try:
                total=0
                for p in [process]+process.children(recursive=True):
                    try:
                        m=p.memory_info();total+=getattr(m,'private',m.vms)
                    except psutil.NoSuchProcess:pass
                peak=max(peak,total)
            except psutil.NoSuchProcess:break
            time.sleep(0.02)
        assert child.wait()==0,f'Inspect {name}.log'
    value=json.loads(path.read_text());value['peakTotalPrivateMiB']=peak/2**20
    path.write_text(json.dumps(value,indent=2),encoding='utf-8')
    print(f'{name}: peak total private {peak/2**20:.1f} MiB',flush=True)
    return value

prepared=run('prepare','prepare-or-reuse-index')
runs=[]
for i in range(5):
    for mode in (['native','official'] if i%2==0 else ['official','native']):runs.append(run(mode,f'{mode}-{i}'))
def pct(values,p):return sorted(values)[math.ceil(len(values)*p)-1]
summary={'prepareOrReuseIndex':prepared,'semantics':'case-folded literal per-event text, ascending seq; NOT ranked FTS',
         'fixtures':'six 16 MiB synthetic text histories','variants':[]}
for mode in ['official','native']:
    selected=[r for r in runs if r['mode']==mode];times=[q['ms'] for r in selected for q in r['queries']]
    summary['variants'].append({'mode':mode,'queries':len(times),'p50Ms':pct(times,.5),'p95Ms':pct(times,.95),
        'medianPeakTotalPrivateMiB':pct([r['peakTotalPrivateMiB'] for r in selected],.5)})
(out/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
print(json.dumps(summary['variants'],indent=2))
