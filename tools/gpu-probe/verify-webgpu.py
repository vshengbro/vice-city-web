"""WebGPU 垂直切片验收:复用 gpu-probe 的浏览器纪律,读游戏自己的通道。

- 浏览器:`~/.agent-browser/browsers/` 的独立 Chrome for Testing,
  `--headless=new`、**无任何 GPU flag**(写了 `--disable-gpu` /
  `--use-angle=swiftshader` 的话 `requestAdapter()` 会返回 null)。
- 用 `PUT /json/new` 在**已经跑着的实例**里开 tab,不新开窗口。
- 判据:`#vcw-hud` 的三角形数非 0 + `window.__vcw` 的 frame 计数在涨
  + 截图。
"""

import asyncio
import json
import os
import subprocess
import sys
import time
import urllib.request

import websockets

CHROME = (
    "/Users/sqs/.agent-browser/browsers/chrome-154.0.8037.57/"
    "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
)
PORT = int(os.environ.get("VCW_PORT", "9241"))
SCRATCH = "/Users/sqs/.hermes/cache/scratch/vcw-webgpu-verify"
SHOTS = "/Users/sqs/code/vice-city-web/tools/gpu-probe/shots"


class Driver:
    def __init__(self, ws):
        self.ws = ws
        self.mid = 0
        self._console = []

    async def cmd(self, method, params=None):
        self.mid += 1
        await self.ws.send(
            json.dumps({"id": self.mid, "method": method, "params": params or {}})
        )
        while True:
            msg = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=300))
            if msg.get("id") == self.mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    async def pump(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            try:
                msg = json.loads(await asyncio.wait_for(self.ws.recv(), 0.4))
            except (asyncio.TimeoutError, TimeoutError):
                continue
            except Exception:
                break
            if msg.get("method") == "Runtime.consoleAPICalled":
                args = []
                for a in msg["params"].get("args", []):
                    args.append(str(a.get("value", a.get("description", ""))))
                self._console.append(args[0] if args else "")

    async def ev(self, expr):
        r = await self.cmd(
            "Runtime.evaluate",
            {"expression": expr, "returnByValue": True, "awaitPromise": True},
        )
        res = r.get("result", {})
        if res.get("subtype") == "error":
            return {"__error": res.get("description", "?")}
        return res.get("value")


def launch():
    os.makedirs(SCRATCH, exist_ok=True)
    subprocess.run(["pkill", "-9", "-f", f"remote-debugging-port={PORT}"], capture_output=True)
    time.sleep(2)
    profile = os.path.join(SCRATCH, "profile")
    subprocess.run(["rm", "-rf", profile], check=False)
    proc = subprocess.Popen(
        [
            CHROME,
            "--headless=new",
            "--no-sandbox",
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
            f"--remote-debugging-port={PORT}",
            "--remote-allow-origins=*",
            f"--user-data-dir={profile}",
            "--hide-scrollbars",
            "--window-size=1280,720",
            # 无头 rAF 默认被节流到 ~1 fps,这两条是任何基于帧的测量的前提。
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "about:blank",
        ],
        stdout=open(os.path.join(SCRATCH, "chrome.log"), "wb"),
        stderr=subprocess.STDOUT,
    )
    for _ in range(40):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=2)
            return proc
        except Exception:
            time.sleep(1)
    proc.kill()
    raise SystemExit("FATAL: CDP never came up")


