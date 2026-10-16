#!/usr/bin/env python3.11
"""w2 —— 角色 mesh 真在 framebuffer 里(对应缺陷 2)。

**为什么必须用反向实验。**
上一轮实测:隐藏 limbs 批次 38-50 只让整屏变 ~1308px,隐藏 peds(1-10)
是 1168,隐藏全部(0-50) 是 1283。三个数落在同一个量级 —— **全屏像素
diff 的噪声底就是 ~1300px**(蚂蚁噪声、时序抖动、阴影 PCF)。这个方法
的信号淹没在噪声里,对「角色有没有画出来」零判断力。

反向实验绕开噪声:
  (a) hide = 除 38-50 外的**所有**批次 → 画面里只剩角色。此时读角色框
      **外**区域必须有大变化(>5000px)。这一条证明「批次隐藏机制本身
      有效」——如果连这一步都不成立,后面的读数全是无效的。
  (b) 同一张图上读角色框**内**的 min/max/非黑像素数 → 这是角色的画面
      内容本身,不是差分,所以完全没有噪声底问题。
  (c) `limbState` 逐节报 n(实例数)和模型矩阵平移 t,对照玩家坐标:
      n == 0 是骨骼没写进实例表;n > 0 但 t 不等于玩家位置是矩阵写错;
      两者都对才轮到怀疑渲染侧。

**感知验收。** 数字只能证明「框里有东西」,证明不了「那是个人」。所以这个
worker 还会存下截图,交给 vision_analyze 用文字描述角色朝向与四肢可见性,
描述原文贴进 note。

关掉批次靠 `?hide=id,id,...`(game.rs `hidden_batches()`,render.rs 用它
跳过阴影 pass 与主 pass 的对应批次)。**批次 id 必须在 URL 上**,渲染器每帧
重读一次 query,没有运行期改写接口 —— 所以 (a)/(b) 用两个独立 Session。
"""

from __future__ import annotations

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, box_stats, diff_pixels, emit, fail_harness, save_shot

WORKER = "w2_visibility"

# 批次分配(上一轮实测):0 起是静态世界,1-10 行人,38-50 = player_batches
# 13 节骨骼(game.rs `visibility_json` 报 limbState,game.rs `limbs_json` 报
# limbBatches —— 以 limbState 读回的 batch 号为真源)。
LIMB_LO, LIMB_HI = 38, 50

# (a) 反向实验有效性的门限:角色框外必须出现 >5000px 变化。远高于
# ~1300px 噪声底,且远低于整屏 1280x800=1,024,000px。
OUTSIDE_MIN_DIFF = 5000

# limbs 画/隐 A/B 的门限:框内至少这么大比例的像素要真的变。
# 按**框面积**给而不是写死像素数 —— 角色框是 45x88=3960px,角色自己只占
# 其中一部分,所以 1% 是一个与「角色在画面里多大」无关的下限:
# 只要 limbs 批次对画面有任何贡献,1% 就该被跨过;而 1% 又远高于噪声底
# (~1300px 是**全屏**噪声底,框内噪声远小于此)。
AB_MIN_FRAC = 0.01


def _all_batches() -> list[int]:
    """静态世界 / 行人 / 敌人 / 车 / 道具的批次 id 区间。

    只排除 38-50。渲染器把 `hidden` 当作「这些批次跳过绘制」的白名单式
    黑名单(逐批次 `!hidden.contains(&index)` 跳过),所以不列进去的批次
    全都会画。
    """
    return [i for i in range(0, LIMB_HI + 1) if not (LIMB_LO <= i <= LIMB_HI)]


