#!/usr/bin/env bash
# 测哪一组 Chrome flag 能拿到 WebGPU adapter。
#
# ## 为什么需要这个脚本
#
# `docs/webgpu-migration.md` 记着「探针不能带 `--disable-gpu`」,但
# 那只说了**不能**带什么,没说**要**带什么。`tools/verify-serial.sh` 现在
# 用的那组 flag(`--use-angle=swiftshader --enable-unsafe-swiftshader`)
# 是为 WebGL 探针调的,实测**拿不到 WebGPU adapter**。所以每一组 flag
# 都必须自己跑一遍,而不是照抄现有脚本。
#
# ## 判据
#
# 不是「`navigator.gpu` 存在」—— 那在所有组合下都为 true,`navigator.gpu`
# 只是个接口对象,不保证背后有硬件。要的是 `requestAdapter()` resolve 出
# 一个非 null adapter,再确认 `requestDevice()` 也能拿到 device。
#
# ## 浏览器纪律
#
# 每组 flag 起一个**独立的无头实例**(`--headless=new`,没有窗口、没有
# tab bar),跑完即弃。用户的 Chrome 完全不碰。
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRATCH="/Users/sqs/.hermes/cache/scratch/vcw-gpuprobe/sweep"
CHROME="/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
PY="/Users/sqs/.hermes/hermes-agent/venv/bin/python3.11"
BASE_PORT=9251

PORT="${VCW_SWEEP_PORT:-8879}"
mkdir -p "$SCRATCH"

# 起一个静态服务器伺服本目录。`navigator.gpu` 只在 secure context
# 里暴露,而通过 CDP `/json/new` 开的 `about:blank` 继承的是 opaque
# origin —— 不是 secure context。`http://127.0.0.1:PORT` 按 W3C
# secure-contexts 规范算 potentially trustworthy,所以 localhost
# 就够,不需要 HTTPS。
(cd "$SELF_DIR" && exec python3 -m http.server "$PORT" --bind 127.0.0.1) \
  > "${SCRATCH}/server.log" 2>&1 &
SERVE_PID=$!
for _ in $(seq 1 20); do
  curl -s --noproxy '*' "http://127.0.0.1:${PORT}/blank.html" -o /dev/null && break
  sleep 0.5
done
if ! curl -s --noproxy '*' "http://127.0.0.1:${PORT}/blank.html" -o /dev/null; then
  echo "FATAL: static server for :${PORT} never came up" >&2
  kill -9 "$SERVE_PID" 2>/dev/null
  exit 3
fi
echo "static server on :${PORT} (pid=${SERVE_PID}) serving ${SELF_DIR}"
echo

trap 'kill -9 "$SERVE_PID" 2>/dev/null' EXIT

# 每组 = 一个名字 + 一组**在 BASE 之上追加**的 flag。
# BASE 是所有组共用的最小集合:无头、CDP、不节流。
# 它**不含**任何 GPU 相关的 flag —— 那正是本脚本要测的变量。
COMMON=(
  --headless=new
  --no-sandbox
  --no-first-run
  --no-default-browser-check
  --disable-component-update
  --disable-extensions
  --disable-background-networking
  --disable-breakpad
  --metrics-recording-only
  --mute-audio
  --hide-scrollbars
  --window-size=800,600
)

declare -a NAMES=(
  "baseline-no-gpu-flags"
  "enable-gpu-only"
  "use-angle-metal"
  "enable-gpu+use-angle-metal"
  "enable-gpu+use-angle-metal+enable-features"
  "use-angle-swiftshader+enable-unsafe-swiftshader"
  "enable-gpu+use-angle-swiftshader+enable-unsafe-swiftshader"
  "disable-gpu-NEGATIVE-control"
)

declare -a EXTRA=(
  ""
  "--enable-gpu"
  "--use-angle=metal"
  "--enable-gpu --use-angle=metal"
  "--enable-gpu --use-angle=metal --enable-features=Vulkan,WebGPU"
  "--use-angle=swiftshader --enable-unsafe-swiftshader"
  "--enable-gpu --use-angle=swiftshader --enable-unsafe-swiftshader"
  "--disable-gpu"
)

for i in "${!NAMES[@]}"; do
  name="${NAMES[$i]}"
  extra="${EXTRA[$i]}"
  port=$((BASE_PORT + i))
  profile="${SCRATCH}/p${i}"
  rm -rf "$profile"

  # shellcheck disable=SC2086
  nohup "$CHROME" "${COMMON[@]}" ${extra} \
    --remote-debugging-port="${port}" --remote-allow-origins='*' \
    --user-data-dir="${profile}" \
    --disable-background-timer-throttling --disable-renderer-backgrounding \
    about:blank > "${SCRATCH}/${name}.log" 2>&1 &
  pid=$!

  for _ in $(seq 1 25); do
    curl -s --noproxy '*' "http://127.0.0.1:${port}/json/version" >/dev/null 2>&1 && break
    sleep 1
  done

  if ! curl -s --noproxy '*' "http://127.0.0.1:${port}/json/version" >/dev/null 2>&1; then
    printf '%-52s  CDP-NOT-UP\n' "$name"
    kill -9 "$pid" 2>/dev/null
    continue
  fi

  VCW_CDP="http://127.0.0.1:${port}" "$PY" "${SELF_DIR}/flag_probe.py" 2>&1 | tr '\n' ' '
  printf '   [flags: %s]\n' "${extra:-<none>}"

  kill -9 "$pid" 2>/dev/null
  pkill -9 -f "remote-debugging-port=${port}" 2>/dev/null
  sleep 1
done
