"""跑 WebGPU 探针并把结果读回来。

探针页面(`index.html`)加载 `pkg/gpu_probe.js`,wasm 跑完后把结论写进
`window.__probe`。这个脚本只做三件事:开一个新 tab、轮询等 `__probe`
出现、把 JSON 打出来。

## 浏览器纪律

**不开新窗口。** 用 `PUT /json/new` 在**已经**跑着的那个 Chrome 实例里
新建一个 tab,而不是启动一个新实例、更不是 `open -na` 新开一个窗口。
跑完关掉这个 tab。

注意 CDP 的历史坑:`/json/new` 在较新的 Chrome 上只接受 `PUT`,
`GET` 返回 405。这一条不踩就会以「探针没跑」的形式失败。

## 超时

wasm 要 init 一遍 WebGPU device(走真实的 Metal 栈)+ 编译 WGSL +
画 120 帧 + `mapAsync` 读回,慢的机器上给到 180 s 仍然不算保守。
"""

import asyncio
import base64
import json
import os
import sys

import websockets

CDP = os.environ.get("VCW_CDP", "http://127.0.0.1:9241")
URL = "http://127.0.0.1:8878/index.html"
# `?cachebust` 是必须的:wasm-bindgen 的 glue 是 ES module,浏览器会按
# URL 缓存它,而 wasm 二进制换了但文件名没变 —— 上一轮的探针结果会以
# 「同样的输入算出同样的数」的形式骗过验收。
URL = f"{URL}?cb={os.getpid()}"


class Driver:
    def __init__(self, ws):
        self.ws = ws
        self.mid = 0

    async def cmd(self, method: str, params: dict | None = None) -> dict:
        self.mid += 1
        await self.ws.send(json.dumps(
            {"id": self.mid, "method": method, "params": params or {}}))
        while True:
            msg = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=300))
            if msg.get("id") == self.mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    async def ev(self, expr: str):
        r = await self.cmd("Runtime.evaluate", {
            "expression": expr, "returnByValue": True, "awaitPromise": True})
        res = r.get("result", {})
        if res.get("subtype") == "error":
            return None
        return res.get("value")


def cdp_list(path: str) -> list[dict]:
    """`GET /json/list`。用 curl 而不是 urllib 是为了绕开这台机器上的
    proxy 环境变量(本机直连 127.0.0.1,走 proxy 必然失败)。"""
    import subprocess
    out = subprocess.run(
        ["curl", "-s", "--noproxy", "*", f"{CDP}{path}"],
        capture_output=True, text=True, timeout=30)
    if not out.stdout.strip():
        return []
    return json.loads(out.stdout)


def cdp_new_tab(url: str) -> str | None:
    """在**已有**实例里新建一个 tab,返回它的 webSocketDebuggerUrl。

    `PUT` 是必须的:Chrome 111+ 把 `/json/new` 从 GET 改成了 PUT。
    """
    import subprocess
    out = subprocess.run(
        ["curl", "-s", "--noproxy", "*", "-X", "PUT", f"{CDP}/json/new?{url}"],
        capture_output=True, text=True, timeout=30)
    try:
        data = json.loads(out.stdout)
    except json.JSONDecodeError:
        return None
    ws = data.get("webSocketDebuggerUrl")
    return ws if isinstance(ws, str) else None


def cdp_close_tab(target_id: str) -> None:
    import subprocess
    subprocess.run(
        ["curl", "-s", "--noproxy", "*", f"{CDP}/json/close/{target_id}"],
        capture_output=True, text=True, timeout=30)