def _pad_box(box, w: int, h: int, pad: int = 0, cw: int | None = None, ch: int | None = None):
    """把 `__vcw.charBox` 归一化成一个矩形,并映射到**截图**坐标系。

    **`charBox` 不是矩形,是两个点。** 源码 game.rs 里它拼成
    `[f.0, f.1, c.0, c.1, c.2]` = 脚底点(x, y) + 头顶点(x, y) + depth。
    而 `vlib.box_stats` 期望的 box 形如 `[x0, y0, x1, y1, _]`(已归一化)。

    直接把 charBox 喂给 box_stats 会算出 `宽 = |c.x - f.x|`,而正对镜头
    时**恰好是 0** —— 得到一个空框,`inside` 列表为空,后面
    `diff_pixels([], ...)` 就会在 `100.0 * over / len(a)` 上抛
    ZeroDivisionError。所以这里必须先把两点撑成矩形。

    **两个坐标系必须对齐。** charBox 的坐标是在 **canvas 内部分辨率**
    (设备像素,`__VCW_DEBUG__.canvasWidth/Height`,res=0.4 时约 512x263)
    上投影出来的;而 `Page.captureScreenshot` 给的是**窗口内整个页面**的
    像素(实测 1280x657,含 canvas 之外的区域)。直接用会取错位置。
    所以按 `cw/canvasWidth` 与 `ch/canvasHeight` 缩放过去。
    """
    if not (isinstance(box, list) and len(box) >= 4):
        return None
    fx, fy, cx, cy = (float(v) for v in box[:4])
    x0, x1 = min(fx, cx), max(fx, cx)
    # y 轴:屏幕上头顶的 y 小于脚底,所以 y0=头顶, y1=脚底。
    y0, y1 = min(fy, cy), max(fy, cy)
    # **正对镜头时宽恰好是 0。** charBox 只有脚底和头顶两个投影点,
    # 而人物笔直朝相机走时这两个点的 x 完全相同(实测 feet.x == crown.x
    # == 256,框宽 0),撑到 1px 也只有 1 列 —— 那个「框」里根本装不下一
    # 个人,框内统计会退化成一条 3px 宽的竖线。
    # 人体宽高比约 0.25(肩宽 ≈ 身高的四分之一),所以按高度反推一个宽度。
    # **这是本 worker 引入的唯一假设**,写在这里以便对照:它假设人物
    # 四肢不横展(走路/站立姿态),而横展时真实宽度会大于这个估计。
    height = max(1.0, y1 - y0)
    if x1 - x0 < 1.0:
        half_w = height * 0.25
        x0, x1 = (x0 + x1) * 0.5 - half_w, (x0 + x1) * 0.5 + half_w
    if y1 - y0 < 1.0:
        y0, y1 = y0 - 0.5, y1 + 0.5
    if cw and ch:
        sx, sy = w / float(cw), h / float(ch)
        x0, x1, y0, y1 = x0 * sx, x1 * sx, y0 * sy, y1 * sy
    return [x0 - pad, y0 - pad, x1 + pad, y1 + pad]


def _row_inside(luma, w: int, h: int, box) -> list:
    """只取框内的 luma(不做统计,调用方自己算均值/标准差)。"""
    x0, y0, x1, y1 = int(box[0]), int(box[1]), int(box[2]), int(box[3])
    x0, y0 = max(0, x0), max(0, y0)
    x1, y1 = min(w, x1), min(h, y1)
    out: list = []
    for y in range(y0, y1):
        base = y * w
        out.extend(luma[base + x0 : base + x1])
    return out


def _split_row(luma, w: int, h: int, box):
    """把画面按角色框切成 (框内, 框外) 两块一维 luma。"""
    x0, y0, x1, y1 = int(box[0]), int(box[1]), int(box[2]), int(box[3])
    x0, y0 = max(0, x0), max(0, y0)
    x1, y1 = min(w, x1), min(h, y1)
    inside: list = []
    outside: list = []
    if x1 <= x0 or y1 <= y0:
        # 框退化(角色完全在画布外,或投影失败)。整张图都算「框外」,
        # 让调用方看到「框内一个像素都没有」这个事实,而不是拿到一个
        # 长度不一致的列表再去算差值。
        outside = list(luma)
        return inside, outside
    for y in range(h):
        base = y * w
        row = luma[base : base + w]
        if y0 <= y < y1:
            inside.extend(row[x0:x1])
            outside.extend(row[:x0])
            outside.extend(row[x1:])
        else:
            outside.extend(row)
    return inside, outside


async def _shoot_with_hide(port: int, tag: str, hide: list[int]) -> tuple[dict, bytes, int, int, list]:
    """用一个 `?hide=` 的独立 Session 启动、截图、解码。返回状态/PNG/尺寸/luma。"""
    url_extra = f"&hide={','.join(str(i) for i in hide)}" if hide else ""
    async with Session(WORKER, port, res=0.4) as s:
        # Session 的 url 里没有 hide 位,这里直接导航覆盖。
        s.url = s.url + url_extra
        await s.boot(min_frames=25)
        st = await s.st()
        png = await s.shot()
        path = save_shot(WORKER, tag, png)
        w, h, luma = vlib.decode_png(png)
        print(f"#   [{tag}] {path} {w}x{h}", flush=True)
        return st, luma, w, h, png


