"""Check the stall counter still means 'wedged', now that it windows.

The page-side sampler reports one sample per frame (~0.08 m at walking
speed). The original 0.30 m stall threshold was tuned for a ~0.7 m
stride, so applied per frame it saturated on the first tick, the leg bailed
immediately, and the wall slide never ran. The fix windows the
displacement over STALL_WINDOW samples.

This replays both regimes over a synthetic per-frame trace and asserts the
verdict is unchanged: a free-running walk must NOT count as stalled, and a
wedged walk MUST.
"""
import math

STALL_WINDOW = 6  # must match walk_to
THRESH = 0.30
CLOSING_MIN = -0.45
TRIP = 3


def verdict(positions, target, window=STALL_WINDOW, thresh=THRESH,
            trip=TRIP):
    """Replay the walk_to stall counter. Returns (max_stall, tripped).

    `window` is the number of recent samples the displacement is measured
    across. `window=1` means "against the previous sample", which is what
    the ORIGINAL code did -- the anchor is `positions[i-1]`, not the
    current point, so a moving walker measures a real stride.

    A sample only counts once the window is FULL. While the window is still
    filling, the anchor is the start of the walk and the displacement so far
    is shorter than one stride, so a perfectly free walk would log phantom
    stalls and trip the counter -- the same false termination this window
    was added to prevent.
    """
    stall = 0
    maxstall = 0
    # `span` is how many samples back the anchor sits. window=1 -> the
    # previous sample (the original behaviour); window=N -> N samples back.
    span = max(1, window)
    for i in range(1, len(positions)):
        nx, nz = positions[i]
        if i < span:
            continue          # not enough history yet
        wx, wz = positions[i - span]
        moved = math.hypot(nx - wx, nz - wz)
        closing = ((nx - wx) * (target[0] - wx) + (nz - wz) * (target[1] - wz))
        if moved < thresh or closing < CLOSING_MIN:
            stall += 1
            maxstall = max(maxstall, stall)
        else:
            stall = 0
    return maxstall, maxstall >= trip


def free_walk(target, start, metres, step_per_frame=0.0767, n=None):
    """A walker moving freely at walking speed toward the target."""
    dx, dz = target[0] - start[0], target[1] - start[1]
    g = math.hypot(dx, dz) or 1.0
    n = n or int(metres / step_per_frame)
    return [(start[0] + dx / g * step_per_frame * i,
             start[1] + dz / g * step_per_frame * i) for i in range(n)]


def wedged(target, start, wall_at, n=30):
    """A walker that stops dead `wall_at` metres along its path."""
    dx, dz = target[0] - start[0], target[1] - start[1]
    g = math.hypot(dx, dz) or 1.0
    ux, uz = dx / g, dz / g
    out = []
    for i in range(n):
        d = min(i * 0.0767, wall_at)
        out.append((start[0] + ux * d, start[1] + uz * d))
    return out


def pushed_back(target, start, wall_at, n=30):
    """A wall shoving the walker AWAY from the goal."""
    dx, dz = target[0] - start[0], target[1] - start[1]
    g = math.hypot(dx, dz) or 1.0
    ux, uz = dx / g, dz / g
    out = []
    for i in range(n):
        d = min(i * 0.0767, wall_at) - max(0, i - 6) * 0.05
        out.append((start[0] + ux * d, start[1] + uz * d))
    return out


fails = []


def check(name, cond, detail=""):
    print(f"  {'ok  ' if cond else 'FAIL'} {name} {detail}")
    if not cond:
        fails.append(name)


T = (0.0, -8.0)
S = (0.0, 0.0)

print("== per-frame samples (the new regime) ==")
mx, trip = verdict(free_walk(T, S, 5.0), T)
check("free walk is not stalled", not trip, f"max_stall={mx}")
check("free walk barely counts", mx <= 1, f"max_stall={mx}")

# A 6-sample window needs W-1 frames to decay plus TRIP to confirm, so the
# trace must be long enough to contain that; 24 samples was one short and
# made a genuinely wedged walk look un-tripped.
mx, trip = verdict(wedged(T, S, 1.5), T)
check("wedged walk trips the counter", trip, f"max_stall={mx}")

mx, trip = verdict(wedged(T, S, 0.0), T)
check("wedged from frame 0 trips the counter", trip, f"max_stall={mx}")

mx, trip = verdict(pushed_back(T, S, 1.5), T)
check("pushed-back walk trips the counter", trip, f"max_stall={mx}")

print("== the old coarse stride regime, for comparison ==")
# A coarse sampler sees ~0.7 m per sample.
# 5 m, not 8: past the 8 m target `closing` correctly goes negative,
# which is the 'walker overshot' signal, not a stall.
coarse = [p for i, p in enumerate(free_walk(T, S, 5.0)) if i % 9 == 0]
# Model the ORIGINAL predicate
# exactly: one sample per verdict, no window, so the guard that skips
# samples while a window fills does not apply.
mx, trip = verdict(coarse, T, window=1, thresh=0.30)
check("coarse free walk not stalled (matches old)", not trip, f"max_stall={mx}")

print("== the bug this fixed ==")
# Per-frame window=1 is the broken regime: every sample is under 0.30 m.
mx, trip = verdict(free_walk(T, S, 5.0), T, window=1, thresh=0.30)
check("window=1 WOULD have false-tripped (the regression)",
      trip, f"max_stall={mx}")

print()
print("ALL STALL CHECKS PASS" if not fails else f"FAILURES: {fails}")
raise SystemExit(1 if fails else 0)
