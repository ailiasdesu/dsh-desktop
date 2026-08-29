#!/usr/bin/env bash
# updater-dryrun.sh — 内核更新准备流程 dry-run（真实 tgz 级验证）
# 验证内容：registry /latest → tgz → sha512(integrity, node crypto) → 解压 package →
#          npm install 完整树（--omit=dev --no-audit --no-fund）→ 隔离 DSH_HOME 冒烟
# 环境：git bash + node/curl/tar；npm 用系统版（流程正确性验证，与 Rust materialize 语义同构）
# 用法：bash scripts/updater-dryrun.sh
set -euo pipefail
W="$(mktemp -d)"
echo "WORKDIR=$W"
cd "$W"

echo "[1] fetch registry latest"
curl -sSLf -o meta.json https://registry.npmjs.org/@deepseek-ai/dsh
LATEST=$(node -p "JSON.parse(require('fs').readFileSync('meta.json','utf8'))['dist-tags']['latest']")
DIST=$(node -p "const d=JSON.parse(require('fs').readFileSync('meta.json','utf8')); JSON.stringify(d['versions'][d['dist-tags']['latest']]['dist'])")
TARBALL=$(node -p "JSON.parse('$DIST').tarball")
INTEG=$(node -p "JSON.parse('$DIST').integrity")
echo "latest=$LATEST"

echo "[2] download tgz"
curl -sSLf -o dsh.tgz "$TARBALL"
ls -la dsh.tgz

echo "[3] sha512 verify (node crypto vs base64 integrity)"
node -e "const fs=require('fs');const crypto=require('crypto');const exp=Buffer.from('$INTEG'.replace('sha512-',''),'base64');const h=crypto.createHash('sha512').update(fs.readFileSync('dsh.tgz')).digest();if(!h.equals(exp)){console.error('INTEG_MISMATCH');process.exit(3)}console.log('INTEG_OK')"

echo "[4] extract tarball"
mkdir pkg && tar -xzf dsh.tgz -C pkg
ls pkg/package/ | head -8

echo "[5] npm install full tree (in package dir, official flags)"
cd pkg/package
npm install --omit=dev --no-audit --no-fund
du -sh . | cut -f1
cd "$W"

echo "[6] isolated smoke (DSH_HOME)"
DSH_HOME="$W/smoke-home" node "$W/pkg/package/lib/bin.js" web --help > smoke.log 2>&1 && echo "SMOKE_EXIT=0" || { echo "SMOKE_EXIT=$?"; tail -5 smoke.log; exit 7; }

echo "[7] family co-release sanity"
node -p "const d=JSON.parse(require('fs').readFileSync('pkg/package/package.json','utf8'));const names=Object.keys(d.dependencies).filter(n=>n.startsWith('@deepseek-ai/'));let bad=0;for(const n of names){try{const p=JSON.parse(require('fs').readFileSync('pkg/package/node_modules/'+n+'/package.json','utf8'));if(p.version!==d.version)bad++}catch(e){bad++}}console.log('family_deps='+names.length+' mismatched='+bad);process.exit(bad>0?1:0)"

echo "DRYRUN_DONE (latest=$LATEST) workdir=$W"
