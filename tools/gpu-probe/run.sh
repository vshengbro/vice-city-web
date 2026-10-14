#!/usr/bin/env bash
# WebGPU 探针的浏览器启动器。
#
# ## 浏览器纪律(用户的硬规则,不要绕过)
#
# 「打开浏览器禁止使用新窗口,应该复用最开始的窗口去新建 tab」。
# 这里用的是**独立的 Chrome for Testing 二进制**
# (`~/.agent-browser/browsers/...`),不是用户的 `/Applications/Google Chrome.app`。
# 它跑在 `--headless=new` 下、没有窗口、没有 tab bar,每个探针一个用完即弃的实例 ——
# 这正是项目里 `tools/verify-serial.sh` 既有的做法,也是唯一能在无头下
# 拿到 WebGPU adapter 的方式。**绝不要**把这里的行为套到用户自己的
# Chrome 上:那个进程(pid 709 之类)在跑游戏,碰它就是抢窗口。
#
# 如果确实要给人看结果,用有头模式开**一个**窗口 + 复用 tab,而不是
# 反复 `open -na "Google Chrome"`。
#
# ## flag 逐条说明
#
# 全部 8 组 flag 实测过(见 `flag-sweep.sh` 的输出与
# `docs/webgpu-feasibility-report.md` §2)。结论只有一条:
#
#   **`--use-angle=swiftshader` 会让 WebGPU 的 `requestAdapter()`
#   返回 null。**
#
# 也就是说, `tools/verify-serial.sh` 现在给 6 个 WebGL 探针用的那组
# flag (`--use-gl=angle --use-angle=swiftshader --enable-unsafe-swiftshader`)
# **拿不到 WebGPU adapter**。实测:该组下 `euv_probe()` 返回 `false`、
# `WebGpuRenderer::init` 报 `WEBGPU_ADAPTER_UNAVAILABLE`,而页面、
# `navigator.gpu`、wasm 全都正常 —— 失败点只有 adapter 一个。
#
# 反过来,**不写任何 GPU 相关 flag 就是最好的**(第 1 组实测
# `vendor=apple / metal-3`)。所以这里刻意不写 `--use-gl=*` /
# `--use-angle=*` / `--enable-gpu`;`--enable-gpu` 无害但不必要,
# 加它只会多一个将来会变语义的变量。
#
# `--disable-gpu` 同样致命(实测 `ADAPTER_NULL`),与 docs 里记的一致。
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRATCH="/Users/sqs/.hermes/cache/scratch/vcw-gpuprobe"
PORT="${VCW_PROBE_PORT:-9241}"
CHROME="/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
PY="/Users/sqs/.hermes/hermes-agent/venv/bin/python3.11"

mkdir -p "$SCRATCH"

if [[ ! -x "$CHROME" ]]; then
  echo "FATAL: Chrome for Testing not found at $CHROME" >&2
  echo "       (see docs/webgpu-feasibility-report.md §2 for why the user's Chrome is not used)" >&2
  exit 2
fi

pkill -9 -f "remote-debugging-port=${PORT}" 2>/dev/null
sleep 2
rm -rf "${SCRATCH}/profile"

# ⚠️ 这里**没有** `--use-angle=swiftshader`,也没有 `--disable-gpu`。
# 两者各自都会让 `requestAdapter()` 返回 null,而失败的表现是
# 「wasm 跑完了、报告也出来了、只有 WebGPU 那侧是空的」——
# 看起来像渲染层的问题,实际是 flag 的问题。
nohup "$CHROME" \
  --headless=new --no-sandbox \
  --disable-component-update --disable-default-apps --disable-sync \
  --no-first-run --no-default-browser-check --disable-extensions \
  --metrics-recording-only --mute-audio --disable-breakpad \
  --disable-background-networking \
  --remote-debugging-port="${PORT}" --remote-allow-origins='*' \
  --user-data-dir="${SCRATCH}/profile" \
  --hide-scrollbars --window-size=800,600 \
  --disable-background-timer-throttling \
  --disable-renderer-backgrounding \
  about:blank > "${SCRATCH}/chrome.log" 2>&1 &
CHROME_PID=$!

# 不盲等:轮询 `/json/version` 直到 CDP 起来或超时。
for _ in $(seq 1 30); do
  if curl -s --noproxy '*' "http://127.0.0.1:${PORT}/json/version" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! curl -s --noproxy '*' "http://127.0.0.1:${PORT}/json/version" >/dev/null 2>&1; then
  echo "FATAL: CDP never came up on :${PORT}. chrome.log:" >&2
  tail -20 "${SCRATCH}/chrome.log" >&2
  kill -9 "$CHROME_PID" 2>/dev/null
  exit 3
fi

echo "chrome pid=$CHROME_PID  cdp=:${PORT}  user_data=${SCRATCH}/profile"

# 起一个静态服务器伺服 tools/gpu-probe/。用 8878 而不是复用 8877 ——
# 8877 上跑的是主游戏的服务器,探针必须有自己的根目录。
SERVE_PID=""
if ! curl -s --noproxy '*' "http://127.0.0.1:8878/index.html" -o /dev/null; then
  (cd "$ROOT" && exec python3 -m http.server 8878 --bind 127.0.0.1) \
    > "${SCRATCH}/server.log" 2>&1 &
  SERVE_PID=$!
  for _ in $(seq 1 20); do
    if curl -s --noproxy '*' "http://127.0.0.1:8878/index.html" -o /dev/null; then
      break
    fi
    sleep 0.5
  done
fi
echo "static server on :8878 (pid=${SERVE_PID:-reused})"

VCW_CDP="http://127.0.0.1:${PORT}" "$PY" "${ROOT}/probe.py" "$@"
RC=$?

# 收尾:探针是「用完即弃」的,不要留进程。静态服务器是探针自己起的,
# 所以也一起收 —— 8877 上主游戏的服务器不在这个脚本管辖内。
kill -9 "$CHROME_PID" 2>/dev/null
[[ -n "$SERVE_PID" ]] && kill -9 "$SERVE_PID" 2>/dev/null
pkill -9 -f "remote-debugging-port=${PORT}" 2>/dev/null

exit $RC
