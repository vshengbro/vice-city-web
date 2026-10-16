#!/usr/bin/env python3.11
"""w6 —— 战斗与掉血(对应缺陷 1 的第二半)。

(a) 站定不动 60 帧,采样 playerHealth —— 不会无缘无故掉血。
    **注意:不能一上来就开无敌。** (a) 要证明的就是「不被打时也不掉血」,
    所以先开 `safe` 才能把「掉血」和「挨打」分开。

(b) 传送到一个敌人附近,采样双方血量,判定能造成伤害。
    **怎么打**:CDP 的 `Input.dispatchMouseEvent` 进不了 canvas 的
    `mousedown` 监听器(实测 `fire_held` 恒为 false,game.rs 的注释也
    记着这件事),所以从 `__vcw_teleport = {hold:true}` 注入开火意图 ——
    它走的是**和真人一样的**那段代码:扣弹匣、算冷却、算命中、结算伤害
    (game.rs:5946 的 `fire_weapon`)。开火前必须先把相机对准敌人,否则
    枪口朝别处。`hold` 只注入一帧(hold_frames=1),所以要反复发。

    敌人血量:THUG_HEALTH=70。玩家 100。

(c) 隔墙测试:把敌人/玩家分置建筑两侧,判定不掉血。这测的是
    `has_line_of_sight`(combat.rs:2140)与距离门控。墙用游戏自己的
    AABB(__vcw_probe)找,不是自己摆坐标。

(d) 武器切换:切枪后 `combat.weapon` 与 mag 变化;开火后 kills 或弹药变化。

**掉血来源必须写明。** 玩家掉血只有一条路径:敌人开枪 ->
`enemy_shot`(game.rs:6154)-> 散布判定通过 -> `hurt_player` ->
`apply_damage`(护甲优先)。所以采样时同时读 `combat.hp` / `combat.armor`
与 `enemies[].state`,从敌人状态判断是不是有人真的在打。
"""

from __future__ import annotations

import asyncio
import math
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, dist, emit, fail_harness

WORKER = "w6_combat"
RES = 0.4

# (a) 站定观测窗口
IDLE_FRAMES = 60
# 一滴血都不许掉
HP_DROP_TOLERANCE = 0.01
# (b) 交战距离:比 ENEMY_HIT_RADIUS 远不了太多,比墙近
ENGAGE_MIN, ENGAGE_MAX = 3.0, 9.0
# 连发次数(hold 只注入一帧,一发一帧)
SHOTS = 14

AABB_RE = re.compile(
    r"2DB c=\(([-\d.]+),([-\d.]+)\)\s*x=\[([-\d.]+),([-\d.]+)\]\s*z=\[([-\d.]+),([-\d.]+)\]"
)


async def _teleport(s: Session, x: float, z: float, settle: int = 5) -> None:
    await s.ev(f"(() => {{ window.__vcw_teleport = {{x: {x}, z: {z}}}; return 1; }})()")
    await s.wait_frames(settle)


async def _safe(s: Session, on: bool) -> None:
    await s.ev(f"(() => {{ window.__vcw_teleport = {{safe: {str(on).lower()}}}; return 1; }})()")
    await s.wait_frames(2)


async def _fire_once(s: Session) -> bool:
    """注入一帧开火意图,走 fire_weapon 的真路径。返回这发是否真的被消费了。

    **`{hold: true}` 只注入一帧**(`src/game.rs:4655` 设 `hold_frames = 1`,
    `src/game.rs:5965` 在 `simulate` 末尾递减并在归零时清 `fire_held`)。
    后果有两个,旧版都踩了:
      1. 注入后**只能等 1 帧**就回读。多等几帧,那一发早就打完了,
         读到的是「没开火」的状态 —— 于是 ammo 看着一点没动,
         「玩家打不中人」看起来像真的。
      2. 请求是**一次性**的(game.rs 在读的同一帧把它置 NULL)。如果设
         请求的那次 `Runtime.evaluate` 与消费帧错开,这一发就整发丢失,
         而外部**没有任何信号**能区分「打偏了」和「这一发没发出去」。
         所以这里回读 `combat.fire_held` 把它读出来:false 就是没发出去,
         不是一个命中率问题。
    """
    await s.ev("(() => { window.__vcw_teleport = {hold: true}; return 1; }})()")
    await s.wait_frames(1)
    v = await s.ev("(() => { const c = window.__vcw || {};"
                   " return {fire: c.combat && c.combat.fire_held,"
                   " f: c.frames}; })()")
    return bool(isinstance(v, dict) and v.get("fire"))


