#!/usr/bin/env bash
# updater-dryrun.sh — 内核更新准备流程 dry-run（真实 tgz 级验证）
# 与 src-tauri/src/updater.rs 的 prepare_new_kernel 对齐：
#   registry dist-tags → curl tgz → sha512 integrity → 捆绑 npm 重放安装 (--prefix 空目录=根工程布局)
#   → 根布局校验 (lib/bin.js + package.json version) → dsh-* 家族 co-release 校验 → isolated smoke
# 环境: node>=22（系统 node 即可，仅验证 npm 物化语义）; 需联网 + npm registry
# 用法: bash scripts/updater-dryrun.sh [工作目录]   （缺省 /tmp/dsh-dryrun）
set -u
DR="${1:-/tmp/dsh-dryrun}"
NPMCLI=""
if [ -f "runtime/node_modules/npm/bin/npm-cli.js" ]; then
  NPMCLI="$(pwd)/runtime/node_modules/npm/bin/npm-cli.js"
elif [ -n "${NPMCLI:-}" ]; then
  :
fi
echo "NPMCLI=$NPMCLI"
[ -f "$NPMCLI" ] || { echo "FATAL: npm-cli not found (bundle runtime/node_modules/npm or set NPMCLI env)"; exit 9; }
rm -rf "$DR"; mkdir -p "$DR"; cd "$DR" || exit 9

echo "[1] registry metadata"
curl -sSLf -o meta.json https://registry.npmjs.org/@deepseek-ai/dsh || { echo FAIL_META; exit 1; }
LATEST=$(node -p "JSON.parse(require('fs').readFileSync('meta.json','utf8'))['dist-tags']['latest']")
INTEG=$(node -p "const d=JSON.parse(require('fs').readFileSync('meta.json','utf8')); d['versions'][d['dist-tags']['latest']]['dist']['integrity']")
TARBALL=$(node -p "const d=JSON.parse(require('fs').readFileSync('meta.json','utf8')); d['versions'][d['dist-tags']['latest']]['dist']['tarball']")
echo "latest=$LATEST"

echo "[2] download tgz"
curl -sSLf -o dsh.tgz "$TARBALL" || { echo FAIL_DL; exit 2; }
echo "tgz_bytes=$(stat -c %s dsh.tgz)"

echo "[3] sha512 integrity"
EXP=$(echo "$INTEG" | sed "s/^sha512-//")
python -c "import hashlib,base64; h=hashlib.sha512(open('dsh.tgz','rb').read()).digest(); exp=base64.b64decode('$EXP'); print('INTEG_OK' if h==exp else 'INTEG_MISMATCH'); exit(0 if h==exp else 3)"

echo "[4] npm replay install (--prefix empty dir => root-project layout)"
node "$NPMCLI" install --prefix "$DR/kernel.new" --omit=dev --no-audit --no-fund "$DR/dsh.tgz" > npm.log 2>&1
echo "npm_exit=$?"
tail -3 npm.log

echo "[5] root-layout verify (lib/bin.js + version)"
ls "$DR/kernel.new/lib/bin.js" || { echo FAIL_BIN; exit 5; }
node -p "console.log('root_version='+JSON.parse(require('fs').readFileSync('$DR/kernel.new/package.json','utf8')).version)"

echo "[6] family co-release check"
node -p "const d=JSON.parse(require('fs').readFileSync('$DR/kernel.new/package.json','utf8')); const names=Object.keys(d.dependencies).filter(n=>n.startsWith('@deepseek-ai/')); let bad=0; for (const n of names){ try { const p=JSON.parse(require('fs').readFileSync('$DR/kernel.new/node_modules/'+n+'/package.json','utf8')); if (p.version!==d.version) bad++; } catch(e) { bad++; } } console.log('family_deps='+names.length+' mismatched='+bad); exit(bad>0?6:0)"

echo "[7] isolated smoke (web --help)"
DSH_HOME="$DR/smoke-home" node "$DR/kernel.new/lib/bin.js" web --help > smoke.log 2>&1
echo "smoke_exit=$?"
tail -2 smoke.log

echo "[8] size"
du -sh "$DR/kernel.new" | cut -f1
echo "DRYRUN_DONE"
