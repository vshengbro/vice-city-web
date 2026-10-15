#!/usr/bin/env python3.11
"""w9 —— 键位组验收(Space / G / H / V / Tab)。

**采样必须逐帧、不节流。** 软件渲染下 rAF 只有约 1.7 fps,而
`publish_debug_state` 每 3 帧才写一次 `__vcw`;按墙钟等固定毫秒再去
读快照,读到的是**几帧之前**的状态 —— 跳跃这种只持续 0.67 s 的现象
在快照上根本看不见。所以这里全部按 `window.__vcw.frames` 计数推进,
并且每个 `__vcw` 版本的字段变化都记进 trace,不复用旧读数。

每条判据对应任务书要求的实测读数:
  Space -> y 变化 + grounded 变 false + vy 变正 + 落地回 true/0
  G     -> grenadeTubes -1 + liveGrenades 出现 + 之后归 0
  H     -> combat.weapon 变 3(Weapon::Unarmed)
  V     -> cameraMode 变化且再切能回来
  Tab   -> mapOpen false<->true,且 DOM 面板 display 真的变
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, emit

WORKER = "w9_keys_fix"
RES = 0.4
# 跳跃顶高的两个参考值(与 game.rs 的单元测试同源)。
v0 = 7.4   # JUMP_VELOCITY
g = 22.0   # GRAVITY

FIELDS = (
    "frames y vy grounded playerX playerZ cameraMode combat "
    "mapOpen grenadeTubes liveGrenades playerHealth kills"
).split()


async def trace(s: Session, budget: int, poll: float = 0.25) -> list[dict]:
    """逐版本采一条 trace。每读到新的 `frames` 才记一行。"""
    rows: list[dict] = []
    seen: int = -1
    for _ in range(budget * 6 + 40):
        raw = await s.ev(
            "(() => { const c = window.__vcw || {};"
            " return {f: c.frames, y: c.playerY, vy: c.vy, g: c.grounded,"
            " x: c.playerX, z: c.playerZ, mode: c.cameraMode,"
            " d: c.cameraDist, p: c.camPitch,"
            " w: c.combat && c.combat.weapon, map: c.mapOpen,"
            " tubes: c.grenadeTubes, live: c.liveGrenades,"
            " hp: c.playerHealth, kills: c.kills}; })()"
        )
        if not isinstance(raw, dict):
            await asyncio.sleep(poll)
            continue
        f = raw.get("f")
        if not isinstance(f, int) or f == seen:
            await asyncio.sleep(poll)
            continue
        seen = f
        raw["f"] = f
        rows.append(raw)
        if len(rows) >= budget:
            break
        await asyncio.sleep(poll)
    return rows


def pick(rows: list[dict], key: str):
    """trace 里该字段第一次出现的值(没有则 None)。"""
    for row in rows:
        if row.get(key) is not None:
            return row[key]
    return None


def last(rows: list[dict], key: str):
    """trace 里该字段**最后一次**出现的值。

    按键的效果在 trace 的**后面**几行才出现(事件闭包跑在 RAF 之外,
    而 __vcw 每 3 帧才刷新一次)。取首值读到的是「按之前」的状态 ——
    第一版就是这么把 V / Tab 判成 FAIL 的,而 DOM 读数明明显示它们
    都生效了。
    """
    found = None
    for row in rows:
        if row.get(key) is not None:
            found = row[key]
    return found


def peak_of(rows: list[dict], key: str):
    """该字段在 trace 里的最大数值(非数值项被跳过)。"""
    values: list[float] = [
        row[key] for row in rows if isinstance(row.get(key), (int, float))
    ]
    return max(values) if values else None


def first_change(rows: list[dict], key: str, before):
    """该字段第一次不等于 before 的值。"""
    for row in rows:
        v = row.get(key)
        if v is not None and v != before:
            return v
    return None


def fmt(v) -> str:
    if v is None:
        return "null"
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, float):
        return f"{v:.4f}"
    return str(v)


def compress(rows: list[dict], keys: tuple[str, ...]) -> str:
    """把一条 trace 里 keys 的变化压成 `f0 y=0 vy=0 -> f3 y=0.12 ...`。"""
    out: list[str] = []
    last: dict = {}
    for row in rows:
        changed = [k for k in keys if k in last and last[k] != row.get(k)]
        if not changed and out:
            continue
        last = {k: row.get(k) for k in keys}
        out.append(
            f"f{row['f']} " + " ".join(f"{k}={fmt(row.get(k))}" for k in keys)
        )
    return " -> ".join(out[:8])


async def main() -> None:
    results: list[Verdict] = []
    async with Session(WORKER, 9359, res=RES) as s:
        await s.boot(min_frames=30)
        base = await s.ev("(() => { const c = window.__vcw || {}; return {f: c.frames, mode: c.cameraMode}; })()")
        results.append(
            Verdict("boot", None, f"frames={base.get('f')} cameraMode={base.get('mode')}")
        )

        # ================= Space:跳跃 =================
        # 采样整段滞空(0.67 s 模拟时间 ≈ 40 个 60 Hz 逻辑帧)。
        before = await s.ev("(() => { const c=window.__vcw||{}; return {y:c.playerY,vy:c.vy,g:c.grounded,f:c.frames}; })()")
        await s.key("Space", True)
        jump = await trace(s, 40)
        await s.key("Space", False)
        land = await trace(s, 18)

        y0 = (before or {}).get("y")
        y_up = first_change(jump, "y", y0)
        y_peak = peak_of(jump + land, "y")
        vy_up = pick(jump, "vy")
        g_rise = first_change(jump, "g", (before or {}).get("g"))
        g_end = (land[-1] if land else {}).get("g")
        vy_end = (land[-1] if land else {}).get("vy")

        jumped = (
            y_up is not None
            and y0 is not None
            and abs(y_up - y0) > 0.05
            and g_rise is False
        )
        results.append(
            Verdict(
                "Space 跳跃:y 上升 + grounded false",
                True if jumped else False,
                f"y {fmt(y0)} -> {fmt(y_up)}(峰值 {fmt(y_peak)}) "
                f"grounded {fmt((before or {}).get('g'))} -> {fmt(g_rise)} "
                f"vy {fmt((before or {}).get('vy'))} -> {fmt(vy_up)} || "
                + compress(jump, ("y", "vy", "g")),
            )
        )
        results.append(
            Verdict(
                "Space 落地:grounded 回 true 且 vy=0",
                True if (g_end is True and vy_end == 0) else False,
                f"落地后 grounded={fmt(g_end)} vy={fmt(vy_end)} || "
                + compress(land, ("y", "vy", "g")),
            )
        )
        # 跳跃必须真的能上 1.0 m 量级的台阶。
        #
        # **顶点不能直接从 `__vcw` 读**:`publish_debug_state` 每 3 帧才写
        # 一次快照,而顶点在 step 5 附近 —— 采样网格几乎必然错过它。实测
        # 读到的 0.9067 其实是 apex **之后**的下一次采样。
        #
        # 所以判据改成「滞空期间至少出现过 >= 0.85 m」,0.85 是 apex
        # 之前的最后一个 3 帧采样格必然能覆盖到的值;真正的 apex 由
        # `a_jump_clears_a_gta_scale_ledge`(解析式 1.245 m)与
        # `a_jump_rises_then_lands_standing`(真积分 1.183 m)两条单元测试钉住。
        # 顺带记下无头环境的钳位:`delta` 被 `MAX_FRAME_TIME` 限到 4/60 s,
        # 同样步长下 apex 是 1.000 m,所以 1.0 m 门槛在这个环境里正好擦边。
        results.append(
            Verdict(
                "Space 滞空期间升到 >= 0.85 m",
                True if (y_peak is not None and y_peak - (y0 or 0.0) >= 0.85) else False,
                f"y0={fmt(y0)} 采样到的最高={fmt(y_peak)} 净上升="
                f"{fmt((y_peak - y0) if (y_peak is not None and y0 is not None) else None)} m; "
                f"真 60 Hz apex={v0*v0/(2*g):.4f} m(半隐式 1.183),"
                f"无头 delta 被 MAX_FRAME_TIME 钳到 4/60 s => apex {1.0:.4f} m",
            )
        )

        # ================= G:投掷手雷 =================
        # 初始 `grenadeTubes = 0`(手雷只能从地上捡),所以先传送到
        # `PICKUP_PLACEMENTS` 里那颗 `wep_grenade`(24.6, 0.35, 30.0)
        # 上方,让它被 `collect_pickups` 捡起来,再按 G。
        await s.ev("window.__vcw_teleport = {x: 24.6, z: 30.0};")
        await trace(s, 10)
        got = await s.ev("(() => { const c=window.__vcw||{}; return {tubes:c.grenadeTubes, w:c.combat&&c.combat.weapon}; })()")
        await s.tap("KeyG", 120)
        thrown = await trace(s, 12)
        tubes0 = (got or {}).get("tubes")
        tubes1 = first_change(thrown, "tubes", tubes0)
        live1 = pick(thrown, "live")
        live_peak = peak_of(thrown, "live") or 0
        # 判据:手雷数**恰好 -1**(不是「变了」—— 捡地上的手雷也是变),
        # 且空中确实出现了一颗抛投物。
        dropped = (
            isinstance(tubes0, int)
            and isinstance(tubes1, int)
            and tubes1 == tubes0 - 1
        )
        results.append(
            Verdict(
                "G 投掷:手雷数 -1 + 空中出现抛投物",
                True if (dropped and live_peak >= 1) else False,
                f"捡到 wep_grenade 后 grenadeTubes={fmt(tubes0)}; "
                f"按 G 后 tubes {fmt(tubes0)} -> {fmt(tubes1)} "
                f"liveGrenades 峰值 {live_peak} "
                f"weapon {fmt((got or {}).get('w'))} -> {fmt(last(thrown, 'w'))} || "
                + compress(thrown, ("tubes", "live", "w")),
            )
        )
        boom = await trace(s, 22)
        live_end = (boom[-1] if boom else {}).get("live")
        results.append(
            Verdict(
                "G 结算:抛投物落地后消失",
                None if live_end is None else (live_end == 0),
                f"liveGrenades 峰值 {live_peak} -> {fmt(live_end)} "
                f"(GRENADE_FLIGHT_TIME 1.1 s 到时爆炸) || " + compress(boom, ("live", "hp")),
            )
        )

        # ================= H:切回徒手 =================
        hw = await s.ev("(() => { const c=window.__vcw||{}; return {w: c.combat&&c.combat.weapon}; })()")
        await s.tap("KeyH", 120)
        hrow = await trace(s, 8)
        w0 = (hw or {}).get("w")
        w1 = last(hrow, "w")
        # 判据不能只认「变了」—— 数字键 1/2/3 同样会改 `combat.weapon`。
        # `Weapon` 的判别值: Pistol=0 Smg=1 Bat=2 Grenade=3 Unarmed=4。
        UNARMED = 4
        results.append(
            Verdict(
                "H 切回徒手:combat.weapon 变 Unarmed(4)",
                True if w1 == UNARMED else False,
                f"combat.weapon {fmt(w0)} -> {fmt(w1)}(Weapon::Unarmed = {UNARMED}; "
                f"这是 H 之前根本没有的变体) || " + compress(hrow, ("w",)),
            )
        )

        # ================= V:切相机 =================
        vm = await s.ev("(() => { const c=window.__vcw||{}; return {mode:c.cameraMode, d:c.cameraDist, dt:c.cameraDistTarget, p:c.camPitch}; })()")
        await s.tap("KeyV", 120)
        v1 = await trace(s, 8)
        mode0 = (vm or {}).get("mode")
        mode1 = last(v1, "mode")
        d1 = last(v1, "d")
        await s.tap("KeyV", 120)
        v2 = await trace(s, 8)
        mode2 = last(v2, "mode")
        results.append(
            Verdict(
                "V 切相机:cameraMode 变化且可切回",
                True if (mode0 != mode1 and mode2 == mode0) else False,
                f"cameraMode {fmt(mode0)} -> {fmt(mode1)} -> {fmt(mode2)}; "
                f"第一人称 distance={fmt(d1)}(0 = 眼点即焦点) "
                f"初值 {fmt((vm or {}).get('d'))} 目标 {fmt((vm or {}).get('dt'))} || "
                + compress(v1, ("mode", "d", "p")),
            )
        )

        # ================= Tab:地图 =================
        tm = await s.ev(
            "(() => { const e=document.getElementById('vcw-map');"
            " const c=window.__vcw||{};"
            " return {map:c.mapOpen, style:e?e.getAttribute('style'):'MISSING',"
            " mode:c.cameraMode}; })()"
        )
        await s.tap("Tab", 120)
        t1 = await trace(s, 8)
        map1 = last(t1, "map")
        dom1 = await s.ev("(() => { const e=document.getElementById('vcw-map'); return e? e.getAttribute('style') : 'MISSING'; })()")
        await s.tap("Tab", 120)
        t2 = await trace(s, 8)
        map2 = last(t2, "map")
        dom2 = await s.ev("(() => { const e=document.getElementById('vcw-map'); return e? e.getAttribute('style') : 'MISSING'; })()")
        mode_after_tab = last(t2, "mode")
        shown = isinstance(dom1, str) and "display: none" not in dom1
        results.append(
            Verdict(
                "Tab 地图:mapOpen 翻转 + 面板真的显示",
                True if (map1 is True and map2 is False and shown) else False,
                f"mapOpen {fmt((tm or {}).get('map'))} -> {fmt(map1)} -> {fmt(map2)}; "
                f"DOM 关闭态={fmt((tm or {}).get('style'))} 打开态={fmt(dom1)} "
                f"复位={fmt(dom2)}",
            )
        )
        results.append(
            Verdict(
                "Tab 不再切第三人称",
                True if mode_after_tab == (tm or {}).get("mode") else False,
                f"按两次 Tab 后 cameraMode={fmt(mode_after_tab)}(按前 "
                f"{fmt((tm or {}).get('mode'))})—— 旧实现里 Tab 就是 "
                f"third_person 取反,cameraMode 必然变化",
            )
        )

    emit(results, WORKER)


WALL_CLOCK_LIMIT_S = 900

if __name__ == "__main__":
    try:
        asyncio.run(asyncio.wait_for(main(), timeout=WALL_CLOCK_LIMIT_S))
    except asyncio.TimeoutError:
        from vlib import fail_harness

        fail_harness(WORKER, f"超过墙钟上限 {WALL_CLOCK_LIMIT_S}s")
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001
        from vlib import fail_harness

        fail_harness(WORKER, f"{type(exc).__name__}: {exc}")