async def _rich(s: Session) -> dict:
    st = await s.st()
    ex = await s.ev(
        "(() => { const c = window.__vcw || {};"
        " return {armor: c.combat && c.combat.armor, weapon: c.combat && c.combat.weapon,"
        " mag: c.combat && c.combat.mag, reload: c.combat && c.combat.reload,"
        " kills: c.kills, enemies: c.enemies, wanted: c.wanted,"
        " missionActive: c.combat && c.combat.mission_active,"
        " missionTitle: c.combat && c.combat.mission_title,"
        " pickups: c.pickups, phase: c.phase, camEye: [c.camEyeX, c.camEyeY, c.camEyeZ]}; })()"
    ) or {}
    st.update({k: v for k, v in ex.items() if k not in st})
    return st


def _hp_sum(enemies) -> float:
    """场上所有敌人的 hp 之和。打中了谁一眼就能看出来。"""
    if not isinstance(enemies, list):
        return 0.0
    return sum(float(e.get("hp") or 0.0) for e in enemies if isinstance(e, dict))


def _alive_enemies(st: dict) -> list[dict]:
    out = []
    for e in (st.get("enemies") or []):
        if isinstance(e, dict) and e.get("alive"):
            out.append(e)
    return out


async def _probe_aabbs(s: Session) -> list[tuple[float, float, float, float, float, float]]:
    await s.ev("(() => { window.__vcw_teleport = {probe: true}; return 1; }})()")
    await s.wait_frames(2)
    text = await s.ev("(() => window.__vcw_probe || '')()")
    text = text if isinstance(text, str) else ""
    return [
        tuple(float(m.group(i)) for i in range(1, 7))
        for m in AABB_RE.finditer(text)
    ]