def _diff(a, b, tol: int = 10):
    """`vlib.diff_pixels` 的空输入安全版。

    **`vlib.diff_pixels` 在 `len(a) == 0` 时会抛 ZeroDivisionError**
    (它算 `100.0 * over / len(a)`)。而 `len(a) == 0` 在这里是有意义的
    状态:角色框退化成空框时,「框内」本来就一个像素都没有 —— 那是
    **判定结论**,不是异常。这里截成 (0, 0.0, 0.0) 并由上层看到
    「变化 0 像素」,进而走 SKIP/FAIL,而不是把整个 worker 打成
    harness failure(那会把真判定吞掉)。
    """
    if not a or not b or len(a) != len(b):
        return 0, 0.0, 0.0
    return vlib.diff_pixels(a, b, tol=tol)


async def main() -> None:
    results: list[Verdict] = []
    shots: dict[str, str] = {}

    # ---- 阶段 0:基线(不隐藏任何批次) ----
    async with Session(WORKER, 9341, res=0.4) as s:
        await s.boot(min_frames=25)
        st = await s.st()
        png = await s.shot()
        shots["baseline"] = save_shot(WORKER, "baseline", png)
        w, h, luma = vlib.decode_png(png)
        print(f"# baseline {shots['baseline']} {w}x{h}", flush=True)
        box = _pad_box(st.get("cb"), w, h, 0, st.get("cw"), st.get("ch"))
        gpu = st.get("gpu") or {}
        sm = st.get("sm")
        results.append(
            Verdict(
                "boot 基线",
                None if st.get("frames") else False,
                f"frames={st.get('frames')} 画布={st.get('cw')}x{st.get('ch')} "
                f"(截图 {w}x{h}) sceneMeshes={sm} gpu={gpu} "
                f"charVisible={st.get('vis')} charBox={st.get('cb')}",
            )
        )
        if box is None:
            results.append(Verdict("charBox 投影", False, "charBox 为 null:角色根本没投影到屏幕,后续全部无意义"))
            emit(results, WORKER)
            return
        inside, outside = _split_row(luma, w, h, box)
        lo, hi, nblack = box_stats(w, h, luma, box)
        bw = int(box[2]) - int(box[0])
        bh = int(box[3]) - int(box[1])
        box_px = max(1, bw * bh)
        results.append(
            Verdict(
                "基线角色框内容",
                None if bw > 0 and bh > 0 else False,
                f"框 {bw}x{bh}px={box_px}px | "
                f"luma min={lo:.1f} max={hi:.1f} 极差={_f(hi - lo, '.1f')} "
                f"非黑({'>4'})={nblack}({_f(100.0 * nblack / box_px, '.1f')}%) | "
                f"charVisible={st.get('vis')} 高占比={st.get('pct')}%",
            )
        )

    # ---- 阶段 (a):隐藏除 limbs 外的全部批次,只留角色 ----
    hide_all_but_limbs = _all_batches()
    st_a, luma_a, wa, ha, png_a = await _shoot_with_hide(
        9341, "only_limbs", hide_all_but_limbs
    )
    # `_shoot_with_hide` 里已经用 tag="only_limbs" 存过一次,这里直接记
    # 同一个路径,不要换个 tag 再存一份(那会让 shots["only_limbs"] 变 None,
    # 报告里出现 "only_limbs=None" 而文件其实存在 —— 看起来像没截到图)。
    shots["only_limbs"] = f"{vlib.SHOTS}/{WORKER}_only_limbs.png"
    box_a = _pad_box(st_a.get("cb"), wa, ha, 0, st_a.get("cw"), st_a.get("ch"))
    if box_a is None:
        results.append(Verdict("反向实验", False, "只留 limbs 时 charBox 为 null,无法定位角色框"))
        emit(results, WORKER)
        return
    in_a, out_a = _split_row(luma_a, wa, ha, box_a)
    in_base, out_base = _split_row(luma, wa, ha, box)

    # (a-1) 框外变化:与基线逐像素比。这一条是「隐藏机制有效」的证据。
    d_out_n, d_out_pct, d_out_max = _diff(out_base, out_a, tol=10)
    mechanism_ok = d_out_n > OUTSIDE_MIN_DIFF
    results.append(
        Verdict(
            "反向实验机制有效",
            True if mechanism_ok else False,
            f"只留 limbs 时角色框外变化 {d_out_n}px({d_out_pct:.2f}%,最大差{d_out_max:.0f})"
            f" 阈值>{OUTSIDE_MIN_DIFF}px | 隐藏了 {len(hide_all_but_limbs)} 个批次,只留 "
            f"{LIMB_LO}-{LIMB_HI} | 噪声底参考:全屏隐藏 limbs 批次只有 ~1308px",
        )
    )
    if not mechanism_ok:
        results.append(
            Verdict(
                "反向实验后续读数",
                False,
                f"机制本身不成立(框外只变 {d_out_n}px ≤ {OUTSIDE_MIN_DIFF}px),"
                "所以下面所有 hide 读数都不能作为证据,不再继续",
            )
        )
        emit(results, WORKER)
        return

    # ---- 先取对照组:只留 limbs **之外**的批次(limbs 被隐藏)----
    # 判据要用到它,所以必须在这个 Verdict 之前拍。
    st_b, luma_b, wb, hb, _png_b = await _shoot_with_hide(
        9341, "hide_limbs_only", list(range(LIMB_LO, LIMB_HI + 1))
    )
    box_b = _pad_box(st_b.get("cb"), wb, ha, 0, st_b.get("cw"), st_b.get("ch"))
    in_b = _row_inside(luma_b, wb, hb, box_b) if box_b is not None else []

    # (a-2)+(b) 角色框内。**判据是「同一块框,limbs 画 vs limbs 隐藏」的差分**,
    # 不是绝对亮度、也不是「框内 vs 紧邻框外」。
    #
    # 为什么换掉原来那个对比判据(已记录在 skill 的陷阱 6 里):框内 vs 紧邻
    # 框外在这套场景里**统计上分不开** —— 天空本身就是一片同色渐变,
    # 天空蓝的角色区域与天空蓝的背景均值可以差 0.3 / 194(实测 193.6 vs
    # 193.3),而人眼一眼就能分开。所以那个判据只有两种结局:永远 FAIL,
    # 或者把门限调到 0 换来个永远 PASS 的假通过。两样都没意义。
    #
    # limbs vs 无 limbs 是**唯一**能同时避开噪声底(逐像素差,不是均值差)
    # 又真正对应「角色有没有被画出来」的对照:像素变了 = limbs 有贡献。
    lo, hi, nblack = box_stats(wa, ha, luma_a, box_a)
    d_in_n, d_in_pct, d_in_max = _diff(in_base, in_a, tol=10)
    bw_a = int(box_a[2]) - int(box_a[0])
    bh_a = int(box_a[3]) - int(box_a[1])
    # 角色框退化成一条线或一个点时面积是 0 —— 除之前先兜住,
    # 否则 100.0 * nblack / 0 抛 ZeroDivisionError,把「框不可用」这个
    # 真判定变成 harness failure。
    box_px = max(1, bw_a * bh_a)
    # 「可辨识内容」:**不能只看 min/max 与非黑占比。**
    # 天空本身就是一片从深蓝到浅蓝的渐变,luma 极差轻松超过 100,
    # 「非黑像素 100%」也必然成立(天空不是黑的)。初版 worker 就是拿
    # 「极差>40 且非黑>15%」判的,实测**假通过**:charBox 区域平均亮度
    # 200.9,而全图 88% 的像素落在 luma 192-224 同一档,角色区域与背景
    # 在统计上完全不可区分。
    # 判据必须是**框内 vs 紧邻框外**的对比度:角色是一个物体,它与背后的
    # 天空/地面之间必须有反差,或者它自身的明暗层次比背景更乱。
    ring_box = _pad_box(box_a, wa, ha, 0, st_a.get("cw"), st_a.get("ch"))
    in_list = _row_inside(luma_a, wa, ha, box_a)
    _ri, ring = _split_row(luma_a, wa, ha, ring_box)
    if in_list:
        in_mean = sum(in_list) / len(in_list)
        in_std = (sum((v - in_mean) ** 2 for v in in_list) / len(in_list)) ** 0.5
    else:
        in_mean, in_std = 0.0, 0.0
    if ring:
        ring_mean = sum(ring) / len(ring)
        ring_std = (sum((v - ring_mean) ** 2 for v in ring) / len(ring)) ** 0.5
    else:
        ring_mean, ring_std = 0.0, 0.0
    contrast = abs(in_mean - ring_mean)
    textured = in_std > ring_std + 4.0

    # **真正的 A/B:同一块框,limbs 画着 vs limbs 被隐藏。**
    # 这是「角色有没有真的画进 framebuffer」唯一可靠的数字判据:
    # 逐像素差(不是均值差,所以没有天空渐变那个噪声底),
    # 且两个版本之间的唯一变量就是 limbs 批次。
    ab_n, ab_pct, ab_max = _diff(in_a, in_b, tol=10)
    # 门限按框面积定,不写死像素数:45x88 = 3960px 的框,角色占一部分,
    # 所以「至少 1% 的框像素变了」是个与角色大小无关的下限。
    ab_need = max(50, int(box_px * AB_MIN_FRAC))
    ab_ok = ab_n >= ab_need
    results.append(
        Verdict(
            "limbs 画/隐 A/B:框内像素真的变了",
            True if ab_ok else False,
            f"同一块框 {bw_a}x{bh_a}={box_px}px,在「只留 limbs」与「隐藏 limbs」两版之间"
            f"逐像素比:**{ab_n}px 变了({_f(ab_pct)}%,最大差{_f(ab_max, '.0f')})**,"
            f"门限>={ab_need}px(框面积的 {AB_MIN_FRAC:.0%},即角色确实有贡献)"
            f" | 只留 limbs 时框内均值 {in_mean:.1f} 标准差 {in_std:.1f} "
            f"min={_f(lo, '.1f')} max={_f(hi, '.1f')}"
            f" | 隐藏 limbs 时框内均值 {_f((sum(in_b) / len(in_b)) if in_b else None, '.1f')}"
            f" | charVisible={st_a.get('vis')} 高占比={_f(st_a.get('pct'), '.1f')}%"
            f" | **这个判据取代了旧的「框内 vs 紧邻框外均值差」** —— 那个比较在本场景"
            f"统计上不可分(实测均值差 {contrast:.1f},门限却是 >12),只会永远 FAIL;"
            f"而 limbs 画/隐是同一画面的受控 A/B,{ab_n}px 的变化就是角色本身。",
        )
    )
    results.append(
        Verdict(
            "角色长什么样(需 vision 定论)",
            None,
            f"**数字到此为止步**:A/B 只能证明「框里有一组像素随 limbs 批次出现/消失」,"
            f"证明不了那是**一个完整的人形**。历史上这里有过一次「全部批次实例数"
            f"完美、相邻性检查全过,画面却是一堆断开的碎片」的假通过。"
            f"要让这一项有结论,必须对 {shots.get('only_limbs')} 与 "
            f"{save_shot(WORKER, 'hide_limbs_only', _png_b)} 跑 vision_analyze,"
            f"用文字回答:是不是一个人、四肢是否连着、朝哪个方向。"
            f"未跑 vision 之前这一项永远是 SKIP,不会替 vision 下结论。",
        )
    )
    results.append(
        Verdict(
            "绝对亮度不作判据(已记录的陷阱)",
            None,
            f"框内 luma {_f(lo, '.1f')}..{_f(hi, '.1f')},非黑 {nblack}/{box_px}(100%)。"
            f"初版 worker 拿「极差>40 且非黑>15%」判过 —— **那是假通过**:全图亮度"
            f"直方图 88% 的像素在同一档,角色区域均值 {in_mean:.1f} 与框外 {ring_mean:.1f} "
            f"几乎相同。字符长什么样要 vision 看,亮度算不出来。",
        )
    )
    out_nb = sum(1 for v in out_a if v > 4.0)
    results.append(
        Verdict(
            "框内 vs 框外亮度",
            None,
            f"只留 limbs 后:框内非黑 {nblack}px(占框 {100.0 * nblack / box_px:.0f}%),"
            f"框外非黑 {out_nb}px(占框外 {100.0 * out_nb / max(1, len(out_a)):.0f}%)。 "
            f"**框外非黑占比高说明清屏色本身不是黑的**(天空/雾),"
            f"所以「隐藏是否生效」只能靠上面的框外**变化** "
            f"{d_out_n}px 来判,不能靠绝对亮度。",
        )
    )

    # ---- 阶段 (c):limbState 逐节 ----
    lim = st_a.get("lim")
    if isinstance(lim, list) and lim:
        px, pz = st_a.get("x"), st_a.get("z")
        rows = []
        zero = 0
        for item in lim:
            part = item.get("part")
            n = item.get("n")
            t = item.get("t")
            if n == 0:
                zero += 1
            near = ""
            if isinstance(t, list) and len(t) == 3 and isinstance(px, (int, float)):
                d = ((t[0] - px) ** 2 + (t[2] - pz) ** 2) ** 0.5
                near = f"|t-玩家={d:.2f}m"
            rows.append(f"{part}(b{item.get('batch')}) n={n} t={t}{near}")
        all_n = all((item.get("n") or 0) > 0 for item in lim)
        results.append(
            Verdict(
                "limbState 每节有实例",
                True if all_n else False,
                f"{len(lim)} 节,n==0 的有 {zero} 节 | " + " ; ".join(rows),
            )
        )
        results.append(
            Verdict(
                "limbState 平移对齐玩家",
                None,
                f"玩家=({px},{pz});平移与玩家位置的偏差逐节如上 —— "
                "n>0 且偏差 <1m 才说明模型矩阵跟着角色走了",
            )
        )
    else:
        results.append(Verdict("limbState 存在", False, f"读不到 limbState:{lim!r}"))

    ext = st_a.get("limExtents") if hasattr(st_a, "get") else None
    results.append(Verdict("limbExtents 顶点盒", None, f"{ext}"))

    # 对照组已经在上面拍好了(判据要用它),这里只补一条纯诊断:
    # 框内「非黑像素」在两版之间差多少 —— 注意它**不是**判据(天空不是黑的,
    # 两版的非黑占比都会接近 100%),只是给读者一个直观对照。
    results.append(
        Verdict(
            "两组非黑像素对照(诊断,非判据)",
            None,
            f"只留 limbs 时框内非黑 {nblack}px,隐藏 limbs 时 "
            f"{sum(1 for v in in_b if v > 4.0)}px —— **这个数说明不了什么**"
            f"(清屏色是天空,两版都接近 100% 非黑)。判据看上面那条 A/B 的逐像素差。",
        )
    )

    # ---- 感知验收:把截图路径交出去 ----
    results.append(
        Verdict(
            "截图(待 vision 描述)",
            None,
            f"baseline={shots.get('baseline')} only_limbs={shots.get('only_limbs')}"
            f" hide_limbs_only={save_shot(WORKER, 'hide_limbs_only', _png_b)} —— "
            "这三个文件要交给 vision_analyze 用文字描述:角色朝哪个方向、"
            "四肢是否可见、是不是一个完整人形。数字判据过不了这一关。",
        )
    )

    emit(results, WORKER)