async def main() -> int:
    ws_url = cdp_new_tab("about:blank")
    if not ws_url:
        print("FATAL: could not open a tab via PUT /json/new", file=sys.stderr)
        for entry in cdp_list("/json/list"):
            print(f"  existing: {entry.get('type')} {entry.get('url')}",
                  file=sys.stderr)
        return 2

    target_id = None
    for entry in cdp_list("/json/list"):
        if entry.get("webSocketDebuggerUrl") == ws_url:
            target_id = entry.get("id")
            break

    data: dict = {}
    report = None
    console: list[str] = []
    shots: list[str] = []
    # 对照组独立于 `report` 存放,最后再合并进 `data`。之前直接写
    # `data[...]` 而 `data` 在后面被 `json.loads(report)` 整个重新赋值,
    # 对照组就这么被悄悄丢掉了 —— 报告里看不到 `raw_webgpu`,而缺失
    # 看起来就像「原生 WebGPU 也不行」。
    raw_webgpu: dict = {}
    raw_bisect: dict = {}
    try:
        async with websockets.connect(ws_url, max_size=32 * 1024 * 1024) as ws:
            d = Driver(ws)
            await d.cmd("Runtime.enable")
            await d.cmd("Page.enable")
            await d.cmd("Log.enable")
            await d.cmd("Network.setCacheDisabled", {"cacheDisabled": True})
            await d.cmd("Page.navigate", {"url": URL})

            # 轮询到结果出现或超时。**不**用固定 sleep:wasm 编译 +
            # device init 的耗时随机器负载差几倍,睡死一个数就是
            # 随机地「有时过有时不过」。
            deadline = asyncio.get_event_loop().time() + 180
            while asyncio.get_event_loop().time() < deadline:
                await asyncio.sleep(1.0)
                report = await d.ev(
                    "(() => { const p = window.__probe;"
                    " return p ? JSON.stringify(p) : null; })()")
                if report:
                    break
                # 顺手把控制台错误攒起来 —— 探针失败时它们是唯一线索。
                got = await d.ev(
                    "(() => (window.__probeErrs || []).join(' | ') || null)()")
                if got:
                    console.append(str(got))

            # ---- 截图 ----------------------------------------------------
            # 截图是**独立于 `window.__probe` 的第二条证据链**。读回
            # API 坏掉时(实测 `read_buffer` 拿到全 0),报告会说
            # `triangle_drawn: false`;但画布上到底有没有三角形,
            # 只有截图说了算。把两者分开记录,才能区分
            # 「WebGPU 画不出来」和「euv 的读回接口坏了」——
            # 这两件事对重构的含义完全相反。
            #
            # 同时读 `window.__raw`(原生 WebGPU,不过 euv)。它是
            # 判别「环境问题」与「引擎问题」的那条对照组。
            #
            # `raw.js` 用的是经典 `<script src>`,**不**是 module,
            # 所以它的顶层 `await` 不合法 —— 它靠 `window.__raw`
            # 这个 Promise 本身挂上去,`await window.__raw` 在这里
            # 才真正解包。等待要带超时:这张卡上设备 init 要好几秒。
            if report:
                shots = await capture(d)
                raw = None
                for _ in range(60):
                    # `window.__raw` 是一个 Promise;`await` 它会等到设备
                    # 真正 init 完。`__rawSeen` 用来区分「脚本压根没跑」
                    # 与「跑了但还没 resolve」—— 两者都要报出来,否则
                    # 对照组缺失会看起来像「原生 WebGPU 也不行」。
                    raw = await d.ev(
                        "(async () => { try {"
                        " if (typeof window.__raw === 'undefined')"
                        "   return JSON.stringify({error: 'raw.js never defined __raw'});"
                        " const v = await window.__raw;"
                        " return JSON.stringify(v || {error: 'resolved undefined'}); }"
                        " catch (e) { return JSON.stringify({error: String(e)}); } })()")
                    if isinstance(raw, str) and "resolved undefined" not in raw \
                            and "never defined" not in raw:
                        break
                    await asyncio.sleep(0.5)
                if isinstance(raw, str):
                    try:
                        raw_webgpu = json.loads(raw)
                    except json.JSONDecodeError:
                        raw_webgpu = {"error": "unparseable", "raw": raw[:400]}
                else:
                    raw_webgpu = {"error": "window.__raw never resolved"}

                # `window.__bisect` 同理:它把 euv 之前的那一段
                # (交换链 / 离屏 / copyTextureToBuffer)逐步拆开跑,
                # 用来定位 euv 的读回到底坏在哪一步。
                bisect = None
                for _ in range(60):
                    bisect = await d.ev(
                        "(async () => { try {"
                        " if (typeof window.__bisect === 'undefined')"
                        "   return JSON.stringify({error: 'bisect.js never defined __bisect'});"
                        " const v = await window.__bisect;"
                        " return JSON.stringify(v || {error: 'resolved undefined'}); }"
                        " catch (e) { return JSON.stringify({error: String(e)}); } })()")
                    if isinstance(bisect, str) and "resolved undefined" not in bisect \
                            and "never defined" not in bisect:
                        break
                    await asyncio.sleep(0.5)
                if isinstance(bisect, str):
                    try:
                        raw_bisect = json.loads(bisect)
                    except json.JSONDecodeError:
                        raw_bisect = {"error": "unparseable", "raw": bisect[:400]}
                else:
                    raw_bisect = {"error": "window.__bisect never resolved"}
    finally:
        if target_id:
            cdp_close_tab(target_id)

    if not report:
        print("FAIL: window.__probe never appeared within 180 s", file=sys.stderr)
        for line in console:
            if line:
                print(f"  page: {line[:600]}", file=sys.stderr)
        return 1

    data = json.loads(report)
    data["raw_webgpu"] = raw_webgpu
    data["raw_bisect"] = raw_bisect
    for path in shots:
        print(f"screenshot: {path}")
    print(json.dumps(data, indent=2, ensure_ascii=False))
    print()
    ok = verdict(data)
    print("VERDICT:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "shots")


async def capture(d: Driver) -> list[str]:
    """给两张 canvas 各截一张图,存到 `shots/` 下。

    用 `Page.captureScreenshot` 的 `clip` 精确框住 canvas 的 CSS
    盒子,而不是整页 —— 整页截图里两张 256x256 的画布只占边角,
    人眼和阈值都判不准。
    """
    os.makedirs(OUT_DIR, exist_ok=True)
    written: list[str] = []
    boxes = await d.ev(
        "(() => { const r = {};"
        " for (const id of ['gpu-canvas', 'raw-canvas', 'gl-canvas', 'bisect-canvas']) {"
        "   const c = document.getElementById(id);"
        "   if (!c) continue;"
        "   const b = c.getBoundingClientRect();"
        "   r[id] = {x: b.x, y: b.y, w: b.width, h: b.height};"
        " } return JSON.stringify(r); })()")
    if not isinstance(boxes, str):
        return written
    for name, box in json.loads(boxes).items():
        r = await d.cmd("Page.captureScreenshot", {
            "format": "png",
            "clip": {"x": box["x"], "y": box["y"],
                     "width": box["w"], "height": box["h"], "scale": 1},
        })
        data = r.get("data")
        if not data:
            continue
        path = os.path.join(OUT_DIR, f"{name}.png")
        with open(path, "wb") as handle:
            handle.write(base64.b64decode(data))
        written.append(path)
    return written


def verdict(data: dict) -> bool:
    """判据:三个后端都真的画出了像素,且三角形覆盖率互相吻合。

    覆盖率不要求逐位相等 —— WebGL 与 WebGPU 的边缘抗锯齿策略不同,
    三角形边缘那一圈像素会差几个百分点。要求「都在 15%~45% 之间」
    才是有意义的判据:太小说明只画出了几个角,太大说明读回的是噪声。

    `raw_webgpu` 是**对照组**,不过 euv。它不参与 pass/fail,而是
    决定失败该怎么解释:euv 侧全 0 而 raw 侧有数,问题在引擎;两边
    都全 0,问题在环境。
    """
    ok = True
    if "error" in data:
        print(f"  top-level error: {data['error']}")
        return False
    for key in ("webgpu", "webgl"):
        side = data.get(key) or {}
        if "error" in side:
            print(f"  {key}: ERROR {side['error']}")
            ok = False
            continue
        drawn = side.get("triangle_drawn")
        cov = side.get("coverage_pct", 0.0)
        drawn_ok = bool(drawn) and 15.0 <= cov <= 45.0
        print(f"  {key}: drawn={drawn} coverage={cov:.2f}% "
              f"non_clear={side.get('non_clear_pixels')}/"
              f"{side.get('total_pixels')} "
              f"mean_rgb={[round(v, 2) for v in (side.get('mean_rgb') or [])]} "
              f"distinct={side.get('distinct_colors')} "
              f"max_ch={side.get('max_channel')} "
              f"fps={side.get('fps'):.1f} "
              f"-> {'ok' if drawn_ok else 'SUSPECT'}")
        for e in side.get("errors") or []:
            print(f"      note: {e}")
        ok = ok and drawn_ok

    raw = data.get("raw_webgpu") or {}
    if raw:
        if "error" in raw:
            print(f"  raw (control): ERROR {raw['error']}")
        else:
            cov = raw.get("coverage_pct", 0.0)
            print(f"  raw (control, no euv): drawn={raw.get('triangle_drawn')} "
                  f"coverage={cov:.2f}% "
                  f"mean_rgb={[round(v, 2) for v in (raw.get('mean_rgb') or [])]} "
                  f"max_ch={raw.get('max_channel')} "
                  f"swapchain_format={raw.get('swapchain_format')}")
        euv_drew = bool((data.get("webgpu") or {}).get("triangle_drawn"))
        raw_drew = bool(raw.get("triangle_drawn"))
        if raw_drew and not euv_drew:
            print("  => raw WebGPU draws fine, euv's path does not:"
                  " the blocker is in euv-engine, NOT in this machine / these flags")
        elif not raw_drew and not euv_drew:
            print("  => neither raw nor euv WebGPU produced pixels:"
                  " the blocker is the environment (flags / headless), not euv-engine")
    return ok


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
