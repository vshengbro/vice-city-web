"""用 PIL 直接量两张已保存的 PNG 的垂直剖面。

- `webgl2-control.png` = **已知正确**(视觉确认:地面在下、天空在上)
- `webgpu-slice.png`  = 当前 WebGPU 输出

同一段代码、同一套阈值,量已知正确的那张来**校准指标本身**。
如果 WebGL2 那张也量成「底部是天空」,那说明指标量错了,而不是
WebGPU 画面反了。
"""

import sys

from PIL import Image


def profile(path, n_rows=12):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    px = im.load()
    rows = []
    for i in range(n_rows):
        y0, y1 = h * i // n_rows, h * (i + 1) // n_rows
        dark = sky = tot = 0
        for y in range(y0, y1, 4):
            for x in range(0, w, 4):
                r, g, b = px[x, y]
                lum = 0.299 * r + 0.587 * g + 0.114 * b
                if lum < 90:
                    dark += 1
                if b - r > 40 and g > 120:
                    sky += 1
                tot += 1
        rows.append((y0, 100 * dark // tot, 100 * sky // tot))
    return w, h, rows


for label, path in (
    ("WEBGL2 control (KNOWN GOOD)", "tools/gpu-probe/shots/webgl2-control.png"),
    ("WEBGPU current", "tools/gpu-probe/shots/webgpu-slice.png"),
):
    try:
        w, h, rows = profile(path)
    except FileNotFoundError:
        print(f"{label}: MISSING {path}")
        continue
    print(f"--- {label}  ({w}x{h})")
    print("    y     dark%  sky%")
    for y0, d, s in rows:
        bar = "#" * (d // 5)
        print(f"  {y0:5d}  {d:5d}  {s:5d}  {bar}")
    print()
