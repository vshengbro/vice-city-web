#!/bin/bash
# 构建 wasm 并灌进 www/pkg —— 静态服务器直接吃这个目录。
#
# wasm 不在 ./target(CARGO_TARGET_DIR 指到共享缓存),所以 wasm-bindgen
# 必须显式喂绝对路径;漏了 export 会报 `failed reading 'target/...'`。
set -euo pipefail

cd /Users/sqs/code/vice-city-web
export CARGO_TARGET_DIR=/Users/sqs/Library/Caches/cargo-target/vcweb

cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir www/pkg --out-name vice_city_web \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/vice_city_web.wasm"
echo "# built -> www/pkg"