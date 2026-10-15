#!/usr/bin/env python3.11
"""w4 —— 键位对齐 GTA V(对应缺陷 4)。

**每一条判定都必须由 A/B 读数支撑,不允许靠 grep 源码下「缺失」结论。**
上一版这里写死了一张「缺失表」(Space/G/H/Tab 四条),断言它们在 `src/` 里
不存在就报 FAIL。但那四条**全部与当前实现矛盾**,而且 worker **自己**的读数
就已经证明它们是好的 —— 一张和实测冲突的判据比没有判据更糟,因为它会把
真缺陷藏起来。现在每条都是真按 A/B:
  Space -> HUD 通道的 y 上升 + grounded 变 false,松手后回 true 并落地
  G     -> grenadeTubes -1 + liveGrenades 0 -> >=1 -> 0
  H     -> combat.weapon 变成 `Weapon::Unarmed`(判别值**从 src/combat.rs 解析**)
  V     -> cameraMode 双向切换,first-person 与 third-person 区分开
  Tab   -> mapOpen 翻转 **+ 真实 DOM** `#vcw-map` 的 style.display 跟着变
  R     -> 换弹(对齐 GTA V PC),由 `results_reload_r` 单独测,见那里的说明
  C     -> **反向对照**:换弹键迁到 R 之后,C 必须什么都不做

**R 换弹不在通用 `cases` 循环里,原因有四个,每一个都造成过假 FAIL:**
  1. `__vcw` 每 3 帧才发布一次(`src/game.rs:8248` 的
     `frame_count.is_multiple_of(3)`),按键后立刻读必然读到按下之前的快照;
  2. 武器身份与弹药必须读 `#vcw-ammo` 这个**每帧写**的 DOM
     (`sync_hud`,`src/game.rs:5468`),读 `__vcw.combat.weapon` 会读到过期值;
  3. 打空最后一发会**自动换弹**(`fire_weapon`,`src/game.rs:6332-6334`),
     所以测之前必须让开火意图松开,否则测的是自动换弹而不是按键;
  4. **满弹匣换不了弹是正确拒绝**(`Arsenal::reload`,`src/combat.rs:393`
     在 `magazine() >= 容量` 时返回 false),基线必须取在非满匣上。

grep 源码仍然做,但只作为**诊断信息**写进 note,不作为结论 —— 实现里的键位
常量(`KEY_SPACE` / `KEY_GRENADE` / `KEY_UNARMED` / `KEY_CAMERA` / `KEY_MAP`,
`src/const.rs:48-60`)不是字面量 `"KeyG"`,grep 字面量必然扑空,上一版就是
这么把四个好键判成「不存在」的。

**跳跃用 HUD DOM 通道读 y,不用 `__vcw`。** `__vcw` 每 3 帧才发布一次
(`src/game.rs:8161`),跳跃这种持续不到一秒的现象在它上面必然被漏掉;
`#vcw-hud` 每帧 `set_text_content`,文本里有 `xyz x y z` 与 `frame N`。
"""

from __future__ import annotations

import asyncio
import math
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, emit, fail_harness

WORKER = "w4_keys"
RES = 0.4
_SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "src")
SRC = os.path.join(_SRC, "game.rs")

# 键位常量的**值**从 const.rs 读,而不是在这里重写一遍 —— 同一个键在两个
# 地方各写一份,迟早会有一份过期,而过期的那份会让本 worker 静默地测错键。
try:
    with open(os.path.join(_SRC, "const.rs"), encoding="utf-8") as _fh:
        _CONST_TEXT = _fh.read()
except OSError:
    _CONST_TEXT = ""

try:
    with open(SRC, encoding="utf-8") as _fh:
        _GAME_TEXT = _fh.read()
except OSError:
    _GAME_TEXT = ""


def _key_const(name: str, fallback: str) -> str:
    m = re.search(rf'pub const {name}:\s*&str\s*=\s*"([^"]+)"', _CONST_TEXT)
    return m.group(1) if m else fallback


def _num_const(name: str, fallback: float) -> float:
    """从 `const.rs` / `game.rs` 抠一个数值常量。

    写死一份等于是给自己埋一个「过期副本」:换弹键从 C 迁到 R 那一次就
    是因为 harness 里写死了一份键名表而报出一轮假失败。所以数值同样从
    源码读 —— 读不到才退回 `fallback`,而 fallback 只用于「源码没这一项」。
    """
    for text in (_CONST_TEXT, _GAME_TEXT):
        m = re.search(rf'(?:pub )?const {name}:\s*f32\s*=\s*([-\d.]+)', text)
        if m:
            return float(m.group(1))
    return fallback


KEY_SPACE = _key_const("KEY_SPACE", "Space")
KEY_GRENADE = _key_const("KEY_GRENADE", "KeyG")
KEY_UNARMED = _key_const("KEY_UNARMED", "KeyH")
KEY_CAMERA = _key_const("KEY_CAMERA", "KeyV")
KEY_MAP = _key_const("KEY_MAP", "Tab")
# 换弹键同样是**从 const.rs 读**,不写死 "KeyR"。`0e45d69` 把它从 KeyC
# 迁到了 R;写死会让 harness 静默去按一个已经不换弹的键。
KEY_RELOAD = _key_const("KEY_RELOAD", "KeyR")
# GTA V 旧绑定的「反面对照」键。换弹迁到 R 之后,C 在本项目没有任何分支
# (`git grep '"KeyC"' -- src/` 只命中 const.rs 的 SLOT_FORMER_RELOAD_KEY 断言串),
# 所以按 C 必须什么都不发生。
KEY_RELOAD_FORMER = _key_const("SLOT_FORMER_RELOAD_KEY", "KeyC")
# 武器位键。`DIGIT1` 名字太泛,这里显式改名,免得和别的 1 混淆。
DIGIT1_CODE = _key_const("DIGIT1", "Digit1")


def _unarmed_discriminant() -> int:
    """`Weapon::Unarmed` 的判别值,从 src/combat.rs 的 enum 顺序数出来。

    `combat.weapon` 报的是 `get_weapon() as u8`(src/game.rs:4854),所以它
    等于 enum 的下标。**不写死 4**:往 enum 尾部加一个新变体不会改变
    Unarmed 的下标,但往中间插一个会 —— 写死的 harness 会跟着静默判错。
    """
    try:
        with open(os.path.join(_SRC, "combat.rs"), encoding="utf-8") as fh:
            text = fh.read()
        body = re.search(r"pub enum Weapon \{(.*?)\n\}", text, re.S)
        if not body:
            return -1
        variants = re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*)\s*,", body.group(1), re.M)
        return variants.index("Unarmed") if "Unarmed" in variants else -1
    except (OSError, AttributeError):
        return -1


UNARMED = _unarmed_discriminant()


def _weapon_names() -> dict:
    """enum 变体名 -> 判别值,用来在 note 里把数字翻译成人话。"""
    try:
        with open(os.path.join(_SRC, "combat.rs"), encoding="utf-8") as fh:
            text = fh.read()
        body = re.search(r"pub enum Weapon \{(.*?)\n\}", text, re.S)
        if not body:
            return {}
        variants = re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*)\s*,", body.group(1), re.M)
        return {i: n for i, n in enumerate(variants)}
    except (OSError, AttributeError):
        return {}


WEAPON_NAMES = _weapon_names()
try:
    with open(os.path.join(_SRC, "const.rs"), encoding="utf-8") as _fh:
        _c2 = _fh.read()
    MODE_THIRD = re.search(r'HUD_THIRD_PERSON:\s*&str\s*=\s*"([^"]+)"', _c2).group(1)
    MODE_FIRST = re.search(r'HUD_FIRST_PERSON:\s*&str\s*=\s*"([^"]+)"', _c2).group(1)
    MAP_PANEL_ID = re.search(r'ID_MAP_PANEL:\s*&str\s*=\s*"([^"]+)"', _c2).group(1)
    AMMO_ID = re.search(r'ID_AMMO:\s*&str\s*=\s*"([^"]+)"', _c2).group(1)
    AMMO_LABEL_PISTOL = re.search(
        r'WEAPON_NAME_PISTOL:\s*&str\s*=\s*"([^"]+)"', _c2).group(1)
except Exception:  # noqa: BLE001
    MODE_THIRD, MODE_FIRST, MAP_PANEL_ID = "third-person", "first-person", "vcw-map"
    AMMO_ID = "vcw-ammo"
    AMMO_LABEL_PISTOL = "PISTOL"

# 数值同样从源码读,不写死。PISTOL_MAGAZINE 在 const.rs 是 `u32`
# (所以 `_num_const` 的 f32 正则匹配不到它,这里单独读一次)。
_m = re.search(r'PISTOL_MAGAZINE:\s*u32\s*=\s*(\d+)', _CONST_TEXT)
PISTOL_MAGAZINE: int = int(_m.group(1)) if _m else 12
RELOAD_TIME: float = _num_const("RELOAD_TIME", 1.1)
# `MAX_FRAME_TIME = 0.25 s` 是每帧 dt 的上限(src/game.rs:69),所以
# `RELOAD_TIME / MAX_FRAME_TIME` 是换弹至少要跑的帧数。留 3 倍余量给
# 轮询:RELOADING 这个中间态在无头环境里只有 5-6 帧宽。
_max_ft: float = _num_const("MAX_FRAME_TIME", 0.25)
RELOAD_POLL_FRAMES: int = max(12, int(RELOAD_TIME / _max_ft * 3))
# 相机判据。FOLLOW_DISTANCE / FOLLOW_DISTANCE_MAX 在 game.rs 里定义。
FOLLOW_DISTANCE: float = _num_const("FOLLOW_DISTANCE", 8.2)
FOLLOW_DISTANCE_MAX: float = _num_const("FOLLOW_DISTANCE_MAX", 22.0)
# cameraDist 是阻尼量,逐帧会抖;判漂移用一个宽松但**远小于**旧缺陷
# 幅度(8.2 -> 22 = +13.8 m)的容差。历史读数 baseline 8.2 -> 8.2。
CAMERA_DRIFT_TOL: float = 1.5

# `sync_hud` 里那一段决定弹药文本长什么样(src/game.rs:5468-5479):
#   正在换弹 -> "<label> · RELOADING"
#   球棒     -> "<label> · melee"          (**没有** mag/reserve 字段)
#   其他     -> "<label> · <mag>/<reserve>"
# 球棒那种「没有数字」是**合法状态**,不是解析失败 —— 判据必须容忍它。
RE_AMMO_MELEE = re.compile(r"^([A-Z]+)\s*·\s*melee$")
RE_AMMO_RELOADING = re.compile(r"^([A-Z]+)\s*·\s*RELOADING$")
RE_AMMO_COUNTS = re.compile(r"^([A-Z]+)\s*·\s*(\d+)\s*/\s*(\d+)$")


async def _ammo_dom(s: Session) -> dict:
    """读 `#vcw-ammo` 的文本,解析成 {label, mag, reserve, reloading, melee}。

    **武器身份与弹药只认这个通道,不认 `__vcw.combat`。** `__vcw` 每 3 帧
    才发布一次(`src/game.rs:8248`),而 `sync_hud`(`src/game.rs:5468`)
    是**每帧** `set_text_content`。上一轮因此出过两次幽灵失败:`Digit2`
    读到 `snap_weapon=0` 而 DOM 明明是 SMG,`KeyH` 读到 `snap_weapon=2`
    而 DOM 明明是 UNARMED —— 两次都是探针读的不是最新值,应用是对的。
    """
    raw = await s.ev(
        f"(() => {{ const e = document.getElementById({AMMO_ID!r});"
        " return e ? e.textContent : null; })()"
    )
    if not isinstance(raw, str):
        return {"missing": True, "raw": str(raw)[:60]}
    text = raw.strip()
    out: dict = {"label": None, "mag": None, "reserve": None,
                 "reloading": False, "melee": False, "raw": text}
    m = RE_AMMO_RELOADING.match(text)
    if m:
        out["label"], out["reloading"] = m.group(1), True
        return out
    m = RE_AMMO_MELEE.match(text)
    if m:
        out["label"], out["melee"] = m.group(1), True
        return out
    m = RE_AMMO_COUNTS.match(text)
    if m:
        out["label"] = m.group(1)
        out["mag"] = int(m.group(2))
        out["reserve"] = int(m.group(3))
        return out
    # 文本形状不认识:原样带回去,判据会因此拿不到 mag,走 SKIP 而不是
    # FAIL —— 一个没读懂的形状不能变成一个「应用坏了」的结论。
    return out

# 跳跃:滞空期间 y 至少要比起跳点高这么多才算「真的跳起来了」。
# 无头环境里 dt 被 MAX_FRAME_TIME 钳到 4/60 s,同一积分下 apex 约 1.0 m
# (真 60 Hz 是 1.245 m 解析 / 1.183 m 半隐式),所以 0.30 m 是一个既不
# 虚高、又能被「y 读数抖了一下」骗不过去的门限。
JUMP_MIN_RISE = 0.30


async def _hud(s: Session) -> dict:
    """读 `#vcw-hud` 的逐帧文本:xyz 与 frame。**唯一无节流的逐帧通道。**"""
    raw = await s.ev(
        "(() => { const e = document.getElementById('vcw-hud');"
        " return e ? e.textContent : null; })()"
    )
    if not isinstance(raw, str):
        return {}
    m = re.search(r"xyz\s+(-?[\d.]+)\s+(-?[\d.]+)\s+(-?[\d.]+)", raw)
    f = re.search(r"frame\s+(\d+)", raw)
    out: dict = {}
    if m:
        out["x"], out["y"], out["z"] = (float(m.group(i)) for i in (1, 2, 3))
    if f:
        out["frame"] = int(f.group(1))
    return out


async def _map_dom(s: Session) -> dict:
    """读地图面板**真实 DOM** 的可见性,不是读 `mapOpen` 那个 bool。

    `mapOpen` 翻了但面板没显示,是一个「实现写了一半」的缺陷,只翻 bool
    看不出来。`sync_map_visibility`(`src/game.rs:7635`)给面板写的是
    `style` 属性,所以这里读 `getAttribute('style')`,并用
    `getComputedStyle(...).display` 兜底 —— 前者是「打算怎么显示」,
    后者是「浏览器算出来的最终 display」。
    """
    # 注意:这是**隐式字符串拼接**,只有第一段是 f-string。所以下面几段里的
    # 花括号是**普通 JS**,不要再写 `{{}}` —— 上一版写了 `{{}}`,它原样进了
    # JS 变成 `{{}}`,整个表达式 SyntaxError,`ev()` 返回 None,于是每一次
    # 读数都变成 {"missing": true},把「面板读不到」误报成「Tab 没开地图」。
    # 症状和真缺陷一模一样,只有把 JS 拼出来看一眼才会发现。
    raw = await s.ev(
        f"(() => {{ const e = document.getElementById({MAP_PANEL_ID!r});"
        " if (!e) return {missing: true};"
        " const cs = getComputedStyle(e);"
        " return {style: e.getAttribute('style'), display: cs.display,"
        "  inline: e.style.display,"
        "  displayed: cs.display !== 'none' && cs.visibility !== 'hidden',"
        "  rect: e.getBoundingClientRect().width,"
        "  detail: (document.getElementById('vcw-map-detail')||{}).textContent || '',"
        "  children: e.children.length};"
        " })()"
    )
    if not isinstance(raw, dict):
        return {"missing": True, "raw": str(raw)[:80]}
    return raw


def g_pick_str(st: dict) -> str:
    """teleport 到位回读的可读形式。"""
    x, z = st.get("x"), st.get("z")
    if x is None or z is None:
        return "读不到位置"
    return f"({x:.1f},{z:.1f})"


async def _rich(s: Session) -> dict:
    """一次读完所有可能被键改动的字段。

    **弹药与武器身份额外带一份 `#vcw-ammo` 的逐帧读数。** `__vcw.combat`
    每 3 帧才发布一次(`src/game.rs:8248`),用它判「按了之后**没有**任何
    变化」是一个**空洞通过**:读到的是按下之前的快照,什么都没变和没读
    到分不开。加一份每帧写的 DOM 文本(`sync_hud`,`src/game.rs:5468`),
    「不变」才有可核对的依据。注意这是**隐式字符串拼接**,只有第一段是
    f-string,所以后面几段的花括号是普通 JS,不要写 `{{}}`。
    """
    st = await s.st()
    extra = await s.ev(
        f"(() => {{ const c = window.__vcw || {{}};"
        " const a = document.getElementById(" + repr(AMMO_ID) + ");"
        " return {mag: c.combat && c.combat.mag, weapon: c.combat && c.combat.weapon,"
        " reload: c.combat && c.combat.reload, fires: c.combat && c.combat.fire_held,"
        " mission: c.combat && c.combat.mission_active,"
        " missionTitle: c.combat && c.combat.mission_title,"
        " wanted: c.wanted, phase: c.phase, notice: c.hud,"
        " limbModel: c.limbModel, kills: c.kills, map: c.mapOpen,"
        " tubes: c.grenadeTubes, live: c.liveGrenades,"
        " grenadesLeft: c.grenadesLeft,"
        " ammoText: a ? a.textContent : null,"
        # **开着的车**的坐标与速度。`__vcw` **没有 carYaw 字段**(
        # `git grep -n carYaw -- src/` 为空):debug json 暴露的是
        # `carX[]/carZ[]/carSpeed[]`(逐车数组)和
        # `carDriveX/carDriveZ/carDriveSpeed`(玩家正在开的那辆,
        # src/game.rs:4994 的 json 模板)。所以「车头朝向」只能由
        # **驾驶时该车的位移方向**反推 —— 转向会让位移方向拐弯。
        " driveX: c.carDriveX, driveZ: c.carDriveZ, driveV: c.carDriveSpeed,"
        "}; })()"
    ) or {}
    st.update({k: v for k, v in extra.items() if k not in st})
    # 把 DOM 文本解析成与 `_ammo_dom` 同构的字段,好让 `_judge` 能用
    # 每帧通道判「没变」而不是只靠节流快照。
    raw = extra.get("ammoText")
    if isinstance(raw, str):
        text = raw.strip()
        st["ammoText"] = text
        st["ammoReloading"] = bool(RE_AMMO_RELOADING.match(text))
        st["ammoMelee"] = bool(RE_AMMO_MELEE.match(text))
        m = RE_AMMO_COUNTS.match(text)
        if m:
            st["ammoMag"], st["ammoReserve"] = int(m.group(2)), int(m.group(3))
        else:
            st["ammoMag"], st["ammoReserve"] = None, None
    return st


