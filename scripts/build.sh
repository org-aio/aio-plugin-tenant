#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

target_dir=${CARGO_TARGET_DIR:-target}
dx_bin=${DX:-dx}
# 每次从当前构建生成前端，避免把旧版本的资源带入整包。
rm -rf dist/frontend "$target_dir/dx/aio-plugin-tenant-client/release/web/public"
"$dx_bin" build --package aio-plugin-tenant-client --web --release --inject-loading-scripts false --debug-symbols false
node scripts/package-frontend.mjs "$target_dir/dx/aio-plugin-tenant-client/release/web/public"

if [ "${1:-}" = "--process" ]; then
  cargo zigbuild --locked --release --target x86_64-unknown-linux-gnu.2.17 -p aio-plugin-tenant-server --bins
  cp "$target_dir/x86_64-unknown-linux-gnu/release/tenant-server" dist/tenant-server
else
  cargo build --locked --release -p aio-plugin-tenant-server --bins
  cp "$target_dir/release/tenant-server" dist/tenant-server
fi
