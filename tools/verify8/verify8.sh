#!/usr/bin/env bash
# 并行跑 8 组只读验收，每组一个独占 Chrome。
#
#   tools/verify8/verify8.sh              # 跑全部 8 组
#   tools/verify8/verify8.sh w1 w5        # 只跑指定几组
#   tools/verify8/verify8.sh --serve      # 复用已在跑的实例（不清 Chrome）
#
# 静态服务器 8877 由持有者负责，本脚本**只检查、不启动、不杀**。
# 端口是硬约束：worker 1..8 -> CDP 9340..9347。
# 9231-9236 归 tools/verify-parallel.sh，9390 归修复 agent，都不许碰。

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
V8="${ROOT}/tools/verify8"
SCRATCH="/Users/sqs/.hermes/cache/scratch/vcw-v8"
PY="/Users/sqs/.hermes/hermes-agent/venv/bin/python3.11"
BASE_PORT=9340
BASE_URL="http://127.0.0.1:8877/index.html"

# worker 名 -> 端口偏移。**唯一真源**，worker 里的 PORT 常量必须与这里一致。
WORKERS=(w1_walk w2_visibility w3_pointerlock w4_keys w5_vehicle w6_combat w7_camera w8_world)

# ---- 可选范围过滤 ----
REUSE=0
if [[ "${1:-}" == "--serve" ]]; then
  REUSE=1
  shift
