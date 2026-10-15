"""Headless gameplay probe for vice-city-web.

Drives the running dev server over CDP, restores rAF in a hidden tab, and
measures real gameplay numbers. Every number this prints comes from sampling
the live game; nothing here is estimated.

The hidden-tab trap: in a CDP-driven tab `document.visibilityState` is
`"hidden"`, and a hidden page never fires `requestAnimationFrame`. The game's
frame callback never runs, so the HUD freezes at `init · frame 0 · 0 tris` and
the build looks broken when it is fine. `ensure_raf` forces the page active
before any measurement.

Usage from browser_exec:
    from vcw_probe import ensure_raf, hud, sample, hold_key

Nothing here writes to the repository.
"""

from __future__ import annotations

import json
import math
import time
from typing import Any

CANVAS: str = "document.querySelector('canvas')"

# The browser harness injects `cdp` / `js` / `goto_url` / ... into the calling
# scope, not into this module's. The helpers below call them as plain globals,
# which resolves against `builtins` — so `_bind_harness` stashes them there and
# the functions stay plain callables.
import builtins as _builtins

HARNESS_NAMES: tuple[str, ...] = (
    "cdp",
    "js",
    "goto_url",
    "wait_for_load",
    "ensure_real_tab",
    "new_tab",
)


def _bind_harness() -> bool:
    """Find the harness helpers in the caller's scope and publish them.

    Walks the frame stack outward looking for the scope the browser harness
    executed our code in. Frames belonging to this module are skipped: this
    module's own globals never carry the helpers, and matching them would
    return early with `None` values installed on `builtins`.
    """
    import inspect

    own: dict[str, object] = globals()
    frame = inspect.currentframe()
    frame = frame.f_back if frame is not None else None
    while frame is not None:
        scope: dict[str, object] = frame.f_globals
        if scope is not own:
            for name in HARNESS_NAMES:
                found: object = scope.get(name)
                if callable(found):
                    setattr(_builtins, name, found)
        frame = frame.f_back
    return all(callable(getattr(_builtins, name, None)) for name in HARNESS_NAMES)


def ensure_raf() -> str:
    """Make the tab visible so requestAnimationFrame fires. Returns the state."""
    _bind_harness()
    cdp("Emulation.setFocusEmulationEnabled", enabled=True)
    cdp("Page.setWebLifecycleState", state="active")
    return _js("document.visibilityState")


def _js(expression: str) -> object:
    """Evaluate JS in the page, binding the harness helpers first."""
    _bind_harness()
    return _builtins.js(expression)


def open_game(url: str = "http://localhost:8765/", settle: float = 6.0) -> str:
    """Load the game and wait until frames advance past zero."""
    _bind_harness()
    ensure_raf()
    ensure_real_tab()
    goto_url(url)
    wait_for_load()
    deadline: float = time.time() + 40.0
    while time.time() < deadline:
        ensure_raf()
        time.sleep(1.0)
        if frame_count() > 0:
            time.sleep(settle)
            return "running"
    return "TIMEOUT: frame count stayed 0"


def hud() -> str:
    """The game's own one-line status readout."""
    return _js("document.querySelector('#vcw-hud')?.innerText || ''") or ""


def state(*keys: str) -> dict[str, Any]:
    """Read fields out of the game's published debug object (`window.__vcw`).

    The Rust side publishes one snapshot object per few frames; the bare
    per-field globals the older acceptance scripts used no longer exist, so
    every read goes through this one object.
    """
    if not keys:
        keys = ("__all__",)
    out: dict[str, Any] = {}
    for key in keys:
        out[key] = _js(
            "(function(){try{var s=window['__vcw'];"
            "if(!s)return 'NO_STATE';"
            "var v=%s;"
            "if(v===undefined)return 'UNDEFINED';"
            "return (typeof v==='object'&&v!==null)?JSON.stringify(v).slice(0,400):String(v);"
            "}catch(e){return 'ERR'}})()" % ("s" if key == "__all__" else f"s['{key}']")
        )
    return out


def frame_count() -> int:
    """Frames rendered so far, parsed out of the HUD."""
    import re

    match = re.search(r"frame (\d+)", hud())
    return int(match.group(1)) if match else 0


def world(*keys: str) -> dict[str, float]:
    """Player/car scalars pulled from the HUD and the debug globals."""
    raw: dict[str, Any] = state(*keys)
    clean: dict[str, float] = {}
    for key, value in raw.items():
        try:
            clean[key] = float(value)
        except (TypeError, ValueError):
            clean[key] = float("nan")
    return clean


def position() -> dict[str, float]:
    """Player world position, from the debug globals with an HUD fallback."""
    got: dict[str, float] = world("playerX", "playerY", "playerZ")
    if all(not math.isnan(v) for v in got.values()):
        return got
    import re

    match = re.search(r"xyz ([\-\d.]+) ([\-\d.]+) ([\-\d.]+)", hud())
    if not match:
        return {"playerX": float("nan"), "playerY": float("nan"), "playerZ": float("nan")}
    return {
        "playerX": float(match.group(1)),
        "playerY": float(match.group(2)),
        "playerZ": float(match.group(3)),
    }