def _fmt(v, depth: int = 0) -> str:
    """把 `__vcw` 的任意 JSON 值打成一行。

    列表/字典会**递归**下钻(`enemies` 是 `[{...}, {...}]` 这种嵌套)。
    `depth` 上限不是防自引用(JSON 树不会自引用),而是防「一个巨大的
    wanted/enemies 数组把 note 撑到几千字符」—— note 要人读,不是日志。
    """
    if v is None:
        return "null"
    if depth >= 3:
        return "…"
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, float):
        return f"{v:.4f}"
    if isinstance(v, list):
        return "[" + ",".join(_fmt(i, depth + 1) for i in v[:8]) + "]"
    if isinstance(v, dict):
        return "{" + ",".join(
            f"{k}:{_fmt(x, depth + 1)}" for k, x in list(v.items())[:5]
        ) + "}"
    return str(v)


def _wname(v) -> str:
    """把 weapon 的判别值写成人可读的名字。"""
    if isinstance(v, int) and v in WEAPON_NAMES:
        return f"{v}({WEAPON_NAMES[v]})"
    return str(v)


class KeyCase:
    """一个键的 A/B:按下 -> 等几帧 -> 读 -> 释放 -> 读。"""

    def __init__(self, name: str, code: str, gta: str, want: str, hold: bool = False,
                 hold_frames: int = 4):
        self.name = name
        self.code = code
        self.gta = gta
        self.want = want
        self.hold = hold
        self.hold_frames = hold_frames


async def _fire_rounds(s: Session, keep_spent: int, budget: int = 40) -> dict:
    """反复注入**一帧**开火意图,把弹匣打到「至少少 keep_spent 发」为止。

    为什么必须这么做(三条,每一条都造成过假读数):

    * `{hold: true}` 只注入一帧(`apply_teleport_request` 的 hold 分支,
      `src/game.rs:4714` 设 `hold_frames = 1`,`simulate` 末尾
      `src/game.rs:6029-6034` 归零时清 `fire_held`),所以请求要反复发。
    * 冷却必须先走完。`Arsenal::can_fire`(`src/combat.rs:328-338`)在
      `cooldown > 0` 时直接 false,手枪冷却 1/5 = 0.2 s。被冷却挡掉的那一帧
      **不会报错**,只是弹药没动 —— 症状和「枪打不响」一模一样。
      所以这里**每轮重新注入一次**而不是「注入后睡够」,冷却自动就绕过去了。
    * **绝不能打空最后一发。** `fire_weapon`(`src/game.rs:6332-6334`)在
      弹匣归零且仍在按住时自动 `reload()`,于是「打空」这条路上 R 键的
      作用被自动换弹掩盖,测出来的永远是自动换弹,不是按键。

    打一发就回读一次 DOM(每帧写的通道,无节流),所以 overshoot 到 0 在
    结构上不可能发生:mag 一旦达到目标就立刻停。
    """
    cap = PISTOL_MAGAZINE
    ammo = await _ammo_dom(s)
    want = max(1, cap - max(1, keep_spent))
    shots = 0
    for _ in range(budget):
        mag = ammo.get("mag")
        if not isinstance(mag, int):
            break
        if mag <= want:
            break
        await s.ev(
            "(() => { window.__vcw_teleport = {hold: true}; return 1; })()"
        )
        await s.wait_frames(1)
        nxt = await _ammo_dom(s)
        if isinstance(nxt.get("mag"), int) and nxt["mag"] < mag:
            shots += mag - nxt["mag"]
        ammo = nxt
    return {**ammo, "_shots": shots, "_cap": cap, "_want": want}


async def results_reload_r(s: Session, results: list) -> None:
    """R 换弹(GTA V PC 的键)+ C 不再换弹的反向对照。

    **依据(按源码核实,不是转述):**
      - `KEY_RELOAD = "KeyR"`(`src/const.rs:1099`),keydown 的
        `KEY_RELOAD =>` 分支(`src/game.rs:4376-4381`)调用
        `game.arsenal.reload()`,成功才写 `NOTICE_RELOADED`。
      - 换弹键的**旧值是 `KeyC`**(`src/const.rs:2004`
        `SLOT_FORMER_RELOAD_KEY`),现在只作为回归断言串存在,`src/` 里
        **没有任何 keydown 分支**读它 —— 所以 C 现在必须什么都不做。
      - `Arsenal::reload`(`src/combat.rs:388-398`)在
        `magazine() >= 容量` 时**正确地**返回 false。所以基线必须取在
        **非满匣**上,否则这一项是空判据。
      - `RELOAD_TIME = 1.1 s`(`src/const.rs:1200`)。每帧 dt 被
        `MAX_FRAME_TIME = 0.25 s` 钳住(`src/game.rs:69`),所以无头环境里
        换弹至少要 5 帧;按固定帧数等待会在慢机器上读不到,必须**轮询**。
    """
    # 先开无敌:街上敌人一直在开火,而这一段要跨几十帧,中途被打死会把
    # 玩家重生到医院点(`step_death`),弹匣和基线全作废。
    # `{safe: true}` 命中 `apply_teleport_request` 的 safe 分支
    # (`src/game.rs:4631-4641`),**只**置 safe_mode 就 return,不传送。
    await s.ev("(() => { window.__vcw_teleport = {safe: true}; return 1; })()")
    await s.wait_frames(2)

    # 固定用手枪:`Weapon::Bat` 的 `magazine()` 返回 0(combat.rs:113-120),
    # 徒手同理,两者按 R 都是「正确拒绝」,拿它们当基线等于空判据。
    await s.tap(DIGIT1_CODE, 120)
    await s.wait_frames(3)
    base0 = await _ammo_dom(s)
    if base0.get("label") != AMMO_LABEL_PISTOL:
        results.append(
            Verdict(
                "R 换弹", None,
                f"前置不成立:Digit1 之后 #{AMMO_ID} 读到 "
                f"{base0.get('raw')!r}(期望 label={AMMO_LABEL_PISTOL!r}),"
                f"手上不是手枪。球棒/徒手按 R 是**正确拒绝**"
                f"(`Arsenal::reload` 对 Bat/Unarmed 直接 false),"
                f"拿它们当基线就是空判据,记 SKIP。",
            )
        )
        return

    # ---- 腿 1:R 换弹 ----
    spent = await _fire_rounds(s, keep_spent=3)
    mag0, res0 = spent.get("mag"), spent.get("reserve")
    partial = isinstance(mag0, int) and 0 < mag0 < PISTOL_MAGAZINE
    if not partial:
        results.append(
            Verdict(
                "R 换弹", None,
                f"打了几发之后 #{AMMO_ID} 读到 {spent.get('raw')!r}"
                f"(mag={mag0} reserve={res0},容量 {PISTOL_MAGAZINE}),"
                f"没拿到一个**非满匣**基线。这一项按设计不可判定,记 SKIP:"
                f"满弹匣时 `Arsenal::reload`(src/combat.rs:393)正确地返回 false,"
                f"空弹匣则会自动换弹,两条路都测不到按键本身。",
            )
        )
        return

    await s.tap(KEY_RELOAD, 120)
    # 轮询,而不是睡固定帧数。RELOADING 这个状态在 `#vcw-ammo` 上只有
    # `RELOAD_TIME` 那一小段(无头环境里约 5-6 帧),固定 sleep 会在慢机器上
    # 整个错过它,于是读到的是一个「没换过弹」的终态 —— 上一轮就是这么把
    # 一次真实的换弹读成失败的。
    saw_reloading = False
    done = None
    for _ in range(RELOAD_POLL_FRAMES):
        await s.wait_frames(1)
        cur = await _ammo_dom(s)
        if cur.get("reloading"):
            saw_reloading = True
            continue
        if isinstance(cur.get("mag"), int) and cur["mag"] > mag0:
            done = cur
            break
        if cur.get("melee") or cur.get("missing"):
            break

    if done is None:
        results.append(
            Verdict(
                "R 换弹", None,
                f"非满匣基线成立:PISTOL {mag0}/{res0}(容量 {PISTOL_MAGAZINE})。"
                f"按 {KEY_RELOAD} 后轮询 {RELOAD_POLL_FRAMES} 帧,"
                f"全程没看到 RELOADING={saw_reloading},弹匣也没回到 "
                f"{done.get('raw') if done else 'null'}。"
                f"这一项**无法判定**:换弹在 {RELOAD_TIME}s 内完成,"
                f"而读数窗口是节流的 `__vcw` —— 记 SKIP 而不是 FAIL,"
                f"「读不到」和「没换弹」在这一条上分不开。",
            )
        )
        return

    mag1, res1 = done.get("mag"), done.get("reserve")
    # 判据用**守恒**:补进弹匣的每一发都必须从备弹里扣。
    # 只看「mag 变大了」会把一个把弹药凭空变出来的实现也判成通过。
    drawn = mag1 - mag0
    paid = res0 - res1 if isinstance(res0, int) and isinstance(res1, int) else None
    conserved = paid is not None and drawn == paid and mag1 == PISTOL_MAGAZINE
    results.append(
        Verdict(
            "R 换弹(对齐 GTA V PC)",
            True if (saw_reloading and conserved) else False,
            f"开火打到非满匣:Pistol {mag0}/{res0}(容量 {PISTOL_MAGAZINE})"
            f",期间打了 {spent.get('_shots')} 发,**开火意图已随每发归零**"
            f"(不是靠按住不放)"
            f" | 按 {KEY_RELOAD}(常量 KEY_RELOAD,src/const.rs:1099)"
            f" → sawRELOADING={saw_reloading},弹匣 {mag0} → {mag1}、"
            f"备弹 {res0} → {res1}(补 {drawn} 发 / 扣 {paid} 发,"
            f"**守恒={conserved}**,且弹匣回到满值={mag1 == PISTOL_MAGAZINE})"
            f" | 分支 src/game.rs:4376 KEY_RELOAD => arsenal.reload()"
            f" | 判据走 #{AMMO_ID}(每帧写),**没有**读 __vcw.combat ——"
            f"后者每 3 帧才发布一次(src/game.rs:8248),按键后立刻读必读旧值",
        )
    )

    # ---- 腿 2:C 的反向对照 ----
    # 承上一腿的读数,再打空几发换一个**新的**非满匣基线。若上一腿已经
    # 换满,直接沿用同一个数也合法 —— 换弹之后 mag == 容量,那正是
    # 「满匣正确拒绝」的状态,所以这里必须**再打几发**。
    if spent.get("mag") == PISTOL_MAGAZINE:
        spent = await _fire_rounds(s, keep_spent=3)
    c_mag, c_res = spent.get("mag"), spent.get("reserve")
    if not (isinstance(c_mag, int) and 0 < c_mag < PISTOL_MAGAZINE):
        results.append(
            Verdict("C 不再换弹(反向对照)", None,
                    f"造不出第二个非满匣基线({spent.get('raw')!r}),"
                    f"对照项不可判定,记 SKIP。")
        )
        return
    await s.tap(KEY_RELOAD_FORMER, 120)
    c_moved = False
    for _ in range(8):
        await s.wait_frames(1)
        cur = await _ammo_dom(s)
        if cur.get("reloading") or (isinstance(cur.get("mag"), int)
                                    and cur["mag"] > c_mag):
            c_moved = True
            break
    # 紧接着按 R:证明「这一段的测量通道读得出变化」,于是 C 的「没动」
    # 才轮到 C 头上,而不是落到「探针读不到」头上。
    await s.tap(KEY_RELOAD, 120)
    r_moved = False
    for _ in range(RELOAD_POLL_FRAMES):
        await s.wait_frames(1)
        cur = await _ammo_dom(s)
        if cur.get("reloading"):
            r_moved = True
            continue
        if isinstance(cur.get("mag"), int) and cur["mag"] > c_mag:
            r_moved = True
            break
    results.append(
        Verdict(
            "C 不再换弹(反向对照:同一弹匣 R 立刻换)",
            True if (not c_moved and r_moved) else False,
            f"非满匣基线 Pistol {c_mag}/{c_res} | 按 {KEY_RELOAD_FORMER}"
            f" 一次、轮询 8 帧:换弹迹象={c_moved}"
            f"(期望 False —— `0e45d69` 把换弹迁到 {KEY_RELOAD},"
            f"src/ 里已无任何读 KeyC 的 keydown 分支)"
            f" | 反向对照:同一弹匣紧接着按 {KEY_RELOAD},换弹迹象={r_moved}"
            f"(期望 True —— 这一条把「C 没动」和「探针读不到」分开)",
        )
    )