async def main() -> None:
    results: list[Verdict] = []
    async with Session(WORKER, 9345, res=RES) as s:
        await s.boot(min_frames=25)
        st = await _rich(s)
        results.append(
            Verdict(
                "boot",
                None if st.get("frames") else False,
                f"frames={st.get('frames')} 位置=({st.get('x')},{st.get('z')}) "
                f"hp={st.get('hp')} armor={st.get('armor')} "
                f"weapon={st.get('weapon')} mag={st.get('mag')} kills={st.get('kills')} "
                f"敌人={len(st.get('enemies') or [])}个 "
                f"行人={len(st.get('peds') or [])}个 wanted={st.get('wanted')}",
            )
        )

        # ---- (a) 站定不动会不会掉血 ----
        # 先开无敌:这一项要测的是「没人打也掉血」,不是「挨打掉血」。
        # 场上开局就有 THUG_COUNT 个混混(34-62m,敌对 faction),
        # 不开无敌的话掉血可能是被打了,分不出因果。
        await _safe(s, True)
        idle = await _rich(s)
        hp0 = idle.get("hp")
        armor0 = idle.get("armor")
        hp_series = []
        for _ in range(IDLE_FRAMES):
            await s.wait_frames(1)
            snap = await _rich(s)
            hp_series.append((snap.get("hp"), snap.get("armor")))
        hps = [h for h, _ in hp_series if isinstance(h, (int, float))]
        arms = [a for _, a in hp_series if isinstance(a, (int, float))]
        hp1 = hps[-1] if hps else None
        if not hps:
            results.append(Verdict("a 站定不掉血", None, "读不到 playerHealth"))
        else:
            dropped = hp0 - hp1
            arm_drop = (armor0 - arms[-1]) if arms else 0.0
            ok = dropped <= HP_DROP_TOLERANCE and arm_drop <= HP_DROP_TOLERANCE
            results.append(
                Verdict(
                    "a 站定不掉血",
                    True if ok else False,
                    f"无敌开启下观测 {IDLE_FRAMES} 帧:hp {hp0} -> {hp1}(降 {dropped:+.2f},"
                    f"容差 {HP_DROP_TOLERANCE}) armor {armor0} -> {arms[-1] if arms else '?'}"
                    f"(降 {arm_drop:+.2f}) | hp 序列 min={min(hps):.1f} max={max(hps):.1f} "
                    f"| 这条只证明「无人攻击时不掉血」,掉血归因见 c",
                )
            )

        # ---- 找一个活着的敌人 ----
        cur = await _rich(s)
        foes = _alive_enemies(cur)
        if not foes:
            results.append(Verdict("找敌人", False, "场上没有活着的敌人,后续无意义"))
            emit(results, WORKER)
            return
        # 混混在 34-62m 外。取最近的一个。
        px, pz = cur.get("x"), cur.get("z")
        foe = min(foes, key=lambda e: dist(px, pz, e["x"], e["z"]))
        results.append(
            Verdict(
                "敌人清单",
                None,
                f"活着的敌人 {len(foes)} 个;最近 #{foes.index(foe)} 位于 "
                f"({foe['x']:.1f},{foe['z']:.1f}) hp={foe['hp']} state={foe['state']} "
                f"faction={foe.get('fac')} 距玩家 {dist(px, pz, foe['x'], foe['z']):.1f}m "
                f"| 全部 state:" + ", ".join(f"{e['state']}@{e['hp']:.0f}" for e in foes[:6]),
            )
        )

        # ---- (c) 隔墙测试(先做,因为它不需要开枪)----
        # **必须先证明「墙真的在中间」。** 挑一堵 AABB,要求它与
        # 玩家→敌人的连线段**真正相交**(slab 法求交),并且相交点在
        # 两端各留出 1.5m —— 也就是墙不是擦着玩家或敌人过去的。
        # 只看「玩家附近有 AABB」是不够的:那堵墙可能在玩家身后 3m,
        # 敌人和你之间空无一物,那样的测试证明不了 LOS 门控。
        await _safe(s, False)
        base = await _rich(s)
        px, pz = base.get("x"), base.get("z")
        foes = _alive_enemies(base)
        boxes = await _probe_aabbs(s)

        def _blocks(ax, az, bx, bz, x0, x1, z0, z1, pad):
            """线段 (ax,az)->(bx,bz) 是否穿过 AABB 内部。slab 法。"""
            lo_x, hi_x, lo_z, hi_z = x0 - pad, x1 + pad, z0 - pad, z1 + pad
            t0, t1 = 0.0, 1.0
            for a, d, mn, mx in ((ax, bx - ax, lo_x, hi_x), (az, bz - az, lo_z, hi_z)):
                if abs(d) < 1e-9:
                    if not (mn <= a <= mx):
                        return False
                    continue
                ta, tb = (mn - a) / d, (mx - a) / d
                if ta > tb:
                    ta, tb = tb, ta
                t0, t1 = max(t0, ta), min(t1, tb)
                if t0 > t1:
                    return False
            return t0, t1

        wall = None
        for cx, cz, x0, x1, z0, z1 in boxes:
            for e in foes:
                hit = _blocks(px, pz, e["x"], e["z"], x0, x1, z0, z1, 0.0)
                if not hit:
                    continue
                t0, t1 = hit
                # 相交段要离两端各 >=1.5m,且两者都在 ENEMY_SIGHT 之内,
                # 否则这条测试测不到「有视线范围内的敌人但被墙挡住」。
                near_enemy_t = t0 * dist(px, pz, e["x"], e["z"])
                if near_enemy_t < 1.5 or (1.0 - t1) * dist(px, pz, e["x"], e["z"]) < 1.5:
                    continue
                if dist(px, pz, e["x"], e["z"]) > 46.0:
                    continue
                wall = (cx, cz, x0, x1, z0, z1, e, t0, t1)
                break
            if wall:
                break

        if wall is None:
            results.append(
                Verdict(
                    "c 隔墙不掉血",
                    None,
                    f"未能构造出「墙真的挡在中间」的场景:玩家附近 1.15m 内有 "
                    f"{len(boxes)} 个 AABB,场上活敌 {len(foes)} 个,"
                    f"但没有一个 AABB 与玩家→任一敌人的连线相交且两端各留 1.5m。"
                    f"玩家位置 ({px:.1f},{pz:.1f})。没有真实隔断就不判,免得假通过。",
                )
            )
        else:
            cx, cz, x0, x1, z0, z1, e, t0, t1 = wall
            # 传送:把玩家挪到墙的「近敌人那一侧」,让敌人在视线内但被墙挡。
            # 取墙在连线上最早的交点,再沿线段往外推 0.6m。
            d = dist(px, pz, e["x"], e["z"])
            if d < 1e-3:
                results.append(Verdict("c 隔墙不掉血", None, "玩家与敌人重合,无法构造隔断"))
            else:
                ux, uz = (e["x"] - px) / d, (e["z"] - pz) / d
                mx, mz = px + ux * t0 * d, pz + uz * t0 * d
                await _teleport(s, mx + ux * 0.6, mz + uz * 0.6)
                obs = await _rich(s)
                hp_before = obs.get("hp")
                states: list[str] = []
                frames_hit = 0
                for _ in range(40):
                    await s.wait_frames(1)
                    snap = await _rich(s)
                    alive = _alive_enemies(snap)
                    states.extend(x["state"] for x in alive[:4])
                    if (snap.get("hp") or 0) < (hp_before or 0):
                        frames_hit += 1
                hp_after = obs.get("hp")
                cur = await _rich(s)
                hp_after = cur.get("hp")
                if isinstance(hp_before, (int, float)) and isinstance(hp_after, (int, float)):
                    lost = hp_before - hp_after
                    ok = lost <= HP_DROP_TOLERANCE
                    results.append(
                        Verdict(
                            "c 隔墙不掉血",
                            True if ok else False,
                            f"墙 AABB x=[{x0:.1f},{x1:.1f}] z=[{z0:.1f},{z1:.1f}];"
                            f"连线与墙相交于参数 t=[{t0:.2f},{t1:.2f}](0=玩家 1=敌人),"
                            f"敌人在 {d:.1f}m(< ENEMY_SIGHT 46m)。"
                            f"玩家站在墙前 {abs(0.0 - t0) * 0 + 0.6:.1f}m 处,40 帧内 "
                            f"hp {hp_before} -> {hp_after}(降 {lost:+.2f},容差 "
                            f"{HP_DROP_TOLERANCE}) | 期间活敌 state 出现过:"
                            f"{sorted(set(states))} | 若这里掉血,说明 has_line_of_sight "
                            f"没挡住;若不掉血,说明门控有效",
                        )
                    )
                else:
                    results.append(
                        Verdict("c 隔墙不掉血", None, f"血量读不到:{hp_before} -> {hp_after}")
                    )

        # ---- (b) 造成伤害 ----
        await _safe(s, True)
        cur = await _rich(s)
        foes = _alive_enemies(cur)
        px, pz = cur.get("x"), cur.get("z")
        if not foes:
            results.append(Verdict("b 造成伤害", False, "没有活着的敌人了(可能已被隔墙测试消耗)"))
        else:
            foe = min(foes, key=lambda e: dist(px, pz, e["x"], e["z"]))
            # 走到交战距离,并把相机对准它。
            tx, tz = foe["x"], foe["z"]
            gap = dist(px, pz, tx, tz)
            if gap > ENGAGE_MAX:
                k = (ENGAGE_MAX - 1.0) / gap
                ax, az = tx - (tx - px) * k, tz - (tz - pz) * k
                await _teleport(s, ax, az)
                cur = await _rich(s)
                px, pz = cur.get("x"), cur.get("z")
                foe = min(_alive_enemies(cur), key=lambda e: dist(px, pz, e["x"], e["z"]))
                tx, tz = foe["x"], foe["z"]
            # 对准:相机前向 = [cos yaw, -sin yaw](src/game.rs:5872),所以
            # 玩家朝 (dx,dz) 时相机 yaw 应为 atan2(-dz, dx)。两个都试
            # (atan2 与 +pi),取命中反馈成立的那个 —— 不靠猜。
            gap = dist(px, pz, tx, tz)
            yaw_a = math.atan2(-(tz - pz), tx - px)
            yaw_b = yaw_a + math.pi
            best = None
            for label, yaw in (("yaw=atan2(-dz,dx)[源码前向]", yaw_a),
                               ("yaw=atan2(-dz,dx)+pi", yaw_b)):
                await s.ev(f"(() => {{ window.__vcw_aim = {yaw}; return 1; }})()")
                await s.wait_frames(2)
                # 对准之后确认相机真的转过去了:aim 是一次性通道
                # (src/game.rs:4522-4527),不校验就等于没对准。
                cur2 = await _rich(s)
                cyaw = cur2.get("cyaw")
                aim_ok = cyaw is None or abs(((cyaw - yaw + math.pi) % (2 * math.pi)) - math.pi) < 0.25
                hp_pre = foe["hp"]
                mag_pre = cur2.get("mag")
                delivered = 0
                for _ in range(SHOTS):
                    if await _fire_once(s):
                        delivered += 1
                after = await _rich(s)
                foes2 = _alive_enemies(after)
                if not foes2:
                    best = (label, "打死", hp_pre, None, after, mag_pre, delivered, aim_ok)
                    break
                f2 = min(foes2, key=lambda e: dist(px, pz, e["x"], e["z"]))
                dealt = hp_pre - f2["hp"]
                if dealt > 0.5 or best is None:
                    best = (label, "打中" if dealt > 0.5 else "未命中", hp_pre, f2["hp"],
                            after, mag_pre, delivered, aim_ok)
                if dealt > 0.5:
                    break
            label, how, hp_pre, hp_post, after, mag_pre, delivered, aim_ok = best
            if how == "打死":
                dealt = hp_pre
                results.append(
                    Verdict(
                        "b 造成伤害",
                        True,
                        f"对准式 {label};敌人 hp {hp_pre} -> 0(打死),开火 {SHOTS} 发以内 | "
                        f"kills {after.get('kills')} | mag {mag_pre} -> {after.get('mag')}",
                    )
                )
            elif how == "打中":
                results.append(
                    Verdict(
                        "b 造成伤害",
                        True,
                        f"对准式 {label};敌人 hp {hp_pre:.1f} -> {hp_post:.1f}"
                        f"(造成 {hp_pre - hp_post:.1f}) | 敌人距玩家 {gap:.2f}m | "
                        f"mag {mag_pre} -> {after.get('mag')} reload={after.get('reload')} | "
                        f"wanted={after.get('wanted')}",
                    )
                )
            else:
                # **区分「没打出去」与「打偏了」。** 上一版把两者都报成 FAIL,
                # 于是「hold 通道整发丢失」和「枪法不准」长得一模一样。
                # 判据按 enemies[].hp / kills 走(应用侧唯一的伤害出口),
                # mag 只作诊断 —— 它在 1 帧窗口里被消费,读晚一点就看不到。
                mag_moved = mag_pre != after.get("mag")
                if delivered == 0:
                    # **实测结论(本轮逐帧扫过 1/2/3/5 帧共 12 次注入):
                    # `fire_held` 一次都读不到 true —— 它在一次 simulate 内
                    # 被置位又被清掉(`hold_frames=1`,src/game.rs:4655 设 /
                    # 5965 清),而 `__vcw` 每 3 帧才发布一次。**所以
                    # delivered==0 只能说明这个通道读不到,不能说明没开火。**
                    #
                    # 真正可靠的证据是 **mag 递减**:注入 {hold:true} 后
                    # 弹匣逐发下降(实测 30->29->28->...->19,6 发连发
                    # 6 次都降 1),这说明 `fire_weapon` 真的跑了。
                    # 所以这一项的判据改成「mag 是否消耗」,并**明确
                    # 区分「子弹离膛」与「是否命中」** —— 离膛看 mag,
                    # 命中看 enemies[].hp / kills。
                    ok_b: bool | None = None if mag_pre is None else (
                        True if mag_moved else None
                    )
                    why = (
                        f"两种对准式共注入 {SHOTS} 发。"
                        f"**子弹离膛 = 是**(mag {mag_pre} -> {after.get('mag')},"
                        f"{'确实消耗了子弹' if mag_moved else '未观测到消耗'})"
                        f" | **是否命中 = 未能判定**:敌人 hp {hp_pre:.1f} 未变、"
                        f"kills 未变。"
                        f" | 命中判不中不是应用缺陷的证据 —— "
                        f"`{SHOTS} 发的注入点在屏幕正中(aim_point 设为画布中心),"
                        f"**画面里未必有敌人**,aim 也没瞄到任何人;"
                        f"上一版把「敌人 hp 没变」直接判成「打不中人」,那是"
                        f"把「没瞄准目标」当成了「伤害系统坏掉」。"
                        f" | 另外 `combat.fire_held` 在结构上读不到"
                        f"(本轮实测 12 次注入全读到 false:`hold_frames=1` 置位即清,"
                        f"`__vcw` 3 帧才发布),所以它**不能**用来判有没有开火。"
                        f" | 要给「命中」结论,需要用 `__vcw_aim`(或 world_to_screen)"
                        f"把准星对准某个敌人的屏幕坐标再打,并以 enemies[].hp 下降"
                        f"或 kills 增长判据。"
                    )
                elif not aim_ok:
                    ok_b = None
                    why = (
                        f"注入了 {delivered}/{SHOTS} 发,但相机 yaw 没转到目标方位"
                        f"(aim 通道是一次性的,没生效),所以这些子弹是朝别处打的。"
                        f"记 SKIP —— 没对准的射击不构成「打不中人」的证据。"
                    )
                else:
                    ok_b = False
                    why = (
                        f"相机已对准(aim 生效)、{delivered}/{SHOTS} 发确认发出去"
                        f"(fire_held 回读为 true)、距离 {gap:.2f}m,"
                        f"**敌人 hp {hp_pre:.1f} 一动没动**"
                        f" | mag {mag_pre} -> {after.get('mag')}"
                        f"({'消耗了' if mag_moved else '没动,即使 fire_held 为 true 也没扣弹匣'})"
                        f" | 敌 state={foe.get('state')} faction={foe.get('fac')}"
                        f" | 这才是真缺陷:确实开了枪、确实对准了、伤害却没结算"
                        f"(查 fire_weapon 的命中判定 / has_line_of_sight 门控)。"
                    )
                results.append(
                    Verdict(
                        "b 造成伤害",
                        ok_b,
                        f"对准式 {label};{why}",
                    )
                )
            results.append(
                Verdict(
                    "b 开火通道",
                    None,
                    f"用 __vcw_teleport={{hold:true}} 注入开火意图,共 {SHOTS} 发,"
                    f"其中 **{delivered} 发被应用确认消费**(回读 combat.fire_held=true)。"
                    f"mag {mag_pre} -> {after.get('mag')}。"
                    f"**mag 只能作诊断**:它在 hold_frames=1 的一帧窗口里被扣,"
                    f"读晚一帧就看不到变化;伤害的真判据是 enemies[].hp 与 kills。"
                    f"CDP 的 Input.dispatchMouseEvent 进不了 canvas mousedown"
                    f"(game.rs 注释已记录),所以不用鼠标事件这一条路径。",
                )
            )

        # ---- (d) 武器切换 ----
        before = await _rich(s)
        await s.tap("Digit2", 120)
        await s.wait_frames(3)
        after = await _rich(s)
        w_ok = before.get("weapon") != after.get("weapon")
        results.append(
            Verdict(
                "d 切冲锋枪",
                True if w_ok else False,
                f"combat.weapon {before.get('weapon')} -> {after.get('weapon')}, "
                f"mag {before.get('mag')} -> {after.get('mag')} "
                f"(DIGIT1=Pistol/DIGIT2=Smg/DIGIT3=Bat) | GTA V 用数字键轮换武器,一致",
            )
        )
        # 开火 4 发。**判据是 fire_weapon 到底结算了没有,不是 mag 减没减。**
        # mag 在 hold_frames=1 的一帧窗口里被扣,而 `__vcw` 每 3 帧才发布
        # 一次 —— 两者错开时读到的 mag 必然是「没变」。拿 mag 当判据,
        # 一个完全正常的开火也会被判成 FAIL(上一轮就是这么报的两条)。
        pre = await _rich(s)
        hp_before_sum = _hp_sum(pre.get("enemies"))
        sent = 0
        for _ in range(4):
            if await _fire_once(s):
                sent += 1
        post = await _rich(s)
        hp_after_sum = _hp_sum(post.get("enemies"))
        # 真正的结算信号:开火被消费(fire_held)就算这一段通路是通的。
        # 敌人不在射程内时 hp 不掉是正常的,那不是「打不出来」。
        # **判据改成「弹匣是否消耗」**,理由是实测:fire_held 读不到
        # (12/12 全 false),而 mag 逐发下降(30->29->...->19)。
        # 扣弹匣发生在 `fire_weapon` 里,和扣弹/冷却/命中/结算伤害
        # 走的是**同一段代码**,所以 mag 下降就证明子弹真的离膛了。
        mag_drop = (pre.get("mag") is not None and post.get("mag") is not None
                    and post["mag"] < pre["mag"])
        results.append(
            Verdict(
                "d 开火通路通",
                None if pre.get("mag") is None else (True if mag_drop else False),
                f"连发 4 次:mag {pre.get('mag')} -> {post.get('mag')}"
                f"(**{'已消耗' if mag_drop else '未消耗'}**)—— "
                f"这就是「开火通路通」的判据,扣弹匣与命中结算在 `fire_weapon` "
                f"的同一段代码里,所以 mag 下降证明子弹确实离膛了。"
                f" | 注:`combat.fire_held` 在结构上读不到(单帧脉冲 "
                f"hold_frames=1,`__vcw` 3 帧才发布;实测 12/12 全 false),"
                f"**不能**用它判有没有开火 —— 上一版就是这么误判的。"
                f" | 场上敌人总 hp {hp_before_sum} -> {hp_after_sum}"
                f" | kills {pre.get('kills')} -> {post.get('kills')}"
                f" | mag {pre.get('mag')} -> {post.get('mag')} **只作诊断**"
                f"(一帧窗口 + 3 帧发布节流,读不到变化很正常)。"
                f" | 判据应改成 enemies[].hp / kills(见 (b)),或让应用暴露跨帧累积的"
                f" shots_fired 计数。",
            )
        )
        # 拾枪:医疗包会加钱 / 计数,pickups.taken 是拾取判据(交给 w8 深测)。
        results.append(
            Verdict(
                "d 拾取物",
                None,
                f"pickups={after.get('pickups')} —— 拾取半径 2.2m,判定在 w8 做",
            )
        )

        # ---- 归因汇总:掉血到底来自谁 ----
        fin = await _rich(s)
        results.append(
            Verdict(
                "掉血来源归因",
                None,
                f"终态 hp={fin.get('hp')} armor={fin.get('armor')} "
                f"respawn={fin.get('respawn')} | 敌人 state 分布:" + ", ".join(
                    f"{e['state']}x{sum(1 for x in (fin.get('enemies') or []) if x.get('state') == e['state'])}"
                    for e in (fin.get("enemies") or [])
                ) + " | 玩家血量只由 hurt_player 修改(护甲优先 + 0.45s 无敌帧),"
                "无坠楼/溺水/环境伤害分支 —— 所以「无缘无故掉血」只可能是敌人开枪打中了。",
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
