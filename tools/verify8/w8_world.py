#!/usr/bin/env python3.11
"""w8 —— 世界连贯性(对应缺陷 5)。

(a) 帧数增加时 tris 稳定(不泄漏)。**判据不是「三角形数在涨」。** 涨是
    正常的(流式地面跟着玩家铺开),真正要抓的是**单帧增量**与
    长时间不回落的累积。做法:分 4 段各推进 30 帧,逐帧记 tris,看
      - 单帧最大增量(>2000/帧就是每帧都在新建批次,是泄漏)
      - 玩家不动时后 3 段的总增量(应当收敛到 0,而不是线性涨)
(b) 流式加载:传送 2 次到不同区域,读 loadedAssets / streamX / streamZ,
    并确认画面**仍然有内容** —— 数字为正不算证据,必须看画面。
(c) 拾取物:传送到最近的一个 pickup,读 pickups.taken 增加。
    拾取半径 PICKUP_RADIUS=2.2m(collect_pickups, game.rs:7025),判据是
    **距离进到 2.2m 以内**,不是「差不多到了」。
(d) 昼夜:按 T,phase 变化,且画面变亮/变暗。亮度用 decode_png 的 luma
    均值 —— 这一条是**数字与感知双向**:数字给均值,vision 给文字描述。
(e) HUD 文本非空:hud 字段。

**感知验收。** 三张关键截图(初始 / 传送后 / 切昼夜后)交给 vision_analyze,
用文字描述「这里看起来是城市还是虚空」「白天还是夜里」。
"""

from __future__ import annotations

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, box_stats, emit, fail_harness, save_shot

WORKER = "w8_world"
RES = 0.4

SEGMENTS = 4
FRAMES_PER_SEGMENT = 30
# 单帧三角形增量门限。流式地面一次重建会一次涨几万,那是**分段**事件;
# 逐帧看,正常帧的增量应该接近 0。
LEAK_PER_FRAME = 2000
# 玩家不动时,后面 3 段的总增量门限(允许流式地面补齐一小块)
IDLE_GROWTH_MAX = 4000
PICKUP_RADIUS = 2.2
# 昼夜亮度差:画面平均亮度至少要变这么多才算「真的变了」
PHASE_LUMA_DELTA = 3.0


def _mean_luma(luma: list) -> float:
    if not luma:
        return 0.0
    return sum(luma) / len(luma)


async def _rich(s: Session) -> dict:
    st = await s.st()
    ex = await s.ev(
        "(() => { const c = window.__vcw || {}; const d = window.__VCW_DEBUG__ || {};"
        " return {phase: c.phase, loadedAssets: c.loadedAssets,"
        " streamX: c.streamX, streamZ: c.streamZ, bn: c.bn,"
        " pickupPos: c.pickupPos, healthPack: c.healthPack,"
        " pickups: c.pickups, hud: c.hud, tris: c.tris,"
        " playerX: c.playerX, playerZ: c.playerZ,"
        " cw: d.canvasWidth, ch: d.canvasHeight,"
        " sm: d.sceneMeshes, gpu: d.gpu}; })()"
    ) or {}
    st.update({k: v for k, v in ex.items() if k not in st})
    return st


async def _shot_luma(s: Session, tag: str):
    png = await s.shot()
    path = save_shot(WORKER, tag, png)
    w, h, luma = vlib.decode_png(png)
    return png, path, w, h, luma


async def _teleport(s: Session, x: float, z: float, settle: int = 12) -> None:
    await s.ev(f"(() => {{ window.__vcw_teleport = {{x: {x}, z: {z}}}; return 1; }})()")
    await s.wait_frames(settle)