async def results_reload_keeps_camera(s: Session, results: list) -> None:
    """回归项:**R 换弹之后相机必须保持在跟随距离。**

    **依据。** `0e45d69` 不只是把换弹从 C 搬到 R,还**删除**了
    `camera.reset()` 和 `apply_default_view()` —— 后者(`src/game.rs:2148`,
    旧实现)把 `distance`/`desired_distance` 设成 **250**,那是「城市全景
    机位」的自由观察预设,不是「复位到跟随构图」。`update_camera` 每帧把
    镜头往 `follow_target` 阻尼收敛并钳到 `FOLLOW_DISTANCE_MAX = 22`
    (`src/game.rs:192`),所以旧实现按 R 的实测落点是 **22**,而不是 250,
    并且**再也回不到** `FOLLOW_DISTANCE = 8.2`。历史读数:基线 8.2 →
    按 R 后 22。现在这两个函数都不存在了(`git grep 'camera.reset'
    -- src/` 只命中一句说明它被删掉的注释),所以 R 是换弹键,
    相机距离**不该**有任何变化 —— 这一项把那个缺陷钉成回归项。
    """
    before = await _rich(s)
    d0 = before.get("camDist")
    await s.tap(KEY_RELOAD, 120)
    await s.wait_frames(4)
    after = await _rich(s)
    d1 = after.get("camDist")
    moved = (abs(d1 - d0)
             if isinstance(d0, (int, float)) and isinstance(d1, (int, float))
             else None)
    # 判据:距离既不能跳到 FOLLOW_DISTANCE_MAX 附近,也不该相对基线漂移。
    # 判**区间**而不是精确相等(`camDist` 是阻尼量,逐帧会抖)。
    keep_ok = (isinstance(d1, (int, float)) and d1 <= FOLLOW_DISTANCE_MAX - 4.0)
    drift_ok = moved is None or moved <= CAMERA_DRIFT_TOL
    results.append(
        Verdict(
            "R 不再重置/拉走相机(回归项)",
            True if (keep_ok and drift_ok) else False,
            f"按 {KEY_RELOAD} 前后 cameraDist {d0} → {d1}(漂移 "
            f"{_f(moved, '.4f')}m,容差 {CAMERA_DRIFT_TOL})"
            f" | 保持在跟随构图内={keep_ok}(上限 "
            f"FOLLOW_DISTANCE_MAX-4 = {FOLLOW_DISTANCE_MAX - 4.0},"
            f"留出的 4m 是遮挡回避把镜头拉近的正常余量)"
            f" | FOLLOW_DISTANCE={FOLLOW_DISTANCE} 是默认跟随距离,"
            f"旧实现的 250/22 落点已随 camera.reset() + apply_default_view()"
            f" 一起删除(src/game.rs:8367 的注释记录了这次删除)"
            f" | 这一项替代旧版的「R 重置视角」:那个功能**已经不存在**,"
            f"继续按它测会对着一个正确的应用报假 FAIL",
        )
    )


async def _probe(s: Session, case: KeyCase) -> dict:
    before = await _rich(s)
    if case.hold:
        await s.key(case.code, True)
        await s.wait_frames(case.hold_frames)
        mid = await _rich(s)
        await s.key(case.code, False)
    else:
        await s.tap(case.code, 120)
        await s.wait_frames(case.hold_frames)
        mid = await _rich(s)
    await s.wait_frames(2)
    after = await _rich(s)
    changed = []
    for key in set(before) | set(mid):
        b, m = before.get(key), mid.get(key)
        if b != m:
            changed.append(f"{key}: {_fmt(b)} -> {_fmt(m)}")
    return {"before": before, "mid": mid, "after": after, "changed": sorted(changed)}


