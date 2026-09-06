import json
import math
import os
from pathlib import Path
import subprocess
import time
import psutil

repo=Path(__file__).resolve().parents[2]
out=repo/'docs/performance/results/files';out.mkdir(parents=True,exist_ok=True)
node=Path(os.environ.get('DSH_INSTALL_PATH','C:/Users/34021/AppData/Local/DSH Desktop'))/'runtime/node.exe'
script=repo/'scripts/performance/file-sample.mjs'
flags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0
subprocess.run([str(node),str(script),'prepare'],cwd=repo,check=True,creationflags=flags)
runs=[]
for iteration in range(5):
    modes=['buffer','stream','native'];modes=modes[iteration%3:]+modes[:iteration%3]
    for mode in modes:
        target=out/f'{mode}-{iteration}.json'
        with (out/f'{mode}-{iteration}.log').open('w',encoding='utf-8') as log:
            child=subprocess.Popen([str(node),'--max-old-space-size=512',str(script),mode,str(target)],
                cwd=repo,stdout=log,stderr=subprocess.STDOUT,creationflags=flags)
            parent=psutil.Process(child.pid);peak=0;started=time.monotonic()
            while child.poll() is None:
                if time.monotonic()-started>60:
                    for p in parent.children(recursive=True):p.kill()
                    child.kill();child.wait();raise RuntimeError('File benchmark timeout')
                try:
                    total=0
                    for p in [parent]+parent.children(recursive=True):
                        try:
                            m=p.memory_info();total+=getattr(m,'private',m.vms)
                        except psutil.NoSuchProcess:pass
                    peak=max(peak,total)
                except psutil.NoSuchProcess:break
                time.sleep(.02)
            assert child.wait()==0,f'Failed {mode} {iteration}'
        result=json.loads(target.read_text());result['peakTotalPrivateMiB']=peak/2**20
        target.write_text(json.dumps(result,separators=(',',':'))+'\n',encoding='utf-8');runs.append(result)
        print(f'{mode} {iteration}: {peak/2**20:.1f} MiB',flush=True)
def pct(values,p):return sorted(values)[math.ceil(len(values)*p)-1]
summary={'fileBytes':256*1024*1024,'processCold':True,'diskCacheControlled':False,'repetitions':5,'variants':[]}
for mode in ['buffer','stream','native']:
    selected=[r for r in runs if r['mode']==mode];ms=[n for r in selected for n in r['ms']]
    summary['variants'].append({'mode':mode,'p50Ms':pct(ms,.5),'p95Ms':pct(ms,.95),
        'medianPeakTotalPrivateMiB':pct([r['peakTotalPrivateMiB'] for r in selected],.5)})
(out/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
print(json.dumps(summary,indent=2))