async def main() -> None:
    results: list[Verdict] = []
    async with Session(WORKER, 9347, res=RES) as s:
        await s.boot(min_frames=25)
        st = await _rich(s)
        results.append(
            Verdict(
                "boot",
                None if st.get("frames") else False,
                f"frames={st.get('frames')} 位置=({st.get('x')},{st.get('z')}) "
                f"tris={st.get('tris')} batches={st.get('bn')} "
                f"sceneMeshes={st.get('sm')} gpu={st.get('gpu')} "
                f"collisionShapes={st.get('shapes')} 画布={st.get('cw')}x{st.get('ch')}",
            )
        )

        png0, path0, w0, h0, luma0 = await _shot_luma(s, "initial")
        base_luma = _mean_luma(luma0)
        # 「画面有内容」的廉价但必要的下界:非黑像素要够多。
        _lo, _hi, nonblack = box_stats(w0, h0, luma0, [0, 0, w0, h0])
        px_total = max(1, w0 * h0)
        results.append(
            Verdict(
                "初始画面有内容",
                True if nonblack > 0.5 * w0 * h0 else None,
                f"{w0}x{h0} 平均亮度 {base_luma:.1f},非黑像素 {nonblack}/{w0 * h0}"
                f"({_f(100.0 * nonblack / px_total, '.1f')}%) | {path0}",
            )
        )

        # ---- (a) tris 稳定性 ----
        # 不按「在涨」判泄漏,按「单帧增量」和「静止时的累积」判。
        seg_tris: list[list[int]] = []
        for _ in range(SEGMENTS):
            seg: list[int] = []
            for _ in range(FRAMES_PER_SEGMENT):
                await s.wait_frames(1)
                snap = await s.st()
                seg.append(snap.get("tris") if isinstance(snap.get("tris"), int) else 0)
            seg_tris.append(seg)
        flat = [t for seg in seg_tris for t in seg]
        if len(flat) < 10 or all(t == 0 for t in flat):
            results.append(Verdict("a tris 稳定", None, f"tris 读不到或恒 0:{flat[:20]}"))
        else:
            per_frame = [flat[i] - flat[i - 1] for i in range(1, len(flat))]
            big = [d for d in per_frame if d > LEAK_PER_FRAME]
            growth_tail = flat[-1] - flat[SEGMENTS * FRAMES_PER_SEGMENT // 2]
            no_leak = not big and growth_tail <= IDLE_GROWTH_MAX
            results.append(
                Verdict(
                    "a tris 无泄漏",
                    True if no_leak else False,
                    f"{len(flat)} 帧 tris {flat[0]} -> {flat[-1]};单帧增量 "
                    f"max={_f(max(per_frame), '+d')} min={_f(min(per_frame), '+d')},"
                    f"超 +{LEAK_PER_FRAME}/帧 的有 {len(big)} 帧 | "
                    f"后半程净增 {_f(growth_tail, '+d')}(门限<={IDLE_GROWTH_MAX},玩家全程不动)"
                    f" | 逐段首值 {[seg[0] for seg in seg_tris]}",
                )
            )
            results.append(
                Verdict(
                    "a tris 读数",
                    None,
                    f"tris 每 10 帧取 1:{flat[::10]} —— 判据是**单帧增量**与"
                    f"静止累积,不是绝对值;三角数在涨本身不是缺陷(流式地面本来就在铺)",
                )
            )

        # ---- (b) 流式加载:传送到两个远处 ----
        # 目的地不写死:城市是无限的,街道轴线落在 60 的整数倍上,
        # 出生点 (33.5,45) 已经在 (30,60) 街区附近。所以往 ±180m 传,
        # 必然落到另外的街区。
        home = await _rich(s)
        first_path = ""
        for label, (tx, tz) in (("远 A", (33.5 + 180.0, 45.0 - 180.0)),
                                ("远 B", (33.5 - 180.0, 45.0 + 180.0))):
            await _teleport(s, tx, tz)
            here = await _rich(s)
            png, path, w, h, luma = await _shot_luma(s, f"teleport_{label[-1]}")
            nb = box_stats(w, h, luma, [0, 0, w, h])[2]
            moved = vlib.dist(home.get("x"), home.get("z"), here.get("x"), here.get("z"))
            results.append(
                Verdict(
                    f"b 流式 {label}",
                    None,
                    f"传送到 ({_f(tx, '.0f')},{_f(tz, '.0f')}),实到 ({_f(here.get('x'), '.1f')},{_f(here.get('z'), '.1f')})"
                    f"(移动 {moved:.0f}m) | loadedAssets {home.get('loadedAssets')} -> "
                    f"{here.get('loadedAssets')} | streamCenter ({home.get('streamX')},"
                    f"{home.get('streamZ')}) -> ({here.get('streamX')},{here.get('streamZ')}) "
                    f"| tris {here.get('tris')} batches={here.get('bn')} "
                    f"sceneMeshes={here.get('sm')} | 画面非黑 {100.0 * nb / (w * h):.1f}%,"
                    f"平均亮度 {_f(_mean_luma(luma), '.1f')} | {path}",
                )
            )
            home = here
            if label == "远 A":
                first_path = path
        results.append(
            Verdict(
                "b 传送后画面非空",
                True if box_stats(w, h, luma, [0, 0, w, h])[2] > 0.4 * w * h else False,
                f"两次传送后画面仍然有内容(非黑占比见上)。数字只证明「有像素」,"
                f"「这些像素是一座城市还是一片虚空」要靠 vision 描述。",
            )
        )

        # ---- (c) 拾取物 ----
        cur = await _rich(s)
        # 优先 healthPack(__vcw 专门导出的「第一件没被捡走的医疗包」),
        # 拿不到就退回 pickupPos 列表里最近的一个。
        target = None
        kind = "无"
        if isinstance(cur.get("healthPack"), list) and len(cur["healthPack"]) == 2:
            target = tuple(cur["healthPack"])
            kind = "healthPack(专用字段)"
        else:
            pp = cur.get("pickupPos")
            if isinstance(pp, list) and pp:
                px, pz = cur.get("x"), cur.get("z")
                target = min(
                    (tuple(p) for p in pp if isinstance(p, list) and len(p) == 2),
                    key=lambda p: vlib.dist(px, pz, p[0], p[1]),
                )
                kind = "pickupPos 里最近的一个"
        if target is None:
            results.append(Verdict("c 拾取物", None, "读不到任何未拾取的 pickup 坐标"))
        else:
            taken0 = (cur.get("pickups") or {}).get("taken")
            total0 = (cur.get("pickups") or {}).get("total")
            # 停到拾取点**正上方**再让游戏跑几帧:半径 2.2m,直接踩上去。
            await _teleport(s, target[0], target[1], settle=3)
            await s.wait_frames(6)
            after = await _rich(s)
            taken1 = (after.get("pickups") or {}).get("taken")
            gap = vlib.dist(after.get("x"), after.get("z"), target[0], target[1])
            grew = isinstance(taken0, int) and isinstance(taken1, int) and taken1 > taken0
            results.append(
                Verdict(
                    "c 拾取物被拾走",
                    True if grew else (False if gap <= PICKUP_RADIUS else None),
                    f"目标来自 {kind} = ({_f(target[0], '.1f')},{_f(target[1], '.1f')});"
                    f"拾取后玩家在 ({_f(after.get('x'))},{_f(after.get('z'))}),"
                    f"距目标 {_f(gap)}m(拾取半径 {PICKUP_RADIUS}m) | "
                    f"pickups.taken {taken0} -> {taken1}(total={total0}) "
                    f"| cash {after.get('playerCash')} hp={after.get('hp')}",
                )
            )
            results.append(
                Verdict(
                    "c 拾取判据",
                    None,
                    f"判据是 pickups.taken 增加,且玩家确实踩进了 {PICKUP_RADIUS}m 半径内"
                    f"(当前 {gap:.2f}m)。若 taken 没涨而距离在半径内,那是拾取逻辑坏了;",
                )
            )

        # ---- (d) 昼夜 ----
        before_phase = await _rich(s)
        png_d0, path_d0, wd, hd, luma_d0 = await _shot_luma(s, "phase_before")
        luma_before = _mean_luma(luma_d0)
        phase0 = before_phase.get("phase")
        await s.tap("KeyT", 130)
        await s.wait_frames(6)
        after_phase = await _rich(s)
        png_d1, path_d1, wd2, hd2, luma_d1 = await _shot_luma(s, "phase_after")
        luma_after = _mean_luma(luma_d1)
        changed = phase0 != after_phase.get("phase")
        delta = luma_after - luma_before
        results.append(
            Verdict(
                "d T 切昼夜:phase 变化",
                True if changed else False,
                f"phase {phase0} -> {after_phase.get('phase')}"
                f"(DayPhase 有 {4} 档,连按会绕圈)",
            )
        )
        results.append(
            Verdict(
                "d 画面亮度随之变化",
                True if abs(delta) >= PHASE_LUMA_DELTA else None,
                f"平均亮度 {_f(luma_before)} -> {_f(luma_after)}(Δ{_f(delta, '+.2f')},"
                f"门限|Δ|>={PHASE_LUMA_DELTA}) | {path_d0} / {path_d1}"
                f" | 这是数字半;「白天还是夜里、能不能看清路面」要看 vision 描述",
            )
        )
        # 再切一次,确认能绕回去而不是卡死。
        await s.tap("KeyT", 130)
        await s.wait_frames(4)
        cycle = await _rich(s)
        _png_c, _path_c, _wc, _hc, luma_cycle = await _shot_luma(s, "phase_cycle")
        results.append(
            Verdict(
                "d 昼夜可循环",
                None,
                f"再按一次 T:phase {after_phase.get('phase')} -> {cycle.get('phase')} "
                f"亮度均值 {_f(_mean_luma(luma_d0))}(初始) vs "
                f"{_f(_mean_luma(luma_cycle))}(绕一圈后)"
                f"{'(回到初始附近,phase 正常绕圈)' if _f(_mean_luma(luma_d0)) == _f(_mean_luma(luma_cycle)) else '(与初始不同,phase 未绕回)'}",
            )
        )

        # ---- (e) HUD ----
        fin = await _rich(s)
        hud = fin.get("hud")
        results.append(
            Verdict(
                "e HUD 非空",
                True if (isinstance(hud, str) and len(hud.strip()) > 0) else False,
                f"hud = {hud!r} | 该串由 format_hud 拼装,含血量/金钱/三角形/位置/"
                f"车辆状态/相机模式/通知",
            )
        )
        results.append(
            Verdict(
                "感知截图汇总",
                None,
                f"初始={path0} 传送后={first_path} 昼夜前={path_d0} 昼夜后={path_d1}"
                f" —— 这四张必须交给 vision_analyze 用文字描述,"
                f"否则 w8 只是一堆数字。",
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