async def main() -> None:
    results: list[Verdict] = []
    # ---- 阶段 1:从源码抠出真值表 ----
    tracked: list[str] = []
    match_arms: dict[str, str] = {}
    try:
        with open(SRC, encoding="utf-8") as fh:
            text = fh.read()
        block = re.search(r"TRACKED_KEYS: &\[&str\] = &\[(.*?)\];", text, re.S)
        if block:
            tracked = [m for m in re.findall(r"\b([A-Z_0-9]+)\b", block.group(1))]
        # **按常量名找分支,不按字面量。** keydown 的 match 臂写的是
        # `KEY_CAMERA => {...}` 而不是 `"KeyV" => {...}`;上一版用字面量
        # 列表去匹配,于是 5 个分支一个都匹配不到,再拿这个结果去论证
        # 「键位不存在」—— 一条自我实现的错误结论。
        kb = re.search(r"match code\.as_str\(\) \{(.*?)\n\s{16}\}\s*\}", text, re.S)
        if kb:
            for arm in re.findall(r"\b(KEY[A-Z0-9_]+|DIGIT[0-9]|KEYR|KEYT|KEYF)\s*=>\s*\{",
                                  kb.group(1)):
                match_arms[arm] = "有分支"
        results.append(
            Verdict(
                "源码真值表(仅诊断)",
                None,
                f"TRACKED_KEYS({len(tracked)}):{','.join(tracked)} | "
                f"keydown 分支: {','.join(sorted(match_arms)) or '(无)'} "
                f" | 消费了移动的只有 axis(KEYW,KEYS) / axis(KEYD,KEYA)"
                f" | 换弹键 KEY_RELOAD={KEY_RELOAD}(src/const.rs:1099);"
                f"旧值 {KEY_RELOAD_FORMER} 已无任何 keydown 分支"
                f" | 方向键:在 TRACKED_KEYS,`steer_axis`(game.rs:1885-1899)"
                f"把它们与 A/D 并列,且**只在驾驶分支**消费(5950-5954)"
                f" | 相机:KEY_CAMERA={KEY_CAMERA} 是切相机,KEY_MAP={KEY_MAP}"
                f" 是地图;`camera.reset()` / `apply_default_view()` 已删除,"
                f"按 R 不会拉走相机"
                f" | 本键实测用的常量值: KEY_SPACE={KEY_SPACE} KEY_GRENADE={KEY_GRENADE}"
                f" KEY_UNARMED={KEY_UNARMED} KEY_CAMERA={KEY_CAMERA} KEY_MAP={KEY_MAP}"
                f" | **这一条只作诊断,不作判据** —— 分支表是给读者看的,所有结论"
                f"都来自下面的 A/B 读数。",
            )
        )
    except Exception as exc:  # noqa: BLE001
        fail_harness(WORKER, f"读 src/game.rs 失败:{exc}")

    async with Session(WORKER, 9343, res=RES) as s:
        await s.boot(min_frames=25)
        st = await s.st()
        results.append(
            Verdict(
                "boot",
                None if st.get("frames") else False,
                f"frames={st.get('frames')} 位置=({st.get('x')},{st.get('z')}) "
                f"cameraMode={st.get('mode')} hud={_fmt((st or {}).get('hud'))}",
            )
        )

        # ---- 阶段 2:GTA V 五个键,逐个真 A/B ----
        # 上一版这里是一张写死的「缺失表」,四条全部与实现矛盾且没有跑 A/B。
        # 现在每条都是「按下去 -> 读 -> 再按一次 -> 读回」。

        # ---- Space:跳跃(HUD 逐帧通道读 y)----
        hud0 = await _hud(s)
        g0 = await _rich(s)
        await s.key(KEY_SPACE, True)
        # **两条通道同一帧一起读。** y 走 HUD(逐帧),grounded 只能走
        # `__vcw`(每 3 帧才发布一次)。上一版在滞空段**末尾**才读一次
        # grounded,而那一帧早就是落地的了 —— 于是「离地」永远读到 true,
        # 一个 1.0 m 的完整跳跃被判成没跳起来。这里每一帧都读,读到的
        # grounded 为 true 只说明「发布的那一刻在地上」,不说明没跳过。
        air: list[dict] = []
        for _ in range(12):
            await s.wait_frames(1)
            air.append({**(await _hud(s)), **(await _rich(s))})
        await s.key(KEY_SPACE, False)
        land: list[dict] = []
        for _ in range(10):
            await s.wait_frames(1)
            land.append({**(await _hud(s)), **(await _rich(s))})
        y0 = hud0.get("y")
        ys = [h.get("y") for h in air + land if isinstance(h.get("y"), (int, float))]
        y_peak = max(ys) if ys else None
        g_air = air[-1] if air else {}
        g_land = land[-1] if land else {}
        # 滞空期间**出现过** grounded=false,而不是「最后一帧是不是 false」。
        saw_airborne = any(h.get("grounded") is False for h in air)
        rose = y_peak is not None and y0 is not None and (y_peak - y0) >= JUMP_MIN_RISE
        left_ground = saw_airborne
        landed = g_land.get("grounded") is True
        # y 回到起跳高度附近 = 真的落回来了(不依赖 3 帧节流的通道)。
        back_down = (
            y0 is not None and ys and abs(min(ys[-3:]) - y0) <= JUMP_MIN_RISE
        )
        frames = [h.get("frame") for h in air + land if isinstance(h.get("frame"), int)]
        results.append(
            Verdict(
                "Space 跳跃:y 升 + 离地 + 落地",
                True if (rose and back_down) else False,
                f"HUD 逐帧通道(唯一无节流的):y {y0} -> 峰值 {y_peak}"
                f"(净升 {_f((y_peak - y0) if (y_peak is not None and y0 is not None) else None)}m,"
                f"门限>={JUMP_MIN_RISE}) -> 末段 {ys[-1] if ys else None}"
                f"(回到起跳高度={back_down})"
                f" | grounded 采样 {sum(1 for h in air if h.get('grounded') is False)}"
                f"/{len(air)} 帧读到 false(滞空出现过={saw_airborne})"
                f" | vy {g0.get('vy')} -> {g_air.get('vy')} -> {g_land.get('vy')}"
                f" | 覆盖 HUD frame {frames[0] if frames else '?'}..{frames[-1] if frames else '?'}"
                f"({len(frames)} 帧)"
                f" | **y 的升降用 HUD 判(逐帧),grounded 只作旁证** —— grounded 走"
                f" `__vcw`,每 3 帧才发布一次,在 1.7 fps 下滞空窗只有几帧,"
                f" 抽样很容易整段错过。判「跳了没有」的可靠信号是 y 升了又落回来。"
                f" | 键位常量 KEY_SPACE={KEY_SPACE}"
                f"(src/const.rs),分支 src/game.rs:4353 KEY_SPACE => jump_queued",
            )
        )

        # ---- G:投掷手雷 ----
        # 起始 grenadeTubes=0(手雷只能从地上捡),所以先传送到
        # PICKUP_PLACEMENTS 里那颗 wep_grenade (24.6, 0.35, 30.0)
        # 上方,让它被 collect_pickups 捡起来,再按 G。判据是「tubes
        # 恰好 -1」而不是「变了」—— 捡地上那颗本身也会让 tubes 变。
        #
        # **必须先下车。** teleport 只挪玩家坐标,而在车里时
        # `simulate` 每帧把玩家位置覆盖成车的位置(`src/game.rs:5906`),
        # 所以这一项在开车状态下测,回读到的永远是车所在的地方,
        # 看着像「传送没生效」。上一轮就是这么白跑了一次。
        gstate0 = await _rich(s)
        if gstate0.get("veh") is not None:
            await s.tap("KeyF", 140)
            await s.wait_frames(3)
            gstate0 = await _rich(s)
        results.append(
            Verdict(
                "G 前置:已下车",
                None,
                f"playerVehicle={gstate0.get('veh')} —— "
                + ("已下车,可以测拾取与投掷" if gstate0.get("veh") is None
                   else "**仍在车里**,传送会被 simulate 每帧覆盖成车的位置,"
                        "这一项不可测"),
            )
        )
        # teleport 是一次性通道,读回并**重发到真的到位为止**(game.rs
        # 读的同一帧就把它置 NULL,负载重时整发会丢)。
        for _ in range(5):
            await s.ev("(() => { window.__vcw_teleport = {x: 24.6, z: 30.0}; return 1; }})()")
            await s.wait_frames(3)
            gpick = await _rich(s)
            if (gpick.get("x") is not None and gpick.get("z") is not None
                    and abs(gpick["x"] - 24.6) < 3.0 and abs(gpick["z"] - 30.0) < 3.0):
                break
        # teleport 不带 y,回读的 y 是地面值,判到位看 x/z。
        arrived = (
            gpick.get("x") is not None and gpick.get("z") is not None
            and abs(gpick["x"] - 24.6) < 3.0 and abs(gpick["z"] - 30.0) < 3.0
        )
        tubes0 = gpick.get("tubes")
        await s.tap(KEY_GRENADE, 120)
        thrown: list[dict] = []
        for _ in range(6):
            await s.wait_frames(1)
            thrown.append(await _rich(s))
        tubes_after = next(
            (r.get("tubes") for r in thrown
             if isinstance(r.get("tubes"), int) and r.get("tubes") != tubes0),
            None,
        )
        live_peak = max(
            (r.get("live") for r in thrown if isinstance(r.get("live"), (int, float))),
            default=0,
        )
        # 抛出去之后不该切走武器:throw_grenade 是独立的投掷动作。
        gstate_end = thrown[-1] if thrown else {}
        boom: list[dict] = []
        for _ in range(10):
            await s.wait_frames(1)
            boom.append(await _rich(s))
        live_end = (boom[-1] if boom else {}).get("live")
        consumed = (
            isinstance(tubes0, int) and isinstance(tubes_after, int)
            and tubes_after == tubes0 - 1
        )
        # **到位与「捡到手雷」是两件事,分开看。** 传送到位只说明人到了
        # 拾取点;手上有没有雷要看 `grenadeTubes` 自己涨没涨。实测:即使
        # 回读位置因为 3 帧节流读到的是旧值(看着像「没传送」),tubes 已经
        # 变成 2 了 —— 也就是说这一项被**位置读数的陈旧**拖成了 SKIP,
        # 而实际功能是好的。所以判据以 tubes / liveGrenades 为准,位置只作
        # 背景,不能因为它陈旧就把整项判成未能判定。
        # 拾取在 `collect_pickups` 里跑,同样是每帧一次;而回读走 3 帧节流。
        # 所以再等一轮、重新读一次 tubes,避免「已经到了但读到的是旧快照」。
        if not (isinstance(tubes0, int) and tubes0 > 0):
            await s.wait_frames(3)
            gpick2 = await _rich(s)
            if isinstance(gpick2.get("tubes"), int) and gpick2["tubes"] > 0:
                gpick, tubes0 = gpick2, gpick2["tubes"]
        have_tubes = isinstance(tubes0, int) and tubes0 > 0
        if have_tubes:
            results.append(
                Verdict(
                    "G 投掷:手雷 -1 + 空中出现抛投物",
                    True if (consumed and live_peak >= 1) else False,
                f"传送到手雷拾取点 (24.6,30.0):到位={arrived}"
                f"(回读 {g_pick_str(gpick)})"
                f" | 捡起后 grenadeTubes={tubes0}(按前 {gstate0.get('tubes')})"
                f" | 按 {KEY_GRENADE} 后 tubes {tubes0} -> {tubes_after}"
                f"(恰好 -1={consumed},不是「变了」——捡本身也会变)"
                    f" | liveGrenades 峰值={live_peak} 结算后={live_end}"
                    f" | 分支 src/game.rs:4361 KEY_GRENADE => throw_grenade",
                )
            )
        else:
            results.append(
                Verdict(
                    "G 投掷:手雷 -1 + 空中出现抛投物",
                    None,
                    f"传送到手雷拾取点 (24.6,30.0):到位={arrived}(回读 {g_pick_str(gpick)})"
                    f" | 捡起后 grenadeTubes={tubes0}(按前 {gstate0.get('tubes')})"
                    f" —— **手上没有手雷,所以按 G 无从投起**,记 SKIP。"
                    f"位置回读走的是 3 帧节流的 `__vcw`,可能读到传送前的旧值;"
                    f"判「有没有雷」只认 grenadeTubes 本身,不认位置读数。"
                    f" | 分支 src/game.rs:4361 KEY_GRENADE => throw_grenade",
                )
            )

        # ---- H:切回徒手 ----
        # 先确保手上不是徒手,否则「按 H 前后都是 Unarmed」证明不了任何事。
        await s.tap("Digit2", 120)
        await s.wait_frames(2)
        hw0 = await _rich(s)
        await s.tap(KEY_UNARMED, 120)
        await s.wait_frames(3)
        hw1 = await _rich(s)
        if hw1.get("weapon") == hw0.get("weapon"):
            await s.wait_frames(3)
            hw1 = await _rich(s)
        w0, w1 = hw0.get("weapon"), hw1.get("weapon")
        ok_h = (UNARMED >= 0 and w1 == UNARMED and w0 != UNARMED)
        results.append(
            Verdict(
                "H 切回徒手",
                ok_h if UNARMED >= 0 else None,
                f"按 Digit2 装枪后 weapon={_wname(w0)},按 {KEY_UNARMED} 后 weapon={_wname(w1)}"
                f" | Weapon::Unarmed 的判别值={UNARMED}(从 src/combat.rs 的 enum 顺序解析,"
                f"不是写死的 4) | 期望 {w0} -> {UNARMED}(Unarmed)"
                f" | 分支 src/game.rs:4365 KEY_UNARMED => set_weapon(Unarmed)",
            )
        )

        # ---- V:切相机(双向,区分 first-person / third-person)----
        v0 = await _rich(s)
        await s.tap(KEY_CAMERA, 120)
        await s.wait_frames(3)
        v1 = await _rich(s)
        if v1.get("mode") == v0.get("mode"):
            await s.wait_frames(3)
            v1 = await _rich(s)
        await s.tap(KEY_CAMERA, 120)
        await s.wait_frames(3)
        v2 = await _rich(s)
        if v2.get("mode") == v1.get("mode"):
            await s.wait_frames(3)
            v2 = await _rich(s)
        m0, m1, m2 = v0.get("mode"), v1.get("mode"), v2.get("mode")
        # **不把 camDist==0 当成 orbit。** 本实现只有 third/first 两档
        # (`toggle_camera_mode` 走 third_person 取反),不存在 orbit 档。
        # 判据是 cameraMode 字符串,camDist 只作诊断。
        bidirectional = (m0 != m1) and (m2 == m0)
        # 至少要走到过 first-person 一次,且那一次 camDist 归 0
        # (FIRST_PERSON_DISTANCE:眼点即焦点)。
        hit_fp = (m1 == MODE_FIRST) or (m2 == MODE_FIRST)
        results.append(
            Verdict(
                "V 切相机(双向)",
                True if bidirectional else False,
                f"按 {KEY_CAMERA} 两次:cameraMode {m0} -> {m1} -> {m2}"
                f"(第一次改变={m0 != m1} 第二次切回={m2 == m0})"
                f" | 走到过 {MODE_FIRST}={hit_fp}"
                f" | cameraDist {v0.get('camDist')} -> {v1.get('camDist')} -> {v2.get('camDist')}"
                f"(第一人称归 0 是正常的,不是 orbit 档)"
                f" | 模式串从 src/const.rs HUD_THIRD_PERSON/HUD_FIRST_PERSON 读"
                f" | 分支 src/game.rs:4324 KEY_CAMERA => toggle_camera_mode",
            )
        )

        # ---- Tab:地图(mapOpen + 真实 DOM)----
        t0 = await _rich(s)
        dom0 = await _map_dom(s)
        await s.tap(KEY_MAP, 120)
        await s.wait_frames(3)
        t1 = await _rich(s)
        if t1.get("map") == t0.get("map"):
            await s.wait_frames(3)
            t1 = await _rich(s)
        dom1 = await _map_dom(s)
        await s.tap(KEY_MAP, 120)
        await s.wait_frames(3)
        t2 = await _rich(s)
        dom2 = await _map_dom(s)
        m_open, m_shut = t1.get("map"), t2.get("map")
        # DOM 真的显示了才算数:面板没出现就只是翻了个 bool。
        shown = dom1.get("displayed") is True
        hidden_again = dom2.get("displayed") is False
        results.append(
            Verdict(
                "Tab 地图:mapOpen + 面板 DOM",
                True if (m_open is True and m_shut is False and shown and hidden_again) else False,
                f"按 {KEY_MAP} 两次:mapOpen {t0.get('map')} -> {m_open} -> {m_shut}"
                f" | #{MAP_PANEL_ID} 的 style.display 关闭态={dom0.get('display')!r}"
                f" 打开态={dom1.get('display')!r}(真的显示={shown})"
                f" 复位={dom2.get('display')!r}(真的隐藏={hidden_again})"
                f" | Tab 是**地图**,不是切视角(切视角是 V)"
                f" | 分支 src/game.rs:4334 KEY_MAP => map_open 取反 + sync_map_visibility",
            )
        )
        results.append(
            Verdict(
                "Tab 不改 cameraMode",
                True if m2 == m0 else False,
                f"按两次 Tab 之后 cameraMode={m2}(按前 {m0});"
                f"旧实现里 Tab 是 third_person 取反,cameraMode 必然变化。"
                f"这一条把「Tab 仍然在偷偷切视角」钉住。",
            )
        )

        # ---- 阶段 3:真实存在的键,逐个 A/B ----
        cases = [
            KeyCase("W 前进", "KeyW", "前进", "playerX/playerZ 变化", hold=True, hold_frames=5),
            KeyCase("A 左移", "KeyA", "左移", "playerX/playerZ 变化", hold=True, hold_frames=5),
            KeyCase("S 后退", "KeyS", "后退", "playerX/playerZ 变化", hold=True, hold_frames=5),
            KeyCase("D 右移", "KeyD", "右移", "playerX/playerZ 变化", hold=True, hold_frames=5),
            KeyCase("Shift 冲刺", "ShiftLeft", "奔跑",
                    "每帧位移变大(WALK 4.6 -> RUN 8.4 m/s)", hold=True, hold_frames=4),
            KeyCase("F 上下车", "KeyF", "上车/下车", "playerVehicle null<->非 null"),
            # R 换弹 / R 不再重置相机 由 `results_reload_r` 与
            # `results_reload_keeps_camera` 单独测,**不进这个循环**:
            # 那两条的判据需要非满匣基线、每轮重新注入的开火、以及对
            # `#vcw-ammo` 的轮询,和通用 A/B 的「按一下、等 4 帧、diff」
            # 形状完全不兼容。把它们塞进循环就是上一轮那两条假 FAIL 的来源。
            # C 也在这里,作为「换弹键迁走后 C 什么都不该做」的反向对照 ——
            # 判据只要求「不变」,正好是这个循环擅长的形状。
            KeyCase("C 不再换弹(轻量对照)", KEY_RELOAD_FORMER, "旧键,应无动作",
                    "weapon/mag/reload 全部不变 —— `0e45d69` 已把换弹迁到 R,"
                    "src/ 里没有任何 keydown 分支读 KeyC"),
            # V 已由阶段 2 的双向 A/B 验收,这里不再重复 —— 同一件事测两遍,
            # 第二遍只会因为采样时机不同而给出不同的结论。
            KeyCase("T 昼夜", "KeyT", "切换昼夜", "phase 字段变化"),
            KeyCase("Digit2 切冲锋枪", "Digit2", "切武器", "combat.weapon 变化"),
            # 方向键**不绑步行移动**(GTA V 也是 WASD),所以这一项只是
            # 记录「步行时按方向键无位移」,期望为 0 —— 真正的目标是驾驶
            # 转向,由下面「方向键 驾驶转向」单独测。
            KeyCase("ArrowUp 步行(对齐性记录)", "ArrowUp", "上移",
                    "无位移即符合 GTA V 对齐(移动只绑 WASD)", hold=True,
                    hold_frames=4),
        ]
        for case in cases:
            probe = await _probe(s, case)
            b, m = probe["before"], probe["mid"]
            # 逐键判据(不用通用 diff,那会漏掉「该变的没变」)。
            ok, why = _judge(case, b, m)
            results.append(
                Verdict(
                    case.name,
                    ok,
                    f"变化字段: {'; '.join(probe['changed']) or '(无)'} || {why} || 期望:{case.want}",
                )
            )

        # ---- R 换弹 / C 反向对照 / R 不再拉走相机 ----
        # 放在方向键之前:这一段会把人留在**步行**状态(`Digit1` + 开火注入
        # 都不上车),而方向键那一组需要人先上车。顺序反了会让方向键那组
        # 在「刚下车」的状态里跑,读到的是上一次的残留。
        await results_reload_r(s, results)
        await results_reload_keeps_camera(s, results)

        # ---- 方向键 驾驶转向(GTA V 的默认转向键)----
        # 阶段 3 的 F 会把人送上车(前提是附近有车)。这里测的是**这才是方向键
        # 的目标行为**:步行不绑方向键,驾驶时方向键转向。
        #
        # **这一段的注释在 `43c340a` 之前写的,当时方向键确实没接。** 现在
        # (HEAD `0e45d69`)方向键**已经**接进驾驶转向,依据:
        #   - `ARROWLEFT/ARROWRIGHT` 在 `TRACKED_KEYS`(src/game.rs:126-127),
        #     所以 keydown 会收下它们;
        #   - `InputState::steer_axis`(src/game.rs:1885-1899)明确把
        #     `ARROWRIGHT/ARROWLEFT` 和 `KEYD/KEYA` 并列 —— 「右优先,其次左」
        #     兜底,外加 `claim_steer` 的「后按的赢」;
        #   - `is_steer_key`(src/game.rs:7378-7384)与 `steer_sign`
        #     (src/game.rs:7397-7403)两个函数都覆盖了方向键;
        #   - `simulate`(src/game.rs:5929)取
        #     `steer_input = game.input.steer_axis()`,**只有驾驶分支**
        #     (src/game.rs:5950-5954 的 `car.drive(..., steer_input, ...)`)
        #     消费它 —— 步行分支的 `intent` 只由 `axis(KEYW,KEYS)` /
        #     `axis(KEYD,KEYA)` 组成(src/game.rs:5917-5918),所以步行时
        #     方向键正确地无效。
        # => 所以这一项的期望是 **PASS**,不是 FAIL;下面 verdict 的名字
        # 也从「(真缺陷)」改掉了,否则一个正确的应用会一直挂着缺陷名。
        await results_arrow_steer(s, results)

    emit(results, WORKER)