async def run(url, shot_name, wait_s, label):
    # 开一个 about:blank tab,**先**接上 CDP 并 enable 各 domain,再
    # Page.navigate —— 否则页面在连接建立前就已经把 console 冲掉了,
    # 而「WebGPU 为什么没跑起来」恰恰只在最早那几十行日志里。
    target = json.loads(
        urllib.request.urlopen(
            urllib.request.Request(
                f"http://127.0.0.1:{PORT}/json/new?about:blank", method="PUT"
            ),
            timeout=30,
        ).read()
    )
    ws_url = target["webSocketDebuggerUrl"]
    logs = []
    async with websockets.connect(ws_url, max_size=80 * 1024 * 1024) as ws:
        drv = Driver(ws)
        await drv.cmd("Runtime.enable")
        await drv.cmd("Log.enable")
        await drv.cmd("Page.enable")
        # 页面内录一份 console:`Runtime.consoleAPICalled` 只在 enable 之后
        # 才收得到事件,而 wasm 的启动日志(尤其是「WebGPU unavailable」)
        # 在第一帧就发完了。这里在**任何页面脚本之前**挂上钩子。
        await drv.cmd(
            "Page.addScriptToEvaluateOnNewDocument",
            {
                "source": (
                    "(()=>{window.__LOGS=[];const o=console.log.bind(console);"
                    "console.log=(...a)=>{window.__LOGS.push('log '+a.map(String).join(' '));o(...a);};"
                    "const e=console.error.bind(console);"
                    "console.error=(...a)=>{window.__LOGS.push('ERR '+a.map(String).join(' '));e(...a);};"
                    "const w=console.warn.bind(console);"
                    "console.warn=(...a)=>{window.__LOGS.push('WARN '+a.map(String).join(' '));w(...a);};"
                    "addEventListener('error',ev=>window.__LOGS.push('EXC '+ev.message));"
                    "addEventListener('unhandledrejection',ev=>window.__LOGS.push('REJ '+ev.reason));"
                    "})();"
                )
            },
        )

        # 收集 console / exception
        async def pump():
            while True:
                try:
                    msg = json.loads(await asyncio.wait_for(ws.recv(), timeout=2))
                except asyncio.TimeoutError:
                    continue
                except Exception:
                    return
                m = msg.get("method")
                if m == "Runtime.consoleAPICalled":
                    parts = [
                        str(a.get("value", a.get("description", "")))
                        for a in msg["params"].get("args", [])
                    ]
                    logs.append(
                        f"[{msg['params'].get('type')}] " + " ".join(parts)
                    )
                elif m == "Runtime.exceptionThrown":
                    det = msg["params"]["exceptionDetails"]
                    logs.append("[exception] " + str(det.get("text")) + " " + str(
                        det.get("exception", {}).get("description", "")))
                elif m == "Log.entryAdded":
                    e = msg["params"]["entry"]
                    logs.append(f"[{e.get('level')}] {e.get('text')}")

        pump_task = asyncio.ensure_future(pump())

        await drv.cmd("Page.navigate", {"url": url})

        # 等场景就绪
        deadline = time.time() + wait_s
        hud1 = hud2 = None
        while time.time() < deadline:
            hud1 = await drv.ev(
                "(()=>{const h=document.querySelector('#vcw-hud');"
                "return h?h.textContent:null})()"
            )
            if hud1 and "tris" not in str(hud1):
                pass
            await asyncio.sleep(3)
            hud2 = await drv.ev(
                "(()=>{const h=document.querySelector('#vcw-hud');"
                "return h?h.textContent:null})()"
            )
            if hud2 and str(hud2) != str(hud1):
                break

        hud = await drv.ev(
            "(()=>{const h=document.querySelector('#vcw-hud');"
            "return h?h.textContent:null})()"
        )
        snap = await drv.ev(
            "(()=>{const v=window.__vcw; if(!v) return {missing:true}; "
            "return {keys:Object.keys(v), frame:v.frame, frameCount:v.frameCount,"
            " tris:v.triangleCount, backend:v.backend, backendNote:v.backendNote,"
            " gpu:v.gpu, fps:v.fps};})()"
        )
        ctx = await drv.ev(
            "(()=>{const c=document.getElementById('vcw-canvas');"
            "if(!c) return null; const g=c.getContext('2d');"
            "return {w:c.width,h:c.height,cw:c.clientWidth,ch:c.clientHeight,"
            "style:c.getAttribute('style')||''};})()"
        )
        adapter = await drv.ev(
            "(async()=>{try{const a=await navigator.gpu.requestAdapter();"
            "if(!a)return 'ADAPTER_NULL';const i=a.info||{};"
            "return {vendor:i.vendor,arch:i.architecture,desc:i.description};}"
            "catch(e){return 'ERR:'+e}})()"
        )

        page_logs = await drv.ev("(()=>window.__LOGS||[])()")
        logmeta = await drv.ev(
            "(()=>{const L=window.__LOGS||[];"
            "return {n:L.length,has:L.some(x=>String(x).includes('diag7')),"
            "diag5:L.filter(x=>String(x).includes('diag5')).length,"
            "diag6:L.filter(x=>String(x).includes('diag6')).length,"
            "sample:L.slice(0,3)};})()"
        )
        all_console = logmeta
        # Is the game's rAF loop alive, and does an INDEPENDENT clear on the
        # SAME canvas land? That separates "my submits are dropped" from
        # "this canvas never presents".
        indep = await drv.ev(
            "(async()=>{try{"
            "const c=document.getElementById('vcw-canvas');"
            "const ctx=c.getContext('webgpu');"
            "const ad=await navigator.gpu.requestAdapter();"
            "const d=await ad.requestDevice();"
            "const t=ctx.getCurrentTexture();"
            "const e=d.createCommandEncoder();"
            "const ps=e.beginRenderPass({colorAttachments:[{view:t.createView(),"
            "clearValue:{r:0,g:1,b:0,a:1},loadOp:'clear',storeOp:'store'}]});"
            "ps.end();d.queue.submit([e.finish()]);"
            "await d.queue.onSubmittedWorkDone();"
            "const cv=document.createElement('canvas');cv.width=c.width;cv.height=c.height;"
            "const g=cv.getContext('2d');g.drawImage(c,0,0);"
            "const px=g.getImageData(0,0,4,1).data;"
            "return {px:[px[0],px[1],px[2],px[3]]};"
            "}catch(e){return {err:String(e)}}})()"
        )
        series = await drv.ev(
            "(()=>new Promise(res=>{const c=document.getElementById('vcw-canvas');"
            "const cv=document.createElement('canvas');cv.width=8;cv.height=8;"
            "const g=cv.getContext('2d');const out=[];let n=0;"
            "const t=setInterval(()=>{g.drawImage(c,0,0,8,8);"
            "const d=g.getImageData(0,0,8,8).data;"
            "let sum=0,alp=0;for(let i=0;i<d.length;i+=4){sum+=d[i]+d[i+1]+d[i+2];alp+=d[i+3];}"
            "out.push([sum/(d.length/4),alp/(d.length/4)]);"
            "if(++n>=12){clearInterval(t);res(out);}},120);}))()"
        )
        pixels = await drv.ev(
            "(()=>{const c=document.getElementById('vcw-canvas');"
            "if(!c)return null;"
            "const t=document.createElement('canvas');t.width=c.width;t.height=c.height;"
            "const g=t.getContext('2d');"
            "try{g.drawImage(c,0,0);}catch(e){return {err:'draw:'+e};}"
            "const d=g.getImageData(0,0,c.width,c.height).data;"
            "const counts={};let sum=[0,0,0];"
            "for(let i=0;i<d.length;i+=4){"
            "const k=d[i]+','+d[i+1]+','+d[i+2]+','+d[i+3];counts[k]=(counts[k]||0)+1;"
            "sum[0]+=d[i];sum[1]+=d[i+1];sum[2]+=d[i+2];}"
            "const n=d.length/4;"
            "const top=Object.entries(counts).sort((a,b)=>b[1]-a[1]).slice(0,5);"
            "return {avg:sum.map(v=>Math.round(v/n)),top:top};})()"
        )
        texinfo = await drv.ev(
            "(async()=>{try{"
            "const c=document.getElementById('vcw-canvas');"
            "const ctx=c.getContext('webgpu');"
            "if(!ctx)return {err:'no ctx'};"
            "const t=ctx.getCurrentTexture();"
            "return {w:t.width,h:t.height,fmt:t.format,usage:t.usage,"
            "label:t.label,dims:t.dimension,samples:t.sampleCount};"
            "}catch(e){return {err:String(e)}}})()"
        )
        probe = await drv.ev(
            "(()=>{const c=document.getElementById('vcw-canvas');"
            "if(!c)return {err:'no canvas'};"
            "let ctx='none';"
            "try{ctx=c.getContext('webgpu')?'webgpu-ctx-present':'webgpu-ctx-null';}catch(e){ctx='throw:'+e;}"
            "return {w:c.width,h:c.height,style:c.getAttribute('style')||'',"
            "ctxKind:ctx,opaque:getComputedStyle(c).opacity,"
            "z:getComputedStyle(c).zIndex,"
            "vis:getComputedStyle(c).visibility,"
            "disp:getComputedStyle(c).display,"
            "canvases:Array.from(document.querySelectorAll('canvas')).map(x=>({id:x.id,w:x.width,h:x.height,z:getComputedStyle(x).zIndex,pos:getComputedStyle(x).position}))};})()"
        )
        rect = await drv.ev(
            "(()=>{const c=document.getElementById('vcw-canvas');"
            "if(!c) return null; const r=c.getBoundingClientRect();"
            "return {x:r.x,y:r.y,w:r.width,h:r.height};})()"
        )
        os.makedirs(SHOTS, exist_ok=True)
        params = {"format": "png"}
        if rect:
            params["clip"] = {"x": rect["x"], "y": rect["y"],
                              "width": rect["w"], "height": rect["h"],
                              "scale": 1}
        shot = await drv.cmd("Page.captureScreenshot", params)
        import base64

        path = os.path.join(SHOTS, shot_name)
        with open(path, "wb") as fh:
            fh.write(base64.b64decode(shot["data"]))

        pump_task.cancel()
        await drv.cmd("Page.close")
        return {
            "label": label,
            "hud": hud,
            "canvas": ctx,
            "snapshot": snap,
            "adapter": adapter,
            "shot": path,
            "logs": logs,
            "pageLogs": page_logs if isinstance(page_logs, list) else [],
            "console": all_console,
            "probe": probe,
            "texinfo": texinfo,
            "pixels": pixels,
            "series": series,
            "indep": indep,
        }


async def main():
    proc = launch()
    try:
        results = []
        # WebGPU 切片(默认后端)
        results.append(
            await run(
                f"http://127.0.0.1:8877/index.html?res=1.0&cb={time.time()}",
                "webgpu-slice.png",
                150,
                "WEBGPU",
            )
        )
        # WebGL2 对照组
        results.append(
            await run(
                f"http://127.0.0.1:8877/index.html?res=1.0&backend=webgl2&cb={time.time()}",
                "webgl2-control.png",
                150,
                "WEBGL2",
            )
        )
        print(json.dumps(results, indent=2, ensure_ascii=False))
    finally:
        subprocess.run(["pkill", "-9", "-f", f"remote-debugging-port={PORT}"], capture_output=True)
        proc.kill()


if __name__ == "__main__":
    asyncio.run(main())
