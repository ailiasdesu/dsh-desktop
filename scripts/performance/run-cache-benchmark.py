"""Serial official-backend comparison. Only synthetic fixture data is read."""
import json
import math
import os
from pathlib import Path
import subprocess
import re
import sys
import time
import psutil

repo = Path(__file__).resolve().parents[2]
label=sys.argv[1] if len(sys.argv)>1 else 'cache'
assert re.fullmatch(r'[a-z0-9-]+',label)
out = repo / 'docs/performance/results' / label
out.mkdir(parents=True, exist_ok=True)
install = Path(os.environ.get('DSH_INSTALL_PATH', 'C:/Users/34021/AppData/Local/DSH Desktop'))
def percentile(values, fraction):
    return sorted(values)[math.ceil(len(values)*fraction)-1]

runs = []
# Rotate order to reduce systematic cache/thermal order bias.
for repetition in range(5):
    order = [1, 2, 5]
    order = order[repetition % 3:] + order[:repetition % 3]
    for size in order:
        result_path = out / f'cache-{size}-run-{repetition}.json'
        command = [str(install / 'runtime/node.exe'), '--expose-gc', '--max-old-space-size=512',
                   str(repo / 'scripts/performance/cache-sample.mjs'),
                   str(repo / 'target/performance-fixtures'), str(size), str(result_path)]
        with (out / f'cache-{size}-run-{repetition}.log').open('w', encoding='utf-8') as log:
            child = subprocess.Popen(command, cwd=repo, stdout=log, stderr=subprocess.STDOUT,
                                     creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
            process = psutil.Process(child.pid)
            peak_private = peak_rss = 0
            started = time.monotonic()
            while child.poll() is None:
                if time.monotonic() - started > 60:
                    child.kill()
                    child.wait()
                    raise RuntimeError('Bounded fixture benchmark exceeded 60 seconds')
                try:
                    memory = process.memory_info()
                    peak_private = max(peak_private, getattr(memory, 'private', memory.vms))
                    peak_rss = max(peak_rss, memory.rss)
                except psutil.NoSuchProcess:
                    break
                time.sleep(0.02)
            assert child.wait() == 0, f'Benchmark failed: {result_path}'
        result = json.loads(result_path.read_text())
        result['sampledPeakPrivateBytes'] = peak_private
        result['sampledPeakRssBytes'] = peak_rss
        result['repetition'] = repetition
        result_path.write_text(json.dumps(result, indent=2), encoding='utf-8')
        runs.append(result)
        print(f'cache={size}, run={repetition}, peak-private={peak_private/2**20:.1f} MiB', flush=True)

summary = {'kind': 'official-cache-policy-comparison', 'fixture': 'synthetic highly compressible histories',
           'repetitions': 5, 'processCold': True, 'diskCacheControlled': False,
           'forcedGc': 'between phases only, for retained-object diagnostics', 'variants': []}
for size in [1,2,5]:
    selected = [r for r in runs if r['cacheSize'] == size]
    phases = {}
    for label in ['cold-sequence','hot-last','hot-pair','hot-five','revisit-sequence']:
        rows = [p for r in selected for p in r['phases'] if p['phase'] == label]
        durations = [n for p in rows for n in p['ms']]
        phases[label] = {'p50Ms': percentile(durations,0.5), 'p95Ms': percentile(durations,0.95),
                         'medianRetainedHeapMiB': percentile([p['memoryAfterDiagnosticGc']['heapUsed']/2**20 for p in rows],0.5)}
    summary['variants'].append({'cacheSize':size, 'phases':phases,
        'medianPeakPrivateMiB':percentile([r['sampledPeakPrivateBytes']/2**20 for r in selected],0.5)})
(out / 'summary.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
print(json.dumps(summary, indent=2))