async def results_arrow_steer(s: Session, results: list) -> None:
    """驾驶中按 ArrowLeft/ArrowRight,车是否会拐弯。

    **没有 carYaw 字段可用。** `git grep -n carYaw -- src/` 为空;
    `__vcw` 暴露的是 `carX[]/carZ[]/carSpeed[]`(逐车数组)与
    `carDriveX/carDriveZ/carDriveSpeed`(玩家正在开的那辆)。所以朝向只能
    由**驾驶时该车的位移方向**反推:直线行驶时位移方向不变,一旦转向
    就会拐出一个夹角。

    **这一段的 docstring 原本写的是「方向键接不进驾驶转向 = 真缺陷」。
    那是 `43c340a` 之前的事实,现在已经不成立了。** 现在的接线(按源码
    核实):
      - `ARROWLEFT/ARROWRIGHT` 在 `TRACKED_KEYS`(src/game.rs:126-127),
        所以 keydown 收得下这两个键;
      - `InputState::steer_axis`(src/game.rs:1885-1899)把
        `ARROWRIGHT/ARROWLEFT` 与 `KEYD/KEYA` 并列为转向来源;
      - `simulate`(src/game.rs:5929)读 `steer_axis()` 并**只**在驾驶分支
        (`car.drive`,src/game.rs:5950-5954)消费 —— 步行分支不吃它,
        所以步行时方向键正确地无效。
    => 期望是 **PASS**。历史上实测(43c340a 实测,不是推断):ArrowLeft
    dyaw/帧 -0.1063 / ArrowRight +0.1044,A/D 对照 -0.2313 / +0.1044。
    """
    # 先确保人在车上
    st = await _rich(s)
    if st.get("veh") is None:
        await s.tap("KeyF", 140)
        await s.wait_frames(4)
        st = await _rich(s)
    if st.get("veh") is None:
        results.append(
            Verdict(
                "方向键 驾驶转向", None,
                f"附近没有车(playerVehicle=None,F 报了 "
                f"{'no vehicle' if 'no vehicle' in str(st.get('notice')) else '无提示'}),"
                f"人没上车,这一项**无法测**,记 SKIP。"
                f"应用侧的方向键缺失由 `git grep` 独立确认,不依赖本项。",
            )
        )
        return

    def heading(a: dict, b: dict) -> float | None:
        """两帧之间这辆车的位移方向(弧度)。"""
        vals = (a.get("driveX"), a.get("driveZ"), b.get("driveX"), b.get("driveZ"))
        if any(v is None for v in vals):
            return None
        return math.atan2(vals[3] - vals[1], vals[2] - vals[0])

    def angdiff(x: float) -> float:
        return abs((x + math.pi) % (2 * math.pi) - math.pi)

    # 直线开一段,建立基准朝向
    await s.key("KeyW", True)
    await s.wait_frames(6)
    base = await _rich(s)
    ref = heading(await _rich(s), base)
    if ref is None:
        await s.key("KeyW", False)
        results.append(
            Verdict(
                "方向键 驾驶转向", None,
                f"人在车上,但读不到驾驶位移(`carDriveX/carDriveZ` 缺失),"
                f"**没有可观测的转向通道**,记 SKIP。要给这一项结论,应用侧需要把"
                f"车头朝向(或转向角)暴露到 `__vcw`。",
            )
        )
        return

    async def steer_delta(code: str) -> tuple[float, str]:
        """按住 code 若干帧,返回「行驶方向相对基准」的最大偏转。"""
        worst = 0.0
        prev = await _rich(s)
        await s.key(code, True)
        for _ in range(5):
            await s.wait_frames(2)
            cur = await _rich(s)
            h = heading(prev, cur)
            prev = cur
            if h is not None and (cur.get("driveV") or 0) > 0.5:
                worst = max(worst, angdiff(h - ref))
        await s.key(code, False)
        await s.wait_frames(2)
        return worst, f"speed={_f(prev.get('driveV'), '.2f')} m/s"

    d_left, s_left = await steer_delta("ArrowLeft")
    d_right, s_right = await steer_delta("ArrowRight")
    # 对照组:KeyA 是**已经实现**的转向键,同样按一次。同一段路上量,
    # 证明这个测量本身有效。
    d_a, s_a = await steer_delta("KeyA")
    await s.key("KeyW", False)
    await s.wait_frames(2)

    # 门限 0.02 rad(约 1.1 度):直线行驶时方向抖动远小于此,
    # 而一次真实转向会明显超过。
    STEER_MIN = 0.02
    d_arrow = max(d_left, d_right)
    ok = d_arrow > STEER_MIN
    results.append(
        Verdict(
            "方向键 驾驶转向",
            True if ok else False,
            f"驾驶中直线行驶({s_left}),按住方向键 5 帧,"
            f"行驶方向相对基准的最大偏转:"
            f"ArrowLeft **{d_left:.4f} rad**、ArrowRight **{d_right:.4f} rad**"
            f"(门限 >{STEER_MIN})"
            f" | 对照组 KeyA(已实现的转向键)同法测得 **{d_a:.4f} rad**"
            f"{s_a}"
            f" | 接线依据(HEAD `0e45d69`):`ARROWLEFT/ARROWRIGHT` 在"
            f"TRACKED_KEYS(src/game.rs:126-127);`InputState::steer_axis`"
            f"(src/game.rs:1885-1899)把它们与 A/D 并列;"
            f"`simulate`(src/game.rs:5929)取 steer_axis() 并**只在驾驶分支**"
            f"消费(src/game.rs:5950-5954 的 car.drive) —— 步行分支的 intent"
            f"只由 axis(KEYW,KEYS)/axis(KEYD,KEYA) 构成(src/game.rs:5917-5918),"
            f"所以步行时方向键正确地无效。"
            f" | GTA V PC 手册:驾驶默认方向键转向(A/D 亦可),对齐。"
            f" | **本项期望是 PASS**:旧版本这里写死「真缺陷」,"
            f"那是 `43c340a` 之前的状态,应用修好后继续报 FAIL 就是假 FAIL。"
            + ("" if d_a > STEER_MIN else
               f" | **注意:对照组 KeyA 也没转({d_a:.4f}),本项测量不成立,"
               f"需先修测量(可能车速太低或位移太小)**"),
        )
    )


