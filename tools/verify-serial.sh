#!/usr/bin/env bash
# 串行跑验收探针,每个探针一个独立 Chrome 实例(用完即弃)。
#
# 6 路并行(verify-parallel.sh)在这台机器上会**整组崩掉**:6 个 SwiftShader
# WebGL 实例同时跑,大约 33 s 后全部报
# `GPU state invalid after WaitForGetOffsetInRange`,探针的 CDP WebSocket
# 随之被 Chrome 关闭(`ConnectionClosedError: no close frame received`),
# 日志里连一条判定行都没有。串行是同一批探针唯一能稳定跑完的调度方式,
# 代价是慢(p3 单组就要几分钟)。
#
#   tools/verify-serial.sh            # 跑全部 6 组
#   tools/verify-serial.sh p1 p3      # 只跑指定几组

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH="/Users/sqs/.hermes/cache/scratch/vcw-cdp"
PY="/Users/sqs/.hermes/hermes-agent/venv/bin/python3.11"
CHROME="/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
PORT=9241
# 探针列表**只认字面量 p1..p6**。外层 shell(zsh)会把后台 `nohup`
# Chrome 的 pid 一起塞进位置参数 —— 实测 `"$@"` 里混进了 `399` / `400`,
# 于是脚本去跑 `tools/sweep/399.py`。
#
# 这里用**字符串**而不是 bash 数组:macOS 自带 bash 3.2,它在 `set -u`
# 下对空数组的 `${#arr[@]}` 与展开都会抛 `unbound variable`。
WANT=""
for a in "$@"; do
  case "$a" in
    p1 | p2 | p3 | p4 | p5 | p6) WANT="$WANT $a" ;;
  esac
done
if [[ -z "$WANT" ]]; then
  WANT=" p1 p2 p3 p4 p5 p6"
fi

mkdir -p "$SCRATCH"
cd "$ROOT"

for g in $WANT; do
  pkill -9 -f "remote-debugging-port=${PORT}" 2>/dev/null
  sleep 2
  rm -rf "${SCRATCH}/serial"
  # 注意:这里**不能**加 `--disable-gpu`。WebGPU 的 adapter 走的是
  # 真实的 GPU 栈(实测 macOS 上拿到 `apple / metal-3`),而
  # `--disable-gpu` 会让 `requestAdapter()` 直接返回 null —— 页面
  # 上 `navigator.gpu` 还在,但一个 adapter 都没有。迁移到 euv 的
  # WebGPU 之后,加了这个 flag 的探针会**全部**失败(而且失败原因
  # 看起来像渲染层坏了,不是环境问题)。`--use-angle=swiftshader`
  # 只影响 WebGL,不影响 WebGPU。
  nohup "$CHROME" \
    --headless=new --no-sandbox \
    --use-gl=angle --use-angle=swiftshader --enable-unsafe-swiftshader \
    --disable-features=Translate,BackForwardCache,CalculateNativeWinOcclusion \
    --disable-component-update --disable-default-apps --disable-sync \
    --no-first-run --no-default-browser-check --disable-extensions \
    --metrics-recording-only --mute-audio --disable-breakpad \
    --disable-background-networking \
    --remote-debugging-port="${PORT}" --remote-allow-origins='*' \
    --user-data-dir="${SCRATCH}/serial" \
    --hide-scrollbars --window-size=1280,800 \
    --disable-background-timer-throttling \
    --disable-renderer-backgrounding \
    --disable-accelerated-2d-canvas \
    about:blank > "/tmp/c${PORT}.log" 2>&1 &
  sleep 8
  echo "########## ${g} ##########"
  VCW_CDP="http://127.0.0.1:${PORT}" "$PY" "${ROOT}/tools/sweep/${g}.py" \
    > "${SCRATCH}/${g}.log" 2>&1
  grep -aE '^ok  |^FAIL |passed$' "${SCRATCH}/${g}.log" || tail -3 "${SCRATCH}/${g}.log"
done

pkill -9 -f "remote-debugging-port=${PORT}" 2>/dev/null
exit 0