fi
if [[ $# -gt 0 ]]; then
  SELECTED=("$@")
else
  SELECTED=("${WORKERS[@]}")
fi

mkdir -p "$SCRATCH"
# cwd 必须留在仓库里：探针脚本的唯一真源是 tools/verify8/。以前 `cd` 到
# scratch 之后跑 ./pN.py，跑的是 scratch 里手工拷贝的旧副本 —— 改了
# tools/verify8/ 却毫无效果，而日志看上去一切正常。log 仍然写到 scratch。
cd "$ROOT"

# ---------------------------------------------------------------- 静态服务器
# **不启动它。** 8877 是别人的端口。挂了就如实报错退出。
if ! curl -s --noproxy '*' --max-time 5 "$BASE_URL" -o /dev/null; then
  echo "FAIL: 静态服务器 8877 不通（$BASE_URL）"
  echo "  本脚本不负责起它。请先由持有者在仓库根目录起静态服务器："
  echo "      (cd $ROOT && python3 -m http.server 8877)"
  echo "  端口 8877 / 9231-9236 / 9390 都不是本脚本的，脚本不会去 probe 或 kill。"
  exit 2
fi
echo "==> 8877 静态服务器可达"

# ---------------------------------------------------------------- 语法预检
# 语法错的 worker 不该占一个端口跑 5 分钟才崩在这里。
echo "==> 语法预检"
SYNTAX_BAD=0
for w in "${SELECTED[@]}"; do
  [[ -f "${V8}/${w}.py" ]] || { echo "  FAIL 缺少 ${w}.py"; SYNTAX_BAD=1; continue; }
  if "$PY" -c "import ast,sys; ast.parse(open(sys.argv[1]).read())" "${V8}/${w}.py" 2>/dev/null; then
    printf '  ok   %s\n' "$w"
  else
    printf '  FAIL %s 语法错误\n' "$w"
    "$PY" -c "import ast,sys; ast.parse(open(sys.argv[1]).read())" "${V8}/${w}.py" || true
    SYNTAX_BAD=1
  fi
done
echo "==> 静态预检（await 下标 / 自调用 / 裸格式化 / 落空分支）"
STATIC_BAD=0
for w in "${SELECTED[@]}"; do
  [[ -f "${V8}/${w}.py" ]] || continue
  out=$("$PY" "${V8}/_static_check.py" "${V8}/${w}.py" 2>&1) || STATIC_BAD=1
  # _path_check 补上 _static_check 抓不到的两类:
  #   1. SyntaxError（嵌套 def 里写 await，静态检查过了、import 时才炸）
  #   2. UnboundLocalError（if/elif 链有一支没 return / 没赋值）
  # 这两类都只在**单跑**时才暴露，跑一遍完整 8 路的代价太高。
  if [[ -n "$out" ]]; then
    printf '  WARN %s\n%s\n' "$w" "$out"
  else
    pout=$("$PY" "${V8}/_path_check.py" "${V8}/${w}.py" 2>&1) || STATIC_BAD=1
    if [[ -n "$pout" && "$pout" != "PATH CHECK: clean" ]]; then
      printf '  WARN %s (path)\n%s\n' "$w" "$pout"
    else
      printf '  ok   %s\n' "$w"
    fi
  fi
done
if [[ "$SYNTAX_BAD" != 0 ]]; then
  echo "  语法预检未通过,不开跑。"
  exit 3
fi

# ---------------------------------------------------------------- 清理旧实例
# 只杀**本脚本自己**用过的端口。模式是精确端口匹配，不会误伤
# 9231-9236（verify-parallel.sh）或 9390（修复 agent）。
if [[ "$REUSE" == 0 ]]; then
  echo "==> 清掉 9340-9347 上的残留实例"
  for p in $(seq "$BASE_PORT" 9347); do
    pkill -9 -f "remote-debugging-port=${p}" 2>/dev/null
  done
  pkill -9 -f "${SCRATCH}/w[1-8]_" 2>/dev/null
  sleep 1
fi

# ---------------------------------------------------------------- 并行启动
echo "==> 启动 ${#SELECTED[@]} 组"
PIDS=()
NAMES=()
port="$BASE_PORT"
for w in "${WORKERS[@]}"; do
  # 只启动被选中的；但端口按 WORKERS 的下标固定，不因筛选而漂移 ——
  # 否则 w5 单独跑的时候会占到 9340，和别人撞。
  keep=0
  for s in "${SELECTED[@]}"; do [[ "$w" == "$s" ]] && keep=1; done
  if [[ "$keep" == 1 ]]; then
    log="${SCRATCH}/${w}.log"
    rm -f "$log"
    # cwd = ROOT，跑的是仓库里那份脚本（见上面注释）。
    "$PY" "${V8}/${w}.py" > "$log" 2>&1 &
    PIDS+=("$!")
    NAMES+=("$w")
    printf '  %-14s port=%s pid=%s log=%s\n' "$w" "$port" "$!" "$log"
  fi
  port=$((port + 1))
done

# ---------------------------------------------------------------- 收尾
# worker 自己 terminate 自己的 Chrome（Session.shutdown 走 proc.terminate
# + rmtree profile）。这里再兜一次底：等所有 worker 退出后，只对
# 9340-9347 做精确匹配地补刀。宁可多杀一次同端口的残留，也不许留下
# 占端口的孤儿 —— 下一轮跑就会撞端口，表现出来像「应用神秘地坏了」。
cleanup() {
  echo
  echo "==> 收尾：清理 9340-9347"
  for p in $(seq "$BASE_PORT" 9347); do
    pkill -9 -f "remote-debugging-port=${p}" 2>/dev/null
  done
  for d in "$SCRATCH"/w[1-8]_*; do
    [[ -d "$d" ]] && rm -rf "$d" 2>/dev/null
  done
  echo "  9340-9347 已释放；9231-9236 / 9390 未触碰"
}
trap cleanup EXIT INT TERM

echo "==> 等待完成（w2 要起 3 个 Chrome，最慢）"
FAILED=0
for i in "${!PIDS[@]}"; do
  pid="${PIDS[$i]}"
  name="${NAMES[$i]}"
  if wait "$pid"; then
    printf '  %-14s exit=0\n' "$name"
  else
    rc=$?
    printf '  %-14s exit=%s\n' "$name" "$rc"
    # 退出码 1 = harness failure（fail_harness 会 exit 1），如实标出来。
    [[ "$rc" == 1 ]] && FAILED=$((FAILED + 1))
  fi
done

# ---------------------------------------------------------------- 汇总
RESULTS="/Users/sqs/.hermes/cache/scratch/vcw-v8/results"
echo
echo "=== 汇总 ==="
TOTAL_OK=0; TOTAL_BAD=0; TOTAL_SKIP=0; TOTAL_HARNESS=0
printf '%-14s %-6s %-6s %-6s %-6s %s\n' worker pass fail skip harness 说明
for w in "${SELECTED[@]}"; do
  f="${RESULTS}/${w}.json"
  if [[ ! -s "$f" ]]; then
    printf '%-14s %-6s %-6s %-6s %-6s %s\n' "$w" - - - - "无 results JSON（worker 没跑到 emit）"
    continue
  fi
  read -r p b s h note <<<"$(
    "$PY" - "$f" <<'PYEOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception as exc:
    print(f"- - - - 读不出 JSON: {exc}")
    raise SystemExit(0)
if d.get("harness_error"):
    print(f"- - - 1 HARNESS FAILURE: {d['harness_error'][:60]}")
    raise SystemExit(0)
p = sum(1 for r in d["results"] if r["ok"] is True)
b = sum(1 for r in d["results"] if r["ok"] is False)
s = sum(1 for r in d["results"] if r["ok"] is None)
h = sum(1 for r in d["results"] if r.get("name") == "harness")
print(f"{p} {b} {s} {h} ok/{b} 失败/{s} 未定")
PYEOF
)"
  printf '%-14s %-6s %-6s %-6s %-6s %s\n' "$w" "$p" "$b" "$s" "$h" "$note"
  [[ "$p" =~ ^[0-9]+$ ]] && TOTAL_OK=$((TOTAL_OK + p))
  [[ "$b" =~ ^[0-9]+$ ]] && TOTAL_BAD=$((TOTAL_BAD + b))
  [[ "$s" =~ ^[0-9]+$ ]] && TOTAL_SKIP=$((TOTAL_SKIP + s))
  [[ "$h" =~ ^[0-9]+$ ]] && TOTAL_HARNESS=$((TOTAL_HARNESS + h))
done

echo
echo "$((TOTAL_OK + TOTAL_BAD + TOTAL_SKIP)) 项判定：${TOTAL_OK} 通过，${TOTAL_BAD} 失败，${TOTAL_SKIP} 未能判定"
[[ "$TOTAL_HARNESS" -gt 0 || "$FAILED" -gt 0 ]] && \
  echo "注意：有 $((TOTAL_HARNESS + FAILED)) 处 harness failure（8877 不通 / Chrome 起不来 / CDP 断），不是应用缺陷"
echo "截图：${SCRATCH}/shots/   逐项明细：${SCRATCH}/results/"
echo
echo "提醒：这是感知检查，不是数字闸门。SKIP 与 FAIL 都要人看 ——"
echo "尤其是 w2/w7/w8 的截图，要 vision_analyze 出文字描述才算验过。"
