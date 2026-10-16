"""8 路并行验收的共用库。

设计约束（全部来自既有踩坑记录，worker 不要重犯）：

* 每个 worker 独占一个 Chrome 端口和 user-data-dir。并发 worker 会互相
  杀死对方的 debug 端口和临时 profile，表现出来像「应用神秘地坏了」。
* 静态服务器 8877 由持有者负责，worker 只读它。8877 不通时先 curl
  确认再报告 —— 那是 harness 问题，不是 app 的缺陷。
* 验收只能是**感知检查**。stddev / 三角形数 / console 无报错对
  「渲染出来的东西是不是人看的东西」毫无判断力。
* 帧预算按**帧数**算，不按墙钟算。SwiftShader 下 1-5 fps，
  30 秒墙钟只有 12 秒模拟时间。
* 绝不可加 --disable-gpu：会让 WebGPU 的 requestAdapter() 返回 null，
  失败原因看起来像渲染层坏了。
"""

from __future__ import annotations

import asyncio
import base64
import json
import math
import os
import shutil
import struct
import subprocess
import sys
import time
import urllib.request
import zlib
from typing import Any, Optional

import websockets

BASE_URL = "http://127.0.0.1:8877/index.html"
CHROME = (
    "/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/"
    "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
)
SCRATCH = "/Users/sqs/.hermes/cache/scratch/vcw-v8"
SHOTS = f"{SCRATCH}/shots"
RESULTS = f"{SCRATCH}/results"


class Verdict:
    """一条验收判定。ok 三态：True 通过 / False 失败 / None 未能判定。"""

    def __init__(self, name: str, ok: bool | None, note: str) -> None:
        self.name = name
        self.ok = ok
        self.note = note

    def line(self) -> str:
        tag = "ok  " if self.ok is True else ("SKIP" if self.ok is None else "FAIL")
        return f"{tag}  {self.name:28} {self.note}"

    def as_dict(self) -> dict:
        return {"name": self.name, "ok": self.ok, "note": self.note}


def emit(results: list[Verdict], worker: str) -> None:
    os.makedirs(RESULTS, exist_ok=True)
    for v in results:
        print(v.line(), flush=True)
    passed = sum(1 for v in results if v.ok is True)
    failed = sum(1 for v in results if v.ok is False)
    skipped = sum(1 for v in results if v.ok is None)
    print(f"# {worker}: {passed} pass, {failed} fail, {skipped} skip", flush=True)
    with open(f"{RESULTS}/{worker}.json", "w") as fh:
        json.dump(
            {"worker": worker, "results": [v.as_dict() for v in results]},
            fh,
            ensure_ascii=False,
            indent=2,
        )


def preflight(worker: str) -> None:
    """静态服务器必须先在。挂了就报 harness failure，不要包装成 app 缺陷。"""
    try:
        with urllib.request.urlopen(BASE_URL, timeout=6) as resp:
            if resp.status != 200:
                raise RuntimeError(f"HTTP {resp.status}")
    except Exception as exc:  # noqa: BLE001
        fail_harness(worker, f"static server 8877 unreachable: {exc}")


def fail_harness(worker: str, why: str) -> None:
    """harness 自身挂了就如实说是 harness。"""
    print(f"FAIL  {'harness':28} {why}", flush=True)
    print(f"# {worker} HARNESS FAILURE: {why}", flush=True)
    os.makedirs(RESULTS, exist_ok=True)
    with open(f"{RESULTS}/{worker}.json", "w") as fh:
        json.dump(
            {
                "worker": worker,
                "harness_error": why,
                "results": [{"name": "harness", "ok": False, "note": why}],
            },
            fh,
            ensure_ascii=False,
            indent=2,
        )
    sys.exit(1)