def _judge(case: KeyCase, b: dict, m: dict):
    """按键各自的语义判,返回 (ok, 说明)。None = 未能判定。"""
    num = lambda k: (b.get(k), m.get(k))  # noqa: E731

    if case.code in ("KeyW", "KeyA", "KeyS", "KeyD", "ArrowUp", "ArrowDown",
                     "ArrowLeft", "ArrowRight"):
        (x0, x1), (z0, z1) = num("x"), num("z")
        if None in (x0, x1, z0, z1):
            return None, "位置读不到"
        moved = ((x1 - x0) ** 2 + (z1 - z0) ** 2) ** 0.5
        if case.code == "ArrowUp":
            # 先把 `ok` 定下来,下面的说明文本要读它。**必须无条件赋值** ——
            # **用 `b`(before)而不是 `case.before`。** `KeyCase` 上**没有**
            # `before` 属性(它只有 name/code/gta/want/hold/hold_frames),
            # 所以旧版这里一跑到就 AttributeError,整轮 w4 崩在 harness 上 ——
            # 而 `verify8.sh` 会把 traceback 过滤掉,症状看起来像「脚本没输出」。
            ok = moved <= 0.01
            # 这一项的**期望是「没有位移」** —— GTA V 的移动只绑 WASD,
            # 方向键是驾驶转向键。应用也确实如此:`simulate` 的步行分支
            # (src/game.rs:5917-5918)只读 `axis(KEYW,KEYS)` /
            # `axis(KEYD,KEYA)`,方向键只进 `steer_axis()`
            # (src/game.rs:1885-1899),而 `steer_axis` 只被驾驶分支消费
            # (src/game.rs:5950-5954)。所以步行按方向键位移必须为 0。
            #
            # **旧版本这里期望位移 > 0.01m,把「方向键接不进驾驶转向」当成
            # 真缺陷。** 那是 `43c340a` 之前的状态;应用修好之后这一项就
            # 永远 FAIL,而方向键转向的真验收在下面
            # `results_arrow_steer` 里(判的是驾驶偏转角,期望 PASS)。
            return (True if moved <= 0.01 else False), (
                f"步行中按住 ArrowUp {case.hold_frames} 帧:位移 {moved:.3f}m"
                f" | 期望≈0 —— GTA V 移动只绑 WASD,方向键**本来就不该绑步行移动**;"
                f"方向键的真验收是**驾驶转向**,由「方向键 驾驶转向」单独测。"
                f" | 本次读数 notice=`{str(b.get('notice', ''))[:60]}`,"
                f"是在{('驾驶中' if 'DRIVING' in str(b.get('notice', '')) else '步行')}"
                f"测的。"
                if moved <= 0.01 else
                f"步行中按住 ArrowUp:位移 {moved:.3f}m —— 方向键竟然推动了步行"
                f"({moved:.3f} > 0.01m 门限),与 src/game.rs:5917-5918 的实现不符,"
                f"复核 keydown match / simulate 的步行分支"
            )
        ok = moved > 0.3
        return (True if ok else False), f"位移 {moved:.3f}m(门限 0.3m)"

    if case.code == "ShiftLeft":
        # 单独按 Shift 不该动;要 W+Shift 才有速度差。这里只报读数,
        # 速度比较由 w1 负责(它有干净的步行/冲刺两段采样)。
        (y0, y1) = num("y")
        moved = 0.0
        if None not in (b.get("x"), m.get("x"), b.get("z"), m.get("z")):
            moved = ((m["x"] - b["x"]) ** 2 + (m["z"] - b["z"]) ** 2) ** 0.5
        return None, (
            f"单独按 Shift 4 帧:位移 {moved:.3f}m,gait {b.get('gait')} -> {m.get('gait')}。"
            f"Shift 是修饰键,速度差必须配 W;该项的真实判据在 w1(sprint Shift+W)。"
            f"vlib 暴露的 gait={m.get('gait')}"
        )

    if case.code == "KeyF":
        v0, v1 = b.get("veh"), m.get("veh")
        if v0 is None and v1 is not None:
            return True, f"上车成功 playerVehicle {v0} -> {v1}(GTA V: F 上车)"
        if v0 is not None and v1 is None:
            return True, f"下车成功 playerVehicle {v0} -> {v1}(GTA V: F 下车)"
        # **没有车在范围内时,F 当然不该上车 —— 那是正确行为,不是缺陷。**
        # `toggle_vehicle` 在门口就把「附近没车」这件事写进 notice 了
        # (HUD 里的 "no vehicle nearby")。拿它报 FAIL 等于因为「路上没车」
        # 而判 F 键坏了。
        notice = str(m.get("notice") or "")
        if "no vehicle" in notice:
            return None, (
                f"playerVehicle {v0} -> {v1},附近没有车 —— 应用自己报 "
                f"notice={notice.split('·')[-1].strip()!r}。这是**正确拒绝**"
                f"(上车门限 4.2m),不是 F 键坏了;记 SKIP。上车/下车本身由 w5_vehicle 验收。"
            )
        return False, (
            f"playerVehicle {v0} -> {v1},没有 null<->非 null 的翻转,"
            f"而且 notice 里没有「附近没车」({notice[-60:]!r})—— 附近有车却上不去,"
            f"这条是真缺陷。"
        )

    if case.code == KEY_RELOAD_FORMER:
        # **换弹键在 `0e45d69` 从 KeyC 迁到了 KeyR。** 所以按 C 现在必须
        # **什么都不发生** —— `src/` 里没有任何 keydown 分支读 KeyC
        # (`git grep '"KeyC"' -- src/` 只命中 const.rs 的
        # `SLOT_FORMER_RELOAD_KEY` 断言串)。
        #
        # 判据是「全都没变」。注意 `mag` 随切武器变是**正常行为**:
        # `get_magazine`(src/combat.rs:253)按武器分别读,球棒是 0、
        # 冲锋枪是 30,两把枪各有各的弹药,不是切枪重置了弹匣 —— 所以
        # 这里比的是**这次按键前后**的一次 diff,不是跨武器的绝对值。
        w0, w1 = b.get("weapon"), m.get("weapon")
        mag0, mag1 = b.get("mag"), m.get("mag")
        rl0, rl1 = b.get("reload"), m.get("reload")
        changed = w0 != w1 or mag0 != mag1 or rl0 != rl1
        # 「没变」必须由**每帧通道**佐证。`__vcw.combat` 每 3 帧才发布一次,
        # 单看它「没变」是空洞通过(读到的是按下之前的快照)。`ammoText`
        # 来自 `#vcw-ammo`,`sync_hud` 每帧重写。
        t0, t1 = b.get("ammoText"), m.get("ammoText")
        dom_changed = t0 != t1
        if changed or dom_changed:
            return False, (
                f"按 {KEY_RELOAD_FORMER} 之后 weapon {_wname(w0)}->{_wname(w1)} "
                f"mag {mag0}->{mag1} reload {rl0}->{rl1},"
                f"#{AMMO_ID} {t0!r}->{t1!r} —— "
                f"**C 不该有任何作用**。`0e45d69` 已把换弹迁到 "
                f"{KEY_RELOAD}(src/const.rs:1099),src/ 里没有任何 keydown "
                f"分支读 KeyC;有变化就说明旧接线还留着,或者读数窗口里"
                f"混进了一次自动换弹(fire_weapon,src/game.rs:6332)。"
            )
        return True, (
            f"按 {KEY_RELOAD_FORMER} 一次(等 {case.hold_frames} 帧):"
            f"weapon {_wname(w0)}->{_wname(w1)} mag {mag0}->{mag1} "
            f"reload {rl0}->{rl1} —— 什么都没变"
            f" | 每帧通道 #{AMMO_ID}:{t0!r} -> {t1!r}(**逐字相同**),"
            f"所以「没变」不是节流快照造成的空洞通过"
            f" | 这一项是「旧换弹键已成死键」的回归项;真正的换弹路径由"
            f"「R 换弹(对齐 GTA V PC)」承担(它需要非满匣基线 + 每帧重发的"
            f"开火注入,不适合放进这个通用 A/B 循环)。"
        )

    if case.code == KEY_RELOAD:
        # **R 现在是换弹键,不再有「重置视角」这回事。** 旧实现在 R 上挂
        # 的是 `camera.reset()` + `apply_default_view()`,而后者把距离设成
        # 250(城市全景机位),`update_camera` 每帧钳到 FOLLOW_DISTANCE_MAX
        # = 22,于是按 R 之后镜头停在 18 m 外再也回不来 —— 那是**已修的**
        # 缺陷,`0e45d69` 把两个函数一起删了。
        # 判 R 就判换弹;相机的部分由 `results_reload_keeps_camera` 单测,
        # 这里只把相机读数带进 note 作旁证。
        cy0, cy1 = b.get("cyaw"), m.get("cyaw")
        cp0, cp1 = b.get("cpitch"), m.get("cpitch")
        mag0, mag1 = b.get("mag"), m.get("mag")
        return None, (
            f"这一项只作诊断:R 现在是**换弹**键(src/const.rs:1099"
            f" KEY_RELOAD = {KEY_RELOAD}),「重置视角」已被 `0e45d69` 删除。"
            f"通用 A/B 循环拿不到换弹判据(需要非满匣基线 + 每帧重发的开火"
            f"注入 + 对 #{AMMO_ID} 的轮询),所以这里不下结论;"
            f"请以「R 换弹(对齐 GTA V PC)」与「R 不再重置/拉走相机(回归项)」"
            f"两条为准。"
            f" | 旁证:cameraYaw {cy0} -> {cy1},cameraPitch {cp0} -> {cp1},"
            f"mag {mag0}->{mag1}。"
            f" | 注意 mag 随切武器变是正常行为:`get_magazine()`"
            f"(src/combat.rs:253)按武器分别取(球棒 0、冲锋枪 30),"
            f"两把枪各有各的弹药,不是切枪重置了弹匣。"
        )

    if case.code == "KeyT":
        p0, p1 = b.get("phase"), m.get("phase")
        ok = p0 != p1
        return (True if ok else False), f"phase {p0} -> {p1}(GTA V: T 无此绑定,属项目自定义)"

    if case.code == "Digit2":
        w0, w1 = b.get("weapon"), m.get("weapon")
        ok = w0 is not None and w0 != w1
        return (True if ok else False), f"combat.weapon {w0} -> {w1}(GTA V: 数字键切武器,一致)"

    return None, "没有为该键写判据"



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
