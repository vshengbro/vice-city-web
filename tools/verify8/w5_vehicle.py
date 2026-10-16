#!/usr/bin/env python3.11
"""w5 —— 上下车 + 驾车(对应缺陷 5)。

(a) 传送到最近一辆车旁 -> 按 F -> playerVehicle 变非 null
(b) 上车后按 W,变的是 **carDriveX / carDriveZ / carDriveSpeed**,不是
    playerX/playerZ 的自由移动(上车后 player 位置是被动的,跟着车走)
(c) 按 F 下车,playerVehicle 回 null
(d) 车不穿建筑 —— 判定依据见下

**「车没穿楼」的判定依据(重要)。**
不能把「建筑 bounds 从 assets JSON 拿」当作直接判据:楼的世界位置来自
`build_city_buildings_with`(game.rs:1353),它用 `hash2`/`hash_unit` 伪随机、
`on_roadway` / `footprint_hits_roadway` / `hits_a_showcase` 三重剔除、还带
scale 与 ±0.06 rad 的 yaw 抖动。在 Python 里复刻这套摆放逻辑,等于把游戏
的随机数发生器抄一遍 —— 抄错了会得出「车在楼里」的错误结论,而真实原因
是脚本里的楼和游戏里的楼不是同一栋。

所以用游戏**自己**的碰撞世界作为唯一真源:`__vcw_teleport = {probe:true}`
触发 `probe_nearby_shapes`(game.rs:1229),它遍历 `game.world.get_shapes()`,
把玩家 1.15m 内的每个 AABB 打成
    `2DB c=(cx,cz) x=[x0,x1] z=[z0,z1] pen=(+,-)`
写进 `window.__vcw_probe`。那是碰撞解算真正在用的那份数据 —— 车被
`resolve_car` / 统一动态分离层推出去时,推的正是这批 shape。

判据:采样车的 (carX, carZ),对探针报出的每个 AABB 算
    inside_x = |cx - center_x| < half_x  且  inside_z 同理
车心同时落在两个轴内 = 车进入了建筑 AABB。允许半车宽的容差(车不是点),
所以改成「车心进入 AABB 收缩 0.5m 之后」判为穿模 —— 停在墙面上不算穿。
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

WORKER = "w5_vehicle"
RES = 0.4

ENTER_RADIUS = 4.2      # traffic.rs ENTER_VEHICLE_RADIUS
# 判定「穿楼」的收缩量(m)。车不是点,贴墙行驶是合法的,车心真的进到墙里
# 半米以上才算穿模。
CLIP_INSET = 0.5
# 位置门限:0.3m 以内算「车没动」—— 一帧 0.25s 内车至少该走一点。
MOVED_EPS = 0.3

# `2DB c=(1.00,2.00) x=[-6.0,7.0] z=[-5.0,7.0] pen=(+0.12,+0.30)`
AABB_RE = re.compile(
    r"2DB c=\(([-\d.]+),([-\d.]+)\)\s*x=\[([-\d.]+),([-\d.]+)\]\s*"
    r"z=\[([-\d.]+),([-\d.]+)\]\s*pen=\(([+-][\d.]+),([+-][\d.]+)\)"
)


async def _teleport(s: Session, x: float, z: float, settle: int = 6) -> None:
    await s.ev(f"(() => {{ window.__vcw_teleport = {{x: {x}, z: {z}}}; return 1; }})()")
    await s.wait_frames(settle)


async def _probe_shapes(s: Session, attempts: int = 6) -> tuple[str, list[dict]]:
    """读游戏自己的碰撞世界探针,**重发到真的拿到非空为止**。

    `__vcw_teleport` 是**一次性**请求:game.rs 在读它的**同一帧**就把它
    置成 NULL(`src/game.rs:4540`)。机器负载一重,设请求的那次
    `Runtime.evaluate` 就可能与消费帧错开,这一发整发丢失,于是
    `__vcw_probe` 读回来是空串。**空串与「附近确实没有碰撞体」在
    外部完全无法区分** —— 旧版就把这两种情况混为一谈,结果:
      - `d 撞击场景` 因为 boxes 为空而报 FAIL(其实根本没测到任何东西)
      - `d 车不穿建筑` 因为 boxes 为空而「通过」(bad==[] 恒成立,假通过)
    诊断实测这是**负载相关**的:同样的代码,一个端口 64 次探针全部为空,
    另一个端口 64 次全部有内容。所以这里改成重试,并且**把「试了几次
    仍然为空」如实报出来**,让上层能把它记成 SKIP 而不是 PASS/FAIL。
    """
    text = ""
    for _ in range(attempts):
        await s.ev("(() => { window.__vcw_teleport = {probe: true}; return 1; }})()")
        await s.wait_frames(2)
        raw = await s.ev("(() => window.__vcw_probe || '')()")
        text = raw if isinstance(raw, str) else ""
        if text.strip():
            break
    out = []
    for m in AABB_RE.finditer(text):
        cx, cz, x0, x1, z0, z1 = (float(m.group(i)) for i in range(1, 7))
        out.append({
            "center": (cx, cz),
            "half": ((x1 - x0) * 0.5, (z1 - z0) * 0.5),
            "box": (x0, x1, z0, z1),
        })
    return text, out


def _inside_aabbs(x: float, z: float, boxes: list[dict], inset: float) -> list[dict]:
    hit = []
    for b in boxes:
        hx, hz = b["half"]
        if hx - inset <= 0 or hz - inset <= 0:
            continue
        if abs(x - b["center"][0]) < hx - inset and abs(z - b["center"][1]) < hz - inset:
            hit.append(b)
    return hit


async def main() -> None:
    results: list[Verdict] = []
    # **进度面包屑。** 这个 worker 在 1.7 fps 下本来就要跑很久
    # (撞楼那步是 30 帧 x 2 个 yaw 的循环),墙钟 1500s 一旦踩到,
    # 没有面包屑就完全不知道卡在哪一步 —— 上一轮只能靠猜。
    step = lambda name: print(f"[w5] {name}", file=sys.stderr, flush=True)
    async with Session(WORKER, 9344, res=RES) as s:
        step('boot')
        await s.boot(min_frames=25)
        step('booted')
        st = await _st(s)
        results.append(
            Verdict(
                "boot",
                None if st.get("frames") else False,
                f"frames={st.get('frames')} 位置=({st.get('x')},{st.get('z')}) "
                f"collisionShapes={st.get('shapes')} playerVehicle={st.get('veh')}",
            )
        )

        # ---- (a) 找最近的车,传送到它旁边 ----
        step('find-cars')
        cars = await s.ev(
            "(() => { const c = window.__vcw || {};"
            " const out = [];"
            " for (let i = 0; i < (c.carX || []).length; i++) {"
            "   out.push({i, x: c.carX[i], z: c.carZ[i], s: (c.carSpeed||[])[i]}); }"
            " return out; })()"
        ) or []
        if not cars:
            results.append(Verdict("找车", False, "carX/carZ 为空:场上没有车,后续无意义"))
            emit(results, WORKER)
            return
        px, pz = st.get("x"), st.get("z")
        ranked = sorted(
            cars, key=lambda c: dist(px, pz, c["x"], c["z"]) if None not in (px, pz) else 1e9
        )
        near = ranked[0]
        results.append(
            Verdict(
                "车队清单",
                None,
                f"{len(cars)} 辆车;最近的是 #{near['i']} 位于 "
                f"({_f(near.get('x'), '.1f')},{_f(near.get('z'), '.1f')})"
                f" 速度 {_f(near.get('s'), '.1f')} m/s,"
                f"离玩家 {_f(dist(px, pz, near['x'], near['z']), '.1f')}m "
                f"| 其余最近 3 辆:" + ", ".join(
                    f"#{c['i']}@({_f(c.get('x'), '.0f')},{_f(c.get('z'), '.0f')})" for c in ranked[1:4]
                ),
            )
        )
        # 传送到车旁 2.5m(小于 ENTER_VEHICLE_RADIUS=4.2,留出余量)
        off = 2.5
        step('teleport-to-car')
        await _teleport(s, near["x"] + off, near["z"])
        step('at-car')
        at_car = await _st(s)
        gap = dist(at_car.get("x"), at_car.get("z"), near["x"], near["z"])
        results.append(
            Verdict(
                "传送到车旁",
                True if gap < ENTER_RADIUS else None,
                f"玩家 ({_f(at_car.get('x'))},{_f(at_car.get('z'))}) 距车 "
                f"({near['x']:.1f},{near['z']:.1f}) {gap:.2f}m "
                f"(上车门限 {ENTER_RADIUS}m) | 传送后 inside={at_car.get('inside')}",
            )
        )

        # ---- (a2) 按 F 上车 ----
        step('tap-F')
        await s.tap("KeyF", 140)
        await s.wait_frames(4)
        in_car = await _st(s)
        veh = in_car.get("veh")
        boarded = veh is not None
        results.append(
            Verdict(
                "a 按 F 上车",
                True if boarded else False,
                f"playerVehicle {at_car.get('veh')} -> {veh} | "
                f"nearest_car 门限 {ENTER_RADIUS}m,当前车心 "
                f"({_f(in_car.get('carDriveX'))},{_f(in_car.get('carDriveZ'))}) "
                f"速度 {_f(in_car.get('carDriveSpeed'))} m/s "
                f"| cameraDist={in_car.get('camDist')}",
            )
        )

        if not boarded:
            results.append(
                Verdict("b 驾车", False, "没能上车,(b)(c) 全部不适用")
            )
            emit(results, WORKER)
            return

        # ---- (b) 按 W,车应该动 ----
        drive_before = await _st(s)
        await s.key("KeyW", True)
        drive_track = []
        for _ in range(10):
            await s.wait_frames(1)
            drive_track.append(await _st(s))
        await s.key("KeyW", False)
        await s.wait_frames(2)
        drive_after = await _st(s)

        xs = [t.get("carDriveX") for t in drive_track]
        zs = [t.get("carDriveZ") for t in drive_track]
        sps = [t.get("carDriveSpeed") for t in drive_track]
        if None in (drive_before.get("carDriveX"), drive_after.get("carDriveX"), *xs, *zs, *sps):
            results.append(Verdict(
                "b 按 W 驾车", None,
                f"carDrive* 读不到: before={_f(drive_before.get('carDriveX'))},"
                f" 序列 x={xs[:4]} —— playerVehicle={_f(drive_before.get('veh'))},"
                f" 读不到通常意味着没在车里(playerVehicle 为 null)"))
        else:
            car_travel = dist(drive_before.get("carDriveX"), drive_before.get("carDriveZ"),
                              drive_after.get("carDriveX"), drive_after.get("carDriveZ"))
            ped_travel = dist(drive_before.get("x"), drive_before.get("z"),
                              drive_after.get("x"), drive_after.get("z"))
            spd_grew = max(sps) - (drive_before.get("carDriveSpeed") or 0.0)
            ok = car_travel > MOVED_EPS and max(sps) > 0.5
            results.append(
                Verdict(
                    "b 按 W 驾车",
                    True if ok else False,
                    f"车位移 {_f(car_travel)}m(门限>{MOVED_EPS}) "
                    f"({_f(drive_before.get('carDriveX'))},{_f(drive_before.get('carDriveZ'))}) -> "
                    f"({_f(drive_after.get('carDriveX'))},{_f(drive_after.get('carDriveZ'))}) | "
                    f"carDriveSpeed 序列 {[round(v, 2) for v in sps]} "
                    f"峰值 {_f(max(sps))} m/s,增量 {_f(spd_grew, '+.2f')} | "
                    f"同期 player 位移 {_f(ped_travel)}m —— "
                    f"车动而 playerX 单独动,说明动力来自车而不是走路",
                )
            )
            results.append(
                Verdict(
                    "b 玩家被车带走",
                    True if ped_travel > 0.3 else None,
                    f"上车后 player 位置变化 {ped_travel:.2f}m;玩家坐标应等于车心:"
                    f"({_f(drive_after.get('x'))},{_f(drive_after.get('z'))}) vs 车心 "
                    f"({_f(drive_after.get('carDriveX'))},{_f(drive_after.get('carDriveZ'))})",
                )
            )

        # ---- (d) 车不穿建筑(用游戏自己的碰撞世界当判据)----
        probe_text, boxes = await _probe_shapes(s)
        results.append(
            Verdict(
                "d 碰撞世界探针",
                None,
                f"__vcw_probe 原文前 240 字: {probe_text[:240]!r} | "
                f"解析出 {len(boxes)} 个 AABB(判据:车心进入 AABB 收缩 {CLIP_INSET}m 后)。"
                f"注意探针只报**查询点 1.15m 内**的碰撞体,而 `probe_nearby_shapes` "
                f"用玩家位置 —— 在车里时玩家位置 = 车心,所以停在空旷车道上"
                f"就是 0 个,这不是 harness 故障。",
            )
        )

        # **构造撞击场景**:探针只在 1.15m 内报,所以不能先看附近有什么 ——
        # 得**朝着一栋楼开**,一边开一边探,进到 1.15m 内才知道前面是什么。
        # 目标建筑用 `bpos`(游戏自己算的「最近一栋楼」),不是自己摆坐标。
        bpos = await s.ev("(() => { const c = window.__vcw || {}; return c.bpos || null; })()")
        clip_samples: list[tuple] = []
        approach_note = "读不到 bpos(最近建筑),开不出撞击场景"
        if isinstance(bpos, list) and len(bpos) == 2 and None not in bpos:
            bx, bz = float(bpos[0]), float(bpos[1])
            cx = drive_after.get("carDriveX")
            cz = drive_after.get("carDriveZ")
            if cx is not None and cz is not None:
                approach_note = (
                    f"目标楼中心 ({bx:.2f},{bz:.2f})(=bpos,游戏自己算的最近建筑),"
                    f"距车心 {dist(cx, cz, bx, bz):.2f}m"
                )
                # 车头朝楼。W 是相机相对前进,先注入 yaw 对准楼。
                step(f'drive-aim-yaw')
                for yaw in (math.atan2(-(bx - cx), -(bz - cz)),
                            math.atan2(bx - cx, bz - cz)):
                    await s.ev(f"(() => {{ window.__vcw_aim = {yaw}; return 1; }})()")
                    await s.wait_frames(2)
                    await s.key("KeyW", True)
                    step('drive-loop')
                    for _ in range(30):
                        await s.wait_frames(1)
                        snap = await _st(s)
                        gx, gz = snap.get("carDriveX"), snap.get("carDriveZ")
                        if gx is None or gz is None:
                            continue
                        clip_samples.append((gx, gz, snap.get("carDriveSpeed")))
                        txt2, near2 = await _probe_shapes(s)
                        if _inside_aabbs(gx, gz, near2, CLIP_INSET):
                            break
                    await s.key("KeyW", False)
                    await s.wait_frames(2)
                    # 撞上就停;没撞上但已采样够,换个 yaw 再试一次。
                    if clip_samples:
                        _t_end, end_boxes = await _probe_shapes(s)
                        if _inside_aabbs(
                            clip_samples[-1][0], clip_samples[-1][1],
                            end_boxes, CLIP_INSET,
                        ):
                            break
                    if len(clip_samples) > 60:
                        break

        # **boxes 为空 = 什么都没测到,不是「撞不进去」。**
        # 旧版写的是 `None if boxes else False` —— 探针空的时候恰好判 FAIL,
        # 于是「harness 没拿到数据」被包装成「车撞不穿建筑」。
        results.append(
            Verdict(
                "d 撞击场景",
                None if boxes else None,
                f"{approach_note} | 推进中采样 {len(clip_samples)} 帧"
                + ("" if boxes else
                   " | **碰撞体探针重试 6 次仍为空** —— 这一项没有测到任何东西,"
                   "记 SKIP(既不判「撞不进去」也不判「能撞穿」)。"
                   "这是 harness 的数据通道问题,不是车的行为。"),
            )
        )

        # 最终判据:所有采样点里没有一个落在 AABB 内部。
        _final_text, final_boxes = await _probe_shapes(s)
        bad = []
        for i, (gx, gz, spd) in enumerate(clip_samples):
            hits = _inside_aabbs(gx, gz, boxes, CLIP_INSET)
            if hits:
                bad.append((i, gx, gz, spd, hits[0]))
        if not clip_samples:
            results.append(
                Verdict("d 车不穿建筑", None, "没能采到行驶样本(附近无 AABB 或读数缺失),未能判定")
            )
        else:
            travel = sum(
                dist(clip_samples[i - 1][0], clip_samples[i - 1][1], clip_samples[i][0], clip_samples[i][1])
                for i in range(1, len(clip_samples))
            )
            # **没采到 AABB 就不能判「没穿墙」。** 0 个碰撞体时
            # `bad` 恒为空,`not bad` 恒为 True —— 那是**空集通过**,
            # 看起来比任何真通过都漂亮。必须有碰撞体作判据才敢给结论。
            has_world = bool(boxes) or bool(final_boxes)
            ok = not bad and has_world
            results.append(
                Verdict(
                    "d 车不穿建筑",
                    True if ok else (False if bad else None),
                    f"{len(clip_samples)} 帧行驶 {_f(travel)}m,峰值速度 "
                    f"{_f(max((s[2] or 0.0) for s in clip_samples))} m/s | "
                    f"落在建筑 AABB 内部的采样:{len(bad)} 次"
                    + ((f" -> 首次: 第{bad[0][0]}帧 ({_f(bad[0][1])},{_f(bad[0][2])}) "
                        f"速度 {_f(bad[0][3])} m/s,所在 AABB 中心 {bad[0][4]['center']}")
                       if bad else
                       ("" if has_world else
                        " —— **探针重试 6 次一个 AABB 都没拿到**,所以「没穿墙」"
                        "这句话没有任何依据(空集恒成立),记 SKIP 而不是 PASS"))
                    + f" | 判据源: __vcw_probe(游戏自己的 game.world,重试后 "
                    f"共 {len(final_boxes)} 个 AABB,起始探针 {len(boxes)} 个)",
                )
            )

        # ---- (c) 按 F 下车 ----
        out_before = await _st(s)
        await s.tap("KeyF", 140)
        await s.wait_frames(4)
        out_after = await _st(s)
        got_out = out_after.get("veh") is None
        out_gap = vlib.dist(out_after.get("x"), out_after.get("z"),
                          out_before.get("carDriveX"), out_before.get("carDriveZ"))
        results.append(
            Verdict(
                "c 按 F 下车",
                True if got_out else False,
                f"playerVehicle {out_before.get('veh')} -> {out_after.get('veh')} | "
                f"下车位置 ({_f(out_after.get('x'))},{_f(out_after.get('z'))}),"
                f"下车点离车心 {_f(out_gap)}m | "
                f"grounded={out_after.get('grounded')} inside={out_after.get('inside')}",
            )
        )

    emit(results, WORKER)



async def _st(s: Session) -> dict:
    """`vlib.st()` 加上 `carDrive*` 三项。

    **`vlib.st()` 没有读 `carDriveX / carDriveZ / carDriveSpeed`** ——
    这三个字段只在 `window.__vcw` 里(`__vcw` 的 JSON 有
    `"carDriveX":{cdrive_x},"carDriveZ":{cdrive_z},"carDriveSpeed":{cdrive_v}`),
    而 `st()` 的 JS 表达式没有把它们取出来。全局 grep 确认 `vlib.py`
    里根本没有 `carDrive` 字样。

    它们是「车真的在动」的唯一证据(玩家坐标在车里是被动的,跟着车走),
    所以这里就地补一次 `ev()`。不去改 vlib。
    """
    st = await s.st()
    ex = await s.ev(
        "(() => { const c = window.__vcw || {};"
        " return {carDriveX: c.carDriveX, carDriveZ: c.carDriveZ,"
        " carDriveSpeed: c.carDriveSpeed}; })()"
    ) or {}
    st.update({k: v for k, v in ex.items() if k not in st})
    return st


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
# **1500s 对这个 worker 不够。** 撞楼那一步是「30 帧 x 2 个候选 yaw」的
# 循环,每帧一次 `_probe_shapes`(6 次重试 x 2 帧等待),在 SwiftShader 的
# 1.7 fps 下光这一步就是好几分钟;前面还有找车、传送、上下车若干轮。
# 1500s 踩到时,报出来的是「没跑完」,把「跑得久」和「真的卡死」混成
# 同一个结论 —— 这跟判据用错是同一类错误。
# 这里抬到 3000s:**只放宽时间上限,不放宽任何判据**;真的卡死仍然由
# 「面屑停在哪一步」来判(见 main 里的 step())。
WALL_CLOCK_LIMIT_S = 3000

if __name__ == "__main__":
    try:
        asyncio.run(asyncio.wait_for(main(), timeout=WALL_CLOCK_LIMIT_S))
    except asyncio.TimeoutError:
        fail_harness(WORKER, f"超过墙钟上限 {WALL_CLOCK_LIMIT_S}s 没跑完(通常是页面没 boot 成功,frames 恒为 0)")
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001
        fail_harness(WORKER, f"{type(exc).__name__}: {exc}")
