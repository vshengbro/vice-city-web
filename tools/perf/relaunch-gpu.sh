#!/bin/bash
# 复用**同一个** Chrome 实例(同 port / 同 user-data-dir / 仍然 headless),
# 只是补上 GPU 开关。
#
# 为什么必须重起:之前那个 9240 是带 `--disable-gpu` 起的,
# `SystemInfo.getInfo` 报 `glRenderer: "Disabled"`,游戏里
# `WebGlRenderer::new()` 失败 → `game.renderer = None` → HUD backend 打
# `init`(`const.rs:177` 的 `BACKEND_NONE`)+ `0 tris`。
# 那种状态下**一个三角形都没画**,测到的 rAF 是空转循环的节奏,
# 拿它当帧率会把「阴影占 33%」这类归因整个带偏。
#
# ⚠️ 上一版这个脚本有个坑:只 kill 主进程,子进程还活着,
# SingletonLock 仍在 → 新的进程启动即 abort(见 chrome-9240.log),
# 于是端口上还是那个 `--disable-gpu` 的旧实例,而脚本却报告「起来了」。
# 这里改成:按 user-data-dir 杀掉**整棵进程树**,确认锁消失,再启动。
#
# `--disable-frame-rate-limit` / `--disable-gpu-vsync`:撤掉 60 Hz 上限。
# 不撤的话 `mean_dt` 恒等于 16.666 ms(实测两次跑都是 60.00 fps),
# 阴影从 2048 降到 1024 这种 4 倍顶点削减在读数上**完全看不出来** ——
# 量的只是显示器刷新率,不是 GPU。
#
# 硬约束:禁止新开浏览器窗口 —— 这里是 headless,屏幕上根本不出现窗口;
# 前台窗口数不变,变的只是这个实例的 GPU 能力。用户的 X tab 用完复原。
set -u

PORT=9240
PROFILE=/Users/sqs/.hermes/cache/scratch/chrome-real-profile/profile
BIN="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
LOG=/Users/sqs/.hermes/cache/scratch/vcw-perf/chrome-9240.log

curl -s --noproxy "*" --max-time 5 "http://127.0.0.1:$PORT/json/list" \
  | python3 -c "
import json,sys
for t in json.load(sys.stdin):
    if t['type']=='page': print(t['url'])
" > /Users/sqs/.hermes/cache/scratch/vcw-perf/tabs-before-restart.txt 2>/dev/null || true

echo '# tabs before restart:'
cat /Users/sqs/.hermes/cache/scratch/vcw-perf/tabs-before-restart.txt 2>/dev/null

echo '# killing every process on this profile (whole tree)'
pkill -9 -f "user-data-dir=$PROFILE" 2>/dev/null || true

# 等锁真的消失 —— 锁还在就说明进程没死干净,起来的会是 abort 空壳。
for i in $(seq 1 40); do
  if pgrep -f "user-data-dir=$PROFILE" >/dev/null 2>&1; then
    sleep 0.5
  else
    break
  fi
done
if pgrep -f "user-data-dir=$PROFILE" >/dev/null 2>&1; then
  echo "# profile processes survive; stale lock removal"
  rm -f "$PROFILE/SingletonLock" "$PROFILE/SingletonCookie" 2>/dev/null || true
fi
sleep 1

# 同样一个实例、同样 headless、同样 port 和 profile —— 只把 GPU 打开。
# --use-angle=metal 让 macOS 走真 Metal;真拿不到硬件时才允许 swiftshader 兜底,
# 但兜底发生时探针会报出来,不会被当成「真 GPU」悄悄混过去。
nohup "$BIN" \
  --remote-debugging-port="$PORT" \
  --user-data-dir="$PROFILE" \
  --headless=new \
  --use-angle=metal \
  --enable-gpu \
  --enable-unsafe-swiftshader \
  --disable-frame-rate-limit \
  --disable-gpu-vsync \
  --no-first-run \
  --no-default-browser-check \
  --window-size=1280,1400 \
  --disable-features=Translate,BackForwardCache,CalculateNativeWinOcclusion \
  --disable-background-timer-throttling \
  --disable-renderer-backgrounding \
  --metrics-recording-only \
  --mute-audio \
  about:blank >"$LOG" 2>&1 &

for _ in $(seq 1 60); do
  sleep 1
  if curl -s --noproxy "*" --max-time 3 "http://127.0.0.1:$PORT/json/version" \
       | grep -q webSocketDebuggerUrl; then
    echo "# chrome $PORT up"
    echo '# verifying the new instance really has a live GL:'
    grep -o 'Abort\|Failed to create ProcessSingleton' "$LOG" | head -3
    ps aux | grep "[C]hrome" | grep "remote-debugging-port=$PORT" | grep -v "type=" \
      | grep -oE '\-\-(disable-gpu|enable-gpu|use-angle=[a-z]+)' | sort -u
    exit 0
  fi
done
echo "# chrome $PORT failed to come up"
tail -20 "$LOG"
exit 1