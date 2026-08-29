#!/usr/bin/env bash
# prepare-runtime.sh — 从系统 Node 官方发行版准备捆绑 runtime/（幂等）
# 产出: runtime/node.exe + runtime/node_modules/npm/bin/npm-cli.js（更新物化所需，自包含 P0）
# 用法: bash scripts/prepare-runtime.sh [NODE_HOME]  （缺省探测系统 node 安装）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NODE_HOME="${1:-}"
if [ -z "$NODE_HOME" ]; then
  NODE_HOME="$(dirname "$(dirname "$(command -v node)")")"
fi
echo "NODE_HOME=$NODE_HOME"
[ -f "$NODE_HOME/node.exe" ] || { echo "FATAL: node.exe not found under $NODE_HOME"; exit 1; }
[ -f "$NODE_HOME/node_modules/npm/bin/npm-cli.js" ] || { echo "FATAL: node_modules/npm not found under $NODE_HOME"; exit 1; }
mkdir -p "$ROOT/runtime/node_modules"
cp "$NODE_HOME/node.exe" "$ROOT/runtime/node.exe"
rm -rf "$ROOT/runtime/node_modules/npm"
cp -r "$NODE_HOME/node_modules/npm" "$ROOT/runtime/node_modules/npm"
echo "runtime prepared:"
ls -la "$ROOT/runtime/node.exe" "$ROOT/runtime/node_modules/npm/bin/npm-cli.js"