def sample(keys: tuple[str, ...], seconds: float = 5.0, hz: float = 5.0) -> list[dict[str, float]]:
    """Poll `keys` at `hz` for `seconds`, returning one row per poll."""
    rows: list[dict[str, float]] = []
    end: float = time.time() + seconds
    while time.time() < end:
        ensure_raf()
        row: dict[str, float] = {"t": time.time()}
        row.update(world(*keys))
        rows.append(row)
        time.sleep(1.0 / hz)
    return rows


def fps(seconds: float = 5.0) -> float:
    """Frames per second, measured over a real interval of rendered frames."""
    start: float = frame_count()
    t0: float = time.time()
    time.sleep(seconds)
    end: int = frame_count()
    elapsed: float = time.time() - t0
    if elapsed <= 0 or end <= start:
        return 0.0
    return (end - start) / elapsed


# CDP virtual key codes for the keys the game tracks. `KeyboardEvent` built in
# page JS never reaches the game: the Rust side binds keydown on `window` and
# reads `KeyboardEvent.code`, and the synthetic event either does not bubble
# the way the wasm closure expects or is dropped before it gets there — a
# synthetic `dispatchEvent(new KeyboardEvent('keydown',{code:'KeyW'}))` moves
# the player 0.000 m. `Input.dispatchKeyEvent` produces a real trusted event
# and does move it.
VK_CODES: dict[str, int] = {
    "KeyW": 87, "KeyA": 65, "KeyS": 83, "KeyD": 68,
    "KeyF": 70, "KeyR": 82, "KeyT": 84, "KeyQ": 81, "KeyE": 69,
    "ShiftLeft": 16, "ShiftRight": 16, "Space": 32,
    "ArrowUp": 38, "ArrowDown": 40, "ArrowLeft": 37, "ArrowRight": 39,
}


def key(code: str, down: bool) -> None:
    """Press or release one key through CDP so the game receives a trusted event."""
    _bind_harness()
    kind: str = "keyDown" if down else "keyUp"
    vk: int = VK_CODES.get(code, 0)
    letter: str = code[3:].lower() if code.startswith("Key") else ""
    cdp(
        "Input.dispatchKeyEvent",
        type=kind,
        code=code,
        key=letter if letter else code,
        windowsVirtualKeyCode=vk,
        nativeVirtualKeyCode=vk,
    )


def hold_key(code: str, seconds: float = 2.0) -> None:
    """Press a key, hold it for `seconds`, release it."""
    key(code, True)
    time.sleep(seconds)
    key(code, False)


def hold_keys(codes: list[str], seconds: float = 3.0, sample_fn=None) -> list[dict[str, float]]:
    """Hold several keys at once (e.g. Shift+W to sprint) for `seconds`.

    Returns the rows collected by `sample_fn` while the keys were down, so a
    caller can measure the movement the keys actually produced.
    """
    for code in codes:
        key(code, True)
    time.sleep(0.2)
    rows: list[dict[str, float]] = []
    if sample_fn is not None:
        rows = sample_fn(seconds)
    else:
        time.sleep(seconds)
    for code in codes:
        key(code, False)
    return rows


def tap(code: str) -> None:
    """Press and release a key with no dwell."""
    _js(
        f"(function(){{"
        f"['keydown','keyup'].forEach(function(t){{"
        f"window.dispatchEvent(new KeyboardEvent(t,{{code:'{code}',bubbles:true}}));"
        f"document.dispatchEvent(new KeyboardEvent(t,{{code:'{code}',bubbles:true}}));"
        f"}});}})()"
    )
    time.sleep(0.2)


def speed_over(rows: list[dict[str, float]], axis: tuple[str, str] = ("playerX", "playerZ")) -> float:
    """Metres per second of horizontal travel across a `sample` run."""
    usable: list[dict[str, float]] = [r for r in rows if not any(math.isnan(r.get(k, float("nan"))) for k in axis)]
    if len(usable) < 2:
        return float("nan")
    first, last = usable[0], usable[-1]
    dt: float = last["t"] - first["t"]
    if dt <= 0:
        return float("nan")
    dist: float = math.hypot(last[axis[0]] - first[axis[0]], last[axis[1]] - first[axis[1]])
    return dist / dt


def screenshot(path: str) -> str:
    """Save a PNG of the viewport and return the path."""
    import os

    os.makedirs(os.path.dirname(path), exist_ok=True)
    _js(
        f"(function(){{var c={CANVAS};if(!c)return 'no canvas';"
        f"window.__shot=c.toDataURL('image/png');return 'ok';}})()"
    )
    data_url: str = _js("window.__shot || ''") or ""
    if not data_url.startswith("data:image/png"):
        return ""
    import base64

    with open(path, "wb") as handle:
        handle.write(base64.b64decode(data_url.split(",", 1)[1]))
    return path


def report(name: str, payload: dict[str, Any]) -> str:
    """Print a finding as one JSON line so results stay machine-parseable."""
    line: str = json.dumps({"name": name, **payload}, ensure_ascii=False)
    print(line)
    return line