class Session:
    """一个 worker 独占的 Chrome：自己的端口 + 自己的 profile。"""

    def __init__(self, worker: str, port: int, res: float = 1.0) -> None:
        self.worker = worker
        self.port = port
        self.profile = f"{SCRATCH}/{worker}"
        self.url = f"{BASE_URL}?res={res}&fresh={worker}"
        self.proc: Optional[subprocess.Popen] = None
        self.ws: Optional[Any] = None
        self._n = 0

    # ---------------------------------------------------------- 生命周期
    def launch(self) -> None:
        os.makedirs(self.profile, exist_ok=True)
        self.proc = subprocess.Popen(
            [
                CHROME,
                "--headless=new",
                "--no-sandbox",
                "--use-gl=angle",
                "--use-angle=swiftshader",
                "--enable-unsafe-swiftshader",
                "--disable-features=Translate,BackForwardCache,"
                "CalculateNativeWinOcclusion",
                "--disable-component-update",
                "--disable-default-apps",
                "--disable-sync",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--metrics-recording-only",
                "--mute-audio",
                "--disable-breakpad",
                "--disable-background-networking",
                f"--remote-debugging-port={self.port}",
                "--remote-allow-origins=*",
                f"--user-data-dir={self.profile}",
                "--hide-scrollbars",
                "--window-size=1280,800",
                "--autoplay-policy=no-user-gesture-required",
                "--disable-background-timer-throttling",
                "--disable-renderer-backgrounding",
                "about:blank",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        for _ in range(60):
            time.sleep(1.0)
            out = subprocess.run(
                [
                    "curl", "-s", "--noproxy", "*", "--max-time", "3",
                    f"http://127.0.0.1:{self.port}/json/version",
                ],
                capture_output=True,
                text=True,
            )
            if out.returncode == 0 and "webSocketDebuggerUrl" in out.stdout:
                return
        raise SystemExit(f"chrome {self.port} never came up")

    def _new_page_url(self) -> str:
        out = subprocess.run(
            [
                "curl", "-s", "--noproxy", "*", "-X", "PUT",
                f"http://127.0.0.1:{self.port}/json/new?about:blank",
            ],
            capture_output=True,
            text=True,
        )
        if '"webSocketDebuggerUrl"' in out.stdout:
            return json.loads(out.stdout)["webSocketDebuggerUrl"]
        out = subprocess.run(
            [
                "curl", "-s", "--noproxy", "*",
                f"http://127.0.0.1:{self.port}/json/new?about:blank",
            ],
            capture_output=True,
            text=True,
        )
        if '"webSocketDebuggerUrl"' in out.stdout:
            return json.loads(out.stdout)["webSocketDebuggerUrl"]
        # 退路：复用已有的第一个 page target。
        listing = subprocess.run(
            ["curl", "-s", "--noproxy", "*", f"http://127.0.0.1:{self.port}/json/list"],
            capture_output=True,
            text=True,
        ).stdout
        pages = [t for t in json.loads(listing) if t.get("type") == "page"]
        if not pages:
            raise SystemExit("no page target available")
        pages.sort(key=lambda t: t.get("id", ""), reverse=True)
        return pages[0]["webSocketDebuggerUrl"]

    def shutdown(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        shutil.rmtree(self.profile, ignore_errors=True)

    # ------------------------------------------------------------- CDP
    async def cmd(self, method: str, params: Optional[dict] = None,
                  **kwargs) -> dict:
        """发一条 CDP 命令。

        **两种调用约定都接受**：`cmd(m, {...})` 和 `cmd(m, key=val)`。
        仓库里既有的 `tools/sweep/sweep_lib.py:53` 用的是**位置 dict**，
        新的 worker 习惯写关键字参数；只支持其中一种会让另一半的调用
        在运行时炸 `TypeError` —— 而那时页面已经起来了，看起来像应用
        挂了，实际是 harness 的签名不兼容。
        """
        assert self.ws is not None, "Session not connected"
        merged: dict = {}
        if params is not None:
            merged.update(params)
        merged.update(kwargs)
        self._n += 1
        mid = self._n
        await self.ws.send(
            json.dumps({"id": mid, "method": method, "params": merged})
        )
        while True:
            msg = json.loads(await self.ws.recv())
            if msg.get("id") == mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    async def ev(self, expr: str):
        r = await self.cmd(
            "Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True
        )
        if r.get("exceptionDetails"):
            return None
        return r.get("result", {}).get("value")

    async def boot(self, min_frames: int = 25, max_seconds: int = 150) -> None:
        await self.cmd("Runtime.enable")
        await self.cmd("Page.enable")
        await self.cmd("Network.enable")
        await self.cmd("Network.setCacheDisabled", {"cacheDisabled": True})
        await self.cmd("Emulation.setFocusEmulationEnabled", {"enabled": True})
        await self.cmd("Page.bringToFront")
        await self.cmd("Page.navigate", {"url": self.url})
        for _ in range(max_seconds):
            await asyncio.sleep(1.0)
            n = await self.ev("(() => (window.__vcw||{}).frames || 0)()")
            if isinstance(n, int) and n > min_frames:
                return
        raise SystemExit("page never booted")

    # ------------------------------------------------------------ 输入
    async def key(self, code: str, down: bool = True) -> None:
        await self.cmd(
            "Input.dispatchKeyEvent",
            type="keyDown" if down else "keyUp",
            code=code,
            key=code,
            windowsVirtualKeyCode=0,
        )

    async def tap(self, code: str, hold_ms: int = 90) -> None:
        await self.key(code, True)
        await asyncio.sleep(hold_ms / 1000.0)
        await self.key(code, False)

    async def click(self, x: int, y: int) -> None:
        for kind in ("mousePressed", "mouseReleased"):
            await self.cmd(
                "Input.dispatchMouseEvent",
                type=kind, x=x, y=y, button="left", clickCount=1,
            )
            await asyncio.sleep(0.05)

    async def mouse_move(self, x: int, y: int) -> None:
        await self.cmd(
            "Input.dispatchMouseEvent",
            type="mouseMoved", x=x, y=y, button="none", buttons=0,
        )

    async def shot(self) -> bytes:
        r = await self.cmd("Page.captureScreenshot", format="png")
        return base64.b64decode(r["data"])

    async def st(self) -> dict:
        v = await self.ev(
            "(() => { const d = window.__VCW_DEBUG__ || {};"
            " const c = window.__vcw || {};"
            " return {frames: c.frames, x: c.playerX, z: c.playerZ, y: c.playerY,"
            " yaw: c.playerYaw, cyaw: c.cameraYaw, hp: c.playerHealth,"
            " veh: c.playerVehicle, gait: c.gaitAmount, grounded: c.grounded,"
            " vy: c.vy, vel: c.vel, inside: c.playerInsideCollider,"
            " shapes: c.collisionShapes, cb: c.charBox, mode: c.cameraMode,"
            " respawn: c.respawn, safe: c.safe, camDist: c.cameraDist,"
            " cdistT: c.cameraDistTarget, coccl: c.camOccluded,"
            " asx: d.afterStatic, avx: d.afterVertical, adx: d.afterDynamic,"
            " lim: d.limbState, gpu: d.gpu, sm: d.sceneMeshes,"
            " ns: d.nearShapes, vis: d.charVisible, tris: c.tris,"
            " cw: d.canvasWidth, ch: d.canvasHeight,"
            " psx: d.playerScreenX, psy: d.playerScreenY,"
            " pct: d.playerScreenPct}; })()"
        )
        return v or {}

    # -------------------------------------------------------- 按帧等待
    async def frames(self) -> int:
        v = await self.ev("(() => (window.__vcw||{}).frames || 0)()")
        return v if isinstance(v, int) else 0

    async def wait_frames(self, budget: int, poll: float = 0.4) -> int:
        """推进 budget 帧，返回实际用掉的帧数。"""
        start = await self.frames()
        target = start + budget
        while (await self.frames()) < target:
            await asyncio.sleep(poll)
        return await self.frames() - start

    async def wait_until(self, cond_expr: str, budget: int,
                         poll: float = 0.5) -> tuple[bool, int]:
        """按帧等条件。返回 (是否满足, 用掉的帧数)。"""
        start = await self.frames()
        for _ in range(budget):
            if await self.ev(f"(() => {{ try {{ return !!({cond_expr}); }}"
                             f" catch (e) {{ return false; }} }})()"):
                return True, await self.frames() - start
            await asyncio.sleep(poll)
        return False, await self.frames() - start

    # -------------------------------------------------------- 上下文管理
    async def __aenter__(self) -> Session:
        preflight(self.worker)
        self.launch()
        url = self._new_page_url()
        self.ws = await websockets.connect(url, max_size=128 * 1024 * 1024)
        return self

    async def __aexit__(self, *_exc) -> None:
        if self.ws is not None:
            try:
                await self.ws.close()
            except Exception:  # noqa: BLE001
                pass
        self.shutdown()


# ---------------------------------------------------------------- PNG 解码
def decode_png(data: bytes) -> tuple[int, int, list]:
    """返回 (width, height, [luma,...])。纯 stdlib。"""
    pos = 8
    w = h = 0
    nch = 3
    idat = b""
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        ctype = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if ctype == b"IHDR":
            w, h, depth, color = struct.unpack(">IIBB", body[:10])
            assert depth == 8 and color in (2, 6), (depth, color)
            nch = 3 if color == 2 else 4
        elif ctype == b"IDAT":
            idat += body
        elif ctype == b"IEND":
            break
        pos += 12 + length
    assert w and h, "bad IHDR"
    raw = zlib.decompress(idat)
    stride = w * nch
    out: list = []
    prev = bytearray(stride)
    p = 0
    for _ in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1 : p + 1 + stride])
        p += 1 + stride
        if f == 1:
            for i in range(nch, stride):
                line[i] = (line[i] + line[i - nch]) & 0xFF
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(stride):
                a = line[i - nch] if i >= nch else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif f == 4:
            for i in range(stride):
                a = line[i - nch] if i >= nch else 0
                c = prev[i - nch] if i >= nch else 0
                b = prev[i]
                pp = a + b - c
                pa, pb, pc = abs(pp - a), abs(pp - b), abs(pp - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        for x in range(w):
            i = x * nch
            out.append(
                0.2126 * line[i] + 0.7152 * line[i + 1] + 0.0722 * line[i + 2]
            )
        prev = line
    return w, h, out


def box_stats(w: int, h: int, luma: list, box) -> tuple[float, float, int]:
    """角色框内的 (min, max, 非黑像素数)。box = [x0,y0,x1,y1,_]。"""
    x0, y0, x1, y1 = box[0], box[1], box[2], box[3]
    x0, y0 = max(0, int(x0)), max(0, int(y0))
    x1, y1 = min(w, int(x1)), min(h, int(y1))
    if x1 <= x0 or y1 <= y0:
        return 0.0, 0.0, 0
    lo, hi, n = 255.0, 0.0, 0
    for y in range(y0, y1):
        base = y * w
        for x in range(x0, x1):
            v = luma[base + x]
            if v < lo:
                lo = v
            if v > hi:
                hi = v
            if v > 4.0:
                n += 1
    return lo, hi, n


def diff_pixels(a: list, b: list, tol: int = 10) -> tuple[int, float, float]:
    """逐像素差：返回 (>tol 的像素数, 占比%, 最大差)。"""
    if len(a) != len(b):
        return 0, 0.0, 0.0
    over = 0
    mx = 0.0
    for i in range(len(a)):
        d = abs(a[i] - b[i])
        if d > mx:
            mx = d
        if d > tol:
            over += 1
    return over, 100.0 * over / len(a), mx


def save_shot(worker: str, tag: str, data: bytes) -> str:
    os.makedirs(SHOTS, exist_ok=True)
    path = f"{SHOTS}/{worker}_{tag}.png"
    with open(path, "wb") as fh:
        fh.write(data)
    return path


def dist(ax: float, az: float, bx: float, bz: float) -> float:
    return math.hypot(ax - bx, az - bz)
