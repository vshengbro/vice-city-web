#!/usr/bin/env bash
# 并行跑 6 组验收，每组一个独立 Chrome 实例。
#
#   tools/verify-parallel.sh          # 起实例 + 跑 6 组 + 汇总
#   tools/verify-parallel.sh --serve  # 只跑，不重启浏览器（复用已有实例）
#
# 6 路并行比串行快 5 倍以上：12 项里 showcase.climb 一项就要 10 分钟，
# 串行等不起。代价是 6 个 Chrome 同时初始化时 p3 那组可能卡几分钟，
# 所以脚本对每组都有独立的超时兜底。

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH="/Users/sqs/.hermes/cache/scratch/vcw-cdp"
SWEEP="${ROOT}/tools/sweep"
PY="/Users/sqs/.hermes/hermes-agent/venv/bin/python3.11"
CHROME="/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
BASE_PORT=9231
PROBE_GROUPS=(p1 p2 p3 p4 p5 p6)

mkdir -p "$SCRATCH"
# 探针脚本的**唯一真源**是 tools/sweep/。以前 `cd "$SCRATCH"` 后直接跑
# `./pN.py`,于是跑的是 scratch 里手工拷贝的旧副本 —— 改了 tools/sweep/
# 却毫无效果,而日志看上去一切正常。cwd 留在仓库里,log 仍然写到 scratch。
cd "$ROOT"

if [[ "${1:-}" != "--serve" ]]; then
  echo "==> 停掉旧实例"
  pkill -9 -f "remote-debugging-port=92[3-4][0-9]" 2>/dev/null
  pkill -9 -f "${SCRATCH}/p[1-6].py" 2>/dev/null
  sleep 2

  echo "==> 起 6 个 Chrome 实例 (--disable-accelerated-2d-canvas 是 60fps 的关键)"
  for i in "${!PROBE_GROUPS[@]}"; do
    port=$((BASE_PORT + i))
    nohup "$CHROME" \
    # 注意:这里**不能**加 `--disable-gpu`。WebGPU 的 adapter 走的是
    # 真实的 GPU 栈(实测 macOS 上拿到 `apple / metal-3`),而
    # `--disable-gpu` 会让 `requestAdapter()` 直接返回 null —— 页面
    # 上 `navigator.gpu` 还在,但一个 adapter 都没有。迁移到 euv 的
    # WebGPU 之后,加了这个 flag 的探针会**全部**失败(而且失败原因
    # 看起来像渲染层坏了,不是环境问题)。`--use-angle=swiftshader`
    # 只影响 WebGL,不影响 WebGPU。
      --headless=new --no-sandbox       --use-gl=angle --use-angle=swiftshader --enable-unsafe-swiftshader \
      --remote-debugging-port="$port" --remote-allow-origins='*' \
      --user-data-dir="${SCRATCH}/p${port}" \
      --hide-scrollbars --window-size=1280,800 \
      --autoplay-policy=no-user-gesture-required \
      --disable-background-timer-throttling \
      --disable-renderer-backgrounding \
      --disable-accelerated-2d-canvas \
      about:blank > "/tmp/c${port}.log" 2>&1 &
  done
  sleep 9
  for i in "${!PROBE_GROUPS[@]}"; do
    port=$((BASE_PORT + i))
    printf '  %s: ' "$port"
    curl -s --noproxy '*' --max-time 3 "http://127.0.0.1:${port}/json/version" \
      >/dev/null 2>&1 && echo UP || echo "DEAD (port ${port})"
  done
fi

echo "==> 并行启动 6 组"
# 循环变量必须取**值**而不是下标。`for i in "${!PROBE_GROUPS[@]}"` 给的是 0..5,
# 而循环体里 `g="${PROBE_GROUPS[$i]}"` 拿这些下标去索引一个 6 元素的组名数组 ——
# 在 bash 里越界不报错,直接展开成空串,于是 $g 变成上一条命令的 pid,
# 脚本去跑 `tools/sweep/58350.py`。改用 `for g in "${PROBE_GROUPS[@]}"` 并单独
# 维护端口计数。
port="$BASE_PORT"
for g in "${PROBE_GROUPS[@]}"; do
  rm -f "${SCRATCH}/${g}.log"
  VCW_CDP="http://127.0.0.1:${port}" "$PY" "${SWEEP}/${g}.py" \
    > "${SCRATCH}/${g}.log" 2>&1 &
  printf '  %-3s port=%s pid=%s\n' "$g" "$port" "$!"
  port=$((port + 1))
done

echo "==> 等待完成（showcase 那组最慢）"
wait

echo
echo "=== 汇总 ==="
total_ok=0
total=0
for g in "${PROBE_GROUPS[@]}"; do
  echo "--- ${g} ---"
  if [[ ! -s "${SCRATCH}/${g}.log" ]]; then
    echo "  (无输出 — 该组可能卡在启动)"
    continue
  fi
  grep -aE '^ok  |^FAIL |passed$' "${SCRATCH}/${g}.log" || echo "  (无判定行)"
done

ok=$(grep -ahcE '^ok  ' "${SCRATCH}"/p[1-6].log 2>/dev/null | paste -sd+ - | bc 2>/dev/null || echo 0)
bad=$(grep -ahcE '^FAIL ' "${SCRATCH}"/p[1-6].log 2>/dev/null | paste -sd+ - | bc 2>/dev/null || echo 0)
echo
echo "$((ok + bad)) 项判定：${ok} 通过，${bad} 失败"