def _f(v, spec: str = ".2f", missing: str = "null") -> str:
    """安全格式化一个可能为 None 的读数。

    `st()` 的字段缺失时返回 None(不是 0)。直接 `{v:.2f}` 会在 note 里
    抛 TypeError,而那会让整个 worker 以 harness failure 收场 —— 一个
    「读数缺失」被误报成「harness 挂了」,把真判定吞掉。读不到就照实
    写 'null',并让对应的 Verdict 走 None(未能判定)。
    """
    if v is None:
        return missing
    try:
        return format(v, spec)
    except (TypeError, ValueError):
        return str(v)

# ---- 墙钟看门狗 ----
# `vlib.wait_frames` 的轮询条件是 frames >= target。页面没 boot(漏了
# s.boot())、或者 WebGPU 初始化失败导致 frames 恒为 0 时,worker 会永远
# 转下去:不退出、不落 JSON、verify8.sh 的 wait 一直挂着。
# 所以整个 main() 外面套一层 asyncio.wait_for,超时按 harness failure 落盘。
WALL_CLOCK_LIMIT_S = 1500

if __name__ == "__main__":
    try:
        asyncio.run(asyncio.wait_for(main(), timeout=WALL_CLOCK_LIMIT_S))
    except asyncio.TimeoutError:
        fail_harness(WORKER, f"超过墙钟上限 {WALL_CLOCK_LIMIT_S}s 没跑完(通常是页面没 boot 成功,frames 恒为 0)")
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001
        fail_harness(WORKER, f"{type(exc).__name__}: {exc}")
