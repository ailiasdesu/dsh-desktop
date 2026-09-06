import concurrent.futures
import http.cookiejar
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import sys
import time
import urllib.request

repo=Path(__file__).resolve().parents[2]
install=Path(os.environ.get('DSH_INSTALL_PATH','C:/Users/34021/AppData/Local/DSH Desktop'))
accept=install/'repair-checks/acceptance'
work=repo/'target/native-runtime';work.mkdir(parents=True,exist_ok=True)
resources=Path(os.environ.get('DSH_NATIVE_RUNTIME_RESOURCES',str(repo))).resolve()
out=repo/'docs/performance/results'/('runtime' if resources==repo else 'installed-runtime');out.mkdir(parents=True,exist_ok=True)
home=accept/'home'
sessions=work/'custom-sessions'
overlay=work/'driver.yml'
overlay.write_text('\n'.join([
    '- id: session-persistence-jsonl',
    '  config:',
    '    root: '+json.dumps(str(sessions)),
    '    preparedSessionCacheSize: 3',
    '    packChunks: false',
    '- insert:',
    '    - id: native-runtime-driver',
    '      name: '+json.dumps((repo/'scripts/performance/runtime-driver.mjs').as_uri()),
])+'\n',encoding='utf-8')
subprocess.run([str(install/'runtime/node.exe'),str(repo/'scripts/performance/native-overlay.mjs'),str(resources),str(work/'native.yml')],check=True,cwd=repo)
reports=[]
for enabled in [True,False]:
    subprocess.run([str(install/'runtime/node.exe'),str(repo/'scripts/performance/prepare-runtime-fixture.mjs'),'--refresh'],check=True,cwd=repo,creationflags=subprocess.CREATE_NO_WINDOW)
    label='native' if enabled else 'fallback'
    token=secrets.token_urlsafe(24)
    env=os.environ.copy();env.update(DSH_HOME=str(home),DSH_NATIVE_ACCEPTANCE_HOME=str(home),
      DSH_NATIVE_ACCEPTANCE_TOKEN=token,DSH_NATIVE_ACCEPTANCE_WORKSPACE=str(accept/'workspaces/one'),
      DSH_ACCEPTANCE_ROOT=str(accept),DSH_ACCEPTANCE_TOKEN=(accept/'token.txt').read_text(),
      DSH_DESKTOP_NATIVE_DISABLED='0' if enabled else '1',NODE_OPTIONS='--max-old-space-size=1024')
    if '--profile' in sys.argv:env['DSH_NATIVE_PROFILE_RESULT']=str(work/f'{label}.cpuprofile')
    stdout=work/f'{label}.stdout.log';stderr=work/f'{label}.stderr.log'
    command=[str(install/'runtime/node.exe'),str(install/'kernel/lib/bin.js'),'--profile','web',
      '--patch',str(accept/'overlay.yml'),'--patch',str(overlay),'--patch',str(work/'native.yml'),'--no-open','--port','0']
    report={'mode':label,'resources':str(resources)}
    with stdout.open('w',encoding='utf-8') as log,stderr.open('w',encoding='utf-8') as err:
        child=subprocess.Popen(command,cwd=accept/'workspaces/one',env=env,stdout=log,stderr=err,stdin=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            started=time.monotonic();ready=None
            while time.monotonic()-started<45:
                if child.poll() is not None:raise RuntimeError(f'{label} exited before ready; see {stderr}')
                matches=re.findall(r'http://127\.0\.0\.1:(\d+)/\?token=([\w-]+)',stdout.read_text(encoding='utf-8',errors='replace'))
                if matches:ready=matches[-1];break
                time.sleep(.1)
            if not ready:raise RuntimeError(f'{label} readiness timeout')
            port,launch_token=ready;origin='http://127.0.0.1:'+port
            opener=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
            opener.open(origin+'/?token='+launch_token,timeout=15).close()
            def request(action):
                req=urllib.request.Request(origin+'/native-acceptance',data=json.dumps({'action':action}).encode(),headers={'content-type':'application/json','x-native-acceptance':token})
                with opener.open(req,timeout=60) as response:return json.load(response)
            status=request('status');assert status['ok'],status
            assert status['value']['native']==enabled,status
            assert Path(status['value']['config']['root'])==sessions,status
            assert status['value']['config']['preparedSessionCacheSize']==3,status
            assert status['value']['config']['packChunks'] is False,status
            report['status']=status
            if '--profile' in sys.argv:assert request('profile-start')['ok']
            def history_snapshot():
                wsenv=env.copy();wsenv['DSH_NATIVE_WS_ORIGIN']=origin
                wsenv['DSH_NATIVE_WS_COOKIE']='; '.join(cookie.name+'='+cookie.value for handler in opener.handlers if isinstance(handler,urllib.request.HTTPCookieProcessor) for cookie in handler.cookiejar)
                completed=subprocess.run([str(install/'runtime/node.exe'),str(repo/'scripts/performance/history-ws.mjs')],env=wsenv,cwd=repo,text=True,encoding='utf-8',stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW,timeout=75)
                assert completed.returncode==0,completed.stderr[-1200:]
                return json.loads(completed.stdout)
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
                future=executor.submit(history_snapshot);health=[]
                while not future.done():
                    begin=time.monotonic()
                    with urllib.request.urlopen(origin+'/health',timeout=10) as response:assert response.status==200
                    health.append((time.monotonic()-begin)*1000);time.sleep(.01)
                report['historySnapshot']=future.result()
            if '--profile' in sys.argv:report['cpuProfile']=request('profile-stop')
            inspected=request('inspect')
            assert inspected['ok'] and 12288<=inspected['value']['events']<=12304,inspected
            assert inspected['value']['originalPrefixLast']=='turn/end',inspected
            if enabled:assert inspected['value']['metrics']['hits']>=1,inspected
            report['inspect']=inspected;report['healthLatencyMs']=health
            if enabled:
                tool=request('tool');assert tool['ok'] and not tool['value']['isError'],tool
                payload=json.loads(tool['value']['value']);assert payload['engine']=='rust' and len(payload['hits'])==1,payload
                report['tool']=payload
            opener.open(origin+'/quit',timeout=15).close();assert child.wait(timeout=15)==0
            report['exitCode']=0;reports.append(report)
            (out/f'{label}.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
            print(f'{label}: config preserved, history correct, clean exit; max health latency {max(health,default=0):.1f} ms',flush=True)
        finally:
            if child.poll() is None:
                import psutil
                for p in psutil.Process(child.pid).children(recursive=True):
                    try:p.kill()
                    except psutil.NoSuchProcess:pass
                child.kill();child.wait()
assert reports[0]['inspect']['value']['metadataEventTypes']==reports[1]['inspect']['value']['metadataEventTypes']
(out/'summary.json').write_text(json.dumps(reports,ensure_ascii=False,indent=2),encoding='utf-8')
