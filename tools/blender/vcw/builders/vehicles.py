"""Category B: NEON BAY street vehicles.

Every vehicle shares one kit so the traffic reads as a single fleet:

* a **lofted rounded-rect shell** running along the length axis (``core.
  section_rings`` + ``core.loft`` over ``core.rounded_rect`` box sections --
  never a circle), driven by a silhouette control table plus a wheel-arch /
  nose / tail **sill** function;
* a second loft for the **greenhouse**, which also supplies the windscreen and
  backlight ramps;
* four Y-axis **wheels** -- dark tyre plus a real five-spoke wheel (dark centre
  disc, five lofted tapered spokes, a rim ring) -- tucked under the arches;
* separate parts for **glass**, **lamps**, **trim**, **plates** and the tyres;
* a **detail kit**: slatted grille, lower intake and splitter, fender louvres,
  flush door pulls, beltline strip, fuel cap, plate recesses, screen wipers,
  arch lips and panel shut lines;
* **paint zoning** through ``core.recolor_faces_where`` -- the single biggest
  quality win here, because the runtime shader is
  ``base * (ambient + light * max(dot(n, l), 0))`` with no specular and no
  texture, so albedo zones and normal breaks are the ONLY things that can
  make two adjacent polygons differ.

Authoring is Blender-style Z-up with one unit = one metre and the origin at
the centre of the ground footprint.  ``core.section_rings`` maps a profile's
horizontal axis onto world +Y ("vehicle width") and its vertical axis onto
world +Z, so the loft advances along the LENGTH axis X: **-X is the nose,
+X is the tail**, and Y is the width axis.

Nothing is positioned by hand: every surface-mounted detail asks the shell for
its own half-width / roof height at that (x, z) -- see :func:`_flank_y`,
:func:`_top_z` and :func:`_ring` -- so trim cannot drift off the rounded
shoulders and become floating slivers.  Everything is driven by optional
config keys, so a new vehicle is a new control table plus a dict.

All geometry is original and procedural -- nothing is imported or traced.
"""

import math

from .. import core as C


# ----------------------------------------------------------------- palette --

TYRE = (0.075, 0.075, 0.085)
HUB = (0.60, 0.62, 0.65)
HUB_DK = (0.16, 0.17, 0.19)
RIM = (0.68, 0.70, 0.73)
GLASS = (0.055, 0.090, 0.130)
BUS_GLASS = (0.035, 0.070, 0.125)
CHROME = (0.74, 0.77, 0.80)
TRIM_DK = (0.13, 0.14, 0.16)
GRILLE_DK = (0.055, 0.058, 0.065)
GRILLE_SLAT = (0.30, 0.32, 0.35)
HEADLIGHT = (1.00, 0.95, 0.76)
TAILLIGHT = (0.80, 0.10, 0.11)
PLATE = (0.86, 0.85, 0.78)
AMBER = (1.00, 0.62, 0.10)
POLICE_RED = (1.00, 0.12, 0.12)
POLICE_BLUE = (0.16, 0.34, 1.00)

# NEON BAY original liveries (no reference vehicles).
WHITE = (0.93, 0.93, 0.91)
MIAMI_PINK = (0.94, 0.36, 0.52)
MIAMI_TEAL = (0.10, 0.62, 0.62)
CORAL = (0.93, 0.42, 0.26)
TAXI_YELLOW = (0.98, 0.78, 0.10)
COPPER_DK = (0.55, 0.34, 0.16)
POLICE_BLACK = (0.09, 0.10, 0.13)
BUS_CREAM = (0.90, 0.88, 0.82)

# Default chamfer for every solid box we bolt to a body.  The runtime has no
# specular, so an arris highlight is the only thing that makes a hard edge
# read at all; 13 mm is the smallest bevel that survives the smallest panel
# thickness below without being clamped to nothing.
_BEVEL = 0.013


# ------------------------------------------------------- silhouette reading --
# Control rows are (x, half_width, z_top, corner_radius) sampled along the
# length axis.  The sill line below them comes from the wheel arches and the
# nose/tail lift functions.


def _field(ctrl, x, k):
    """Linear interpolation of column ``k`` of a (x, ...) control table."""
    if x <= ctrl[0][0]:
        return ctrl[0][k]
    for i in range(len(ctrl) - 1):
        a, b = ctrl[i], ctrl[i + 1]
        if a[0] <= x <= b[0]:
            span = b[0] - a[0]
            t = 0.0 if span <= 0.0 else (x - a[0]) / span
            return a[k] + (b[k] - a[k]) * t
    return ctrl[-1][k]


def _arch_z(x, cfg):
    """Sill line: rides at ``sill`` but bulges to ``arch_top`` over each axle."""
    z = cfg["sill"]
    for ax in cfg["axles"]:
        d = abs(x - ax) / cfg["arch_half"]
        if d < 1.0:
            z = max(z, cfg["sill"]
                    + (cfg["arch_top"] - cfg["sill"]) * (1.0 - d * d))
    return z


def _end_z(x, cfg):
    """Sill lift over the nose and the tail (approach / departure angle)."""
    z = cfg["sill"]
    for ex, ez in cfg["ends"]:
        t = abs(x - ex) / cfg["end_span"]
        if t < 1.0 and ez > z:
            z = max(z, cfg["sill"] + (ez - cfg["sill"]) * (1.0 - t))
    return z


def _shell_z0(cfg, x):
    """Bottom of the shell ring at station x."""
    return max(_arch_z(x, cfg), _end_z(x, cfg))


def _arch_xs(cfg):
    """Extra stations so the wheel arch is faceted, not one steep ramp."""
    ah = cfg["arch_half"]
    n = cfg.get("arch_segs", 5)
    out = []
    for ax in cfg["axles"]:
        for i in range(n):
            out.append(ax + ah * (-1.0 + 2.0 * i / float(n - 1)))
    return out


def _ring(ctrl, z0_fn, x):
    """(half_width, half_height, lift, radius, z_bottom, z_top) at station x.

    This mirrors exactly what ``_shell_rings`` hands to ``core.rounded_rect``,
    so a detail placed with it lands on the surface that actually gets lofted.
    """
    z_top = _field(ctrl, x, 2)
    z_bot = min(z0_fn(x), z_top - 0.05)
    hx = max(0.05, _field(ctrl, x, 1))
    hy = max(0.02, (z_top - z_bot) * 0.5)
    # rounded_rect clamps r itself, but clamping here as well keeps every ring
    # at the same point count (a radius clamped to zero drops the arcs and
    # changes the ring cardinality, which loft rejects).
    r = max(0.02, min(_field(ctrl, x, 3), hx * 0.9, hy * 0.9))
    return hx, hy, (z_top + z_bot) * 0.5, r, z_bot, z_top


def _half_w(ring, z):
    """Half-width of a rounded-rect ring at height ``z``."""
    hx, hy, lift, r, _z_bot, _z_top = ring
    py = max(-hy, min(hy, z - lift))
    flat = hy - r
    if abs(py) <= flat:
        return hx
    d = abs(py) - flat
    return (hx - r) + math.sqrt(max(0.0, r * r - d * d))


def _top_at(ring, y):
    """Height of a rounded-rect ring's top surface at lateral offset ``y``.

    ``None`` when ``y`` is off the profile entirely, so a caller can skip a
    detail that would hang in mid-air.
    """
    hx, hy, lift, r, _z_bot, z_top = ring
    d = abs(y) - (hx - r)
    if d >= r:
        return None
    if d < 0.0:
        return z_top
    return lift + (hy - r) + math.sqrt(max(0.0, r * r - d * d))


def _shell_rings(ctrl, z0_fn, extra_xs=(), n_corner=3):
    """Rounded-rect box-section rings along X for a silhouette control table."""
    xs = set(round(r[0], 6) for r in ctrl)
    xs.update(round(x, 6) for x in extra_xs)
    lo, hi = ctrl[0][0], ctrl[-1][0]
    xs = sorted(x for x in xs if lo - 1e-9 <= x <= hi + 1e-9)

    def z_top(x):
        return _field(ctrl, x, 2)

    def z_bot(x):
        return min(z0_fn(x), z_top(x) - 0.05)

    def hx(x):
        return max(0.05, _field(ctrl, x, 1))

    def hy(x):
        return max(0.02, (z_top(x) - z_bot(x)) * 0.5)

    def rad(x):
        return max(0.02, min(_field(ctrl, x, 3), hx(x) * 0.9, hy(x) * 0.9))

    def lift(x):
        return (z_top(x) + z_bot(x)) * 0.5

    return C.section_rings(xs, hx, hy, rad, n_corner=n_corner, lift_fn=lift)


def _body_ring(cfg, x):
    return _ring(cfg["shell"], lambda t: _shell_z0(cfg, t), x)


def _flank_y(cfg, x, z):
    """Half-width of the OUTER surface at (x, z): the greenhouse if it covers
    that point, otherwise the body shell."""
    cab = cfg.get("cabin")
    if cab is not None:
        lo, hi = cab["ctrl"][0][0], cab["ctrl"][-1][0]
        if lo - 1e-6 <= x <= hi + 1e-6 and z >= cab["z0"]:
            r = _ring(cab["ctrl"], lambda t: cab["z0"], x)
            if z <= r[5]:
                return _half_w(r, z)
    return _half_w(_body_ring(cfg, x), z)


def _top_z(cfg, x, y):
    """Height of the uppermost lofted surface above (x, y).

    Bonnet, boot and roof details ride this, so they follow the real lofted
    skin instead of a straight line between control-table rows.
    """
    z = _top_at(_body_ring(cfg, x), y)
    cab = cfg.get("cabin")
    if cab is not None:
        lo, hi = cab["ctrl"][0][0], cab["ctrl"][-1][0]
        if lo - 1e-6 <= x <= hi + 1e-6:
            zc = _top_at(_ring(cab["ctrl"], lambda t: cab["z0"], x), y)
            if zc is not None:
                z = zc if z is None else max(z, zc)
    return z


def _roof_z(cfg):
    z = max(r[2] for r in cfg["shell"])
    cab = cfg.get("cabin")
    if cab is not None:
        z = max(z, max(r[2] for r in cab["ctrl"]))
    return z


def _part(a, name, **kw):
    """Get-or-create ``name`` on the asset.

    Detail hooks run after the main pass, so several of them append to a part
    the vehicle builder already made.  The exporter rejects duplicate part
    names outright, so a hook must never call ``asset.part`` for one.
    """
    for p in a.parts:
        if p.name == name:
            if kw:
                if "base_color" in kw:
                    p.base_color = tuple(kw["base_color"])
                if "emissive" in kw:
                    p.emissive = tuple(kw["emissive"])
            return p
    return a.part(name, **kw)


# ------------------------------------------------------------- primitives ---

def _slab(m, pts, push, color):
    """Closed 4-sided slab: quad ``pts`` extruded by ``push``.

    ``pts`` is the OUTER face; the slab grows along ``push`` into the body.
    ``core.loft`` advances ring-to-ring along the ring's right-hand normal, so
    that normal must point along ``push``; this only fixes the corner order.
    The outer cap is then the flipped ``cap_start``.
    """
    pts = [tuple(p) for p in pts]
    p = C.normalize(push)
    n = C.normalize(C.cross(C.sub(pts[1], pts[0]), C.sub(pts[2], pts[0])))
    if C.dot(n, p) < 0.0:
        pts = list(reversed(pts))
    C.loft(m, [pts, [C.add(q, push) for q in pts]], color,
           cap_start_flip=True, cap_end_flip=False)
    return m


def _merge_xform(part, tmp, center=(0.0, 0.0, 0.0), rot=()):
    """Merge a temporary mesh after a chain of (axis, degrees) rotations.

    ``rot`` is applied in order, so ('Z', skew) then ('Y', ramp) lays a box
    into the plane spanned by the skew axis and the ramp -- exactly what a
    screen wiper is.
    """
    m = part.mesh
    base = len(m.pos)
    for p in tmp.pos:
        q = p
        for axis, deg in rot:
            a = math.radians(deg)
            ca, sa = math.cos(a), math.sin(a)
            if axis == "X":
                q = (q[0], q[1] * ca - q[2] * sa, q[1] * sa + q[2] * ca)
            elif axis == "Y":
                q = (q[0] * ca + q[2] * sa, q[1], -q[0] * sa + q[2] * ca)
            else:
                q = (q[0] * ca - q[1] * sa, q[0] * sa + q[1] * ca, q[2])
        m.pos.append((q[0] + center[0], q[1] + center[1], q[2] + center[2]))
    for f in tmp.faces:
        m.faces.append((f[0] + base, f[1] + base, f[2] + base))
    m.fcol.extend(tmp.fcol)
    return part


def _cbx(part, size, center, color, bevel=_BEVEL):
    """One chamfered box: 28 triangles, but every arris catches the light."""
    C.chamfer_box(part.mesh, size, center=center, color=color, bevel=bevel)
    return part


def _ramp_glass(m, xa, za, ha, xb, zb, hb, ref, color, lift=0.014, thick=0.05):
    """Windscreen / backlight pane lying on a greenhouse ramp.

    ``xa,za`` is the lower edge, ``xb,zb`` the upper one; ``ref`` is a point
    inside the cabin and is only used to pick which of the two perpendicular
    directions points outward.
    """
    dx, dz = xb - xa, zb - za
    nx, nz = -dz, dx
    n = math.hypot(nx, nz)
    if n < 1e-9:
        return m
    nx, nz = nx / n, nz / n
    mx, mz = (xa + xb) * 0.5, (za + zb) * 0.5
    if nx * (mx - ref[0]) + nz * (mz - ref[1]) < 0.0:
        nx, nz = -nx, -nz
    ex, ez = nx * lift, nz * lift
    quad = [(xa + ex, -ha, za + ez),
            (xa + ex, ha, za + ez),
            (xb + ex, hb, zb + ez),
            (xb + ex, -hb, zb + ez)]
    return _slab(m, quad, (-nx * thick, 0.0, -nz * thick), color)


def _side_glass(m, hw, x1, x2, z1, z2, color, lift=0.014, thick=0.05):
    """One flat side pane on both flanks, sitting just proud of ``hw``."""
    for sy in (-1, 1):
        yy = sy * (hw + lift)
        quad = [(x1, yy, z1), (x2, yy, z1), (x2, yy, z2), (x1, yy, z2)]
        _slab(m, quad, (0.0, -sy * thick, 0.0), color)
    return m


def _flank_pane(cfg, m, x0, x1, z0, z1, color, lift=0.012, thick=0.05, nseg=6,
                sides=(-1, 1)):
    """A long side pane that follows the actual flank instead of a flat plane.

    The bus flank is 2.5 m wide by 2.3 m tall; a single flat quad across that
    would visibly leave the skin at the shoulders.  Sampling ``_flank_y`` at
    each station keeps the glass on the surface at any station.  ``sides``
    restricts it to one flank (a driver's window is kerb side only).
    """
    zc = (z0 + z1) * 0.5
    prev = None
    for i in range(nseg + 1):
        x = x0 + (x1 - x0) * (i / float(nseg))
        cur = (x, _flank_y(cfg, x, zc) + lift)
        if prev is not None:
            for sy in sides:
                quad = [(prev[0], sy * prev[1], z0), (cur[0], sy * cur[1], z0),
                        (cur[0], sy * cur[1], z1), (prev[0], sy * prev[1], z1)]
                _slab(m, quad, (0.0, -sy * thick, 0.0), color)
        prev = cur
    return m


def _window_row(cfg, m, x0, x1, z0, z1, n, color, pillar=0.075, part=None):
    """A glazed band split by real pillars into ``n`` separate windows.

    One continuous pane 9 m long reads as a black stripe, not as a bus: the
    rhythm of pillars IS the visual signature of a transit bus.  Each pane is
    built from two segments so it follows the flank, and the pillars between
    them are body-coloured bars standing 8 mm proud of the glass.
    """
    w = (x1 - x0) / n
    for i in range(n):
        xa = x0 + i * w + pillar * 0.5
        xb = x0 + (i + 1) * w - pillar * 0.5
        _flank_pane(cfg, m, xa, xb, z0, z1, color, lift=0.012, thick=0.06,
                    nseg=2)
    tgt = m if part is None else part
    for i in range(1, n):
        x = x0 + i * w
        _flank_pair(cfg, tgt, x, (z0 + z1) * 0.5, (pillar, 0.0, z1 - z0),
                    cfg.get("pillar_color", (0.30, 0.32, 0.35)),
                    out=0.014, thick=0.026, chamfer=True)
    return m


def _bx(part, size, center, color):
    C.box(part.mesh, size, center=center, color=color)
    return part


def _pair(part, size, center, color):
    x, y, z = center
    for sy in (-1, 1):
        C.box(part.mesh, size, center=(x, sy * y, z), color=color)
    return part


def _cbx_pair(part, size, center, color, bevel=_BEVEL):
    x, y, z = center
    for sy in (-1, 1):
        _cbx(part, size, (x, sy * y, z), color, bevel)
    return part


def _flank_pair(cfg, part, x, z, size, color, out=0.008, thick=0.020,
                chamfer=False):
    """A pair of thin boxes glued to BOTH flanks at (x, z).

    ``size`` is (dx, dy, dz); ``out`` is the gap between the box's inner face
    and the shell surface at that point, so the detail follows the fenders
    instead of drifting off the rounded shoulder.
    """
    y = _flank_y(cfg, x, z) + out + thick * 0.5
    if chamfer:
        _cbx_pair(part, (size[0], thick, size[2]), (x, y, z), color)
    else:
        _pair(part, (size[0], thick, size[2]), (x, y, z), color)
    return part


def _flank_line(cfg, part, x, z0, z1, color, out=0.006, segs=3):
    """A vertical cut-line down both flanks, sampled so it hugs the shell."""
    for i in range(segs):
        za = z0 + (z1 - z0) * (i / float(segs))
        zb = z0 + (z1 - z0) * ((i + 1) / float(segs))
        zm = (za + zb) * 0.5
        y = _flank_y(cfg, x, zm) + out + 0.010
        _pair(part, (0.020, 0.018, zb - za + 0.006), (x, y, zm), color)
    return part


def _flank_strip(cfg, part, x0, x1, z, color, out=0.006, segs=5, h=0.028):
    """A horizontal moulding (beltline) following both flanks from x0 to x1."""
    for i in range(segs):
        xa = x0 + (x1 - x0) * (i / float(segs))
        xb = x0 + (x1 - x0) * ((i + 1) / float(segs))
        xm = (xa + xb) * 0.5
        y = _flank_y(cfg, xm, z) + out + 0.011
        _cbx_pair(part, (xb - xa + 0.004, 0.022, h), (xm, y, z), color)
    return part


def _up_quad(part, quad, color):
    """Emit ``quad`` with its normal forced to point up.

    A strip laid on a curved skin can come out wound either way depending on
    which way the surface falls, and the shut-line part declares its outward
    direction as +Z -- so the corner order is normalised here rather than
    making every caller think about it.
    """
    n = C.normalize(C.cross(C.sub(quad[1], quad[0]), C.sub(quad[2], quad[0])))
    if n[2] < 0.0:
        quad = list(reversed(quad))
    part.mesh.quad(quad[0], quad[1], quad[2], quad[3], color)
    return part


def _top_line_x(cfg, part, x0, x1, y, width, color, n=4, out=0.004):
    """A dark inset panel gap running along X across a horizontal skin."""
    w = width * 0.5
    samples = []
    for i in range(n + 1):
        x = x0 + (x1 - x0) * (i / float(n))
        z = _top_z(cfg, x, y)
        if z is not None:
            samples.append((x, z))
    for (xa, za), (xb, zb) in zip(samples, samples[1:]):
        _up_quad(part, [(xa, y - w, za + out), (xb, y - w, zb + out),
                        (xb, y + w, zb + out), (xa, y + w, za + out)], color)
    return part


def _top_line_y(cfg, part, x, y0, y1, width, color, n=4, out=0.004):
    """The same, running along Y (the closing cut of a bonnet or boot lid)."""
    w = width * 0.5
    samples = []
    for i in range(n + 1):
        y = y0 + (y1 - y0) * (i / float(n))
        z = _top_z(cfg, x, y)
        if z is not None:
            samples.append((y, z))
    for (ya, za), (yb, zb) in zip(samples, samples[1:]):
        _up_quad(part, [(x - w, ya, za + out), (x - w, yb, zb + out),
                        (x + w, yb, zb + out), (x + w, ya, za + out)], color)
    return part


def _zone_paint(body, cfg, paint):
    """Zone the paint with ``face_colors`` -- the cheapest quality win there is.

    The runtime shader is ``base * (ambient + light * max(dot(n, l), 0))``:
    no specular, no texture.  Two adjacent polygons on the same shell differ
    ONLY if their normals differ (see ``chamfer_box``) or their albedo does.
    Splitting the body into a dark rocker, a bright upper shoulder, a bright
    horizontal deck and a darker roof is what stops a coloured shell reading
    as one dead value.
    """
    paint = tuple(paint)
    roof = _roof_z(cfg)
    belt = (cfg.get("beltline") or (0.0, 0.0, 0.0))[2]
    rocker = cfg["sill"] + 0.13
    # 1. undersides: in permanent shadow, so drop them right down.
    C.recolor_faces_where(body, lambda n, c: n[2] < -0.55, C.shade(paint, 0.50))
    # 2. rocker / lower body: 13 cm above the sill, or under the beltline.
    C.recolor_faces_where(
        body, lambda n, c: abs(n[2]) < 0.55 and c[2] < rocker,
        C.shade(paint, 0.74))
    # 3. every upward-facing deck (bonnet, boot, wheel-arch tops) brightens.
    C.recolor_faces_where(body, lambda n, c: n[2] > 0.55, C.shade(paint, 1.07))
    # 4. ... except the roof, which is the one large horizontal plane on the
    #    greenhouse and reads better a shade down from the shoulders.
    C.recolor_faces_where(
        body, lambda n, c: n[2] > 0.55 and c[2] >= roof - 0.03,
        C.shade(paint, 0.88))
    # 5. a narrow bright band under the beltline: the classic "shoulder line"
    #    every production car has, and free here.
    if belt > 0.0:
        C.recolor_faces_where(
            body, lambda n, c: abs(n[2]) < 0.55 and belt - 0.15 < c[2] < belt,
            C.shade(paint, 1.06))
    # 6. optional livery stripe band zoned straight into the paint.
    stripe = cfg.get("stripe")
    if stripe:
        z0, z1, col = stripe
        C.recolor_faces_where(
            body, lambda n, c: abs(n[2]) < 0.62 and z0 < c[2] < z1, col)
    return body


# ------------------------------------------------------- end-cap features ---

def _cap_ring(cfg, front):
    """The closed cap the loft puts on the nose (front) or tail (rear)."""
    x = cfg["shell"][0][0] if front else cfg["shell"][-1][0]
    return _body_ring(cfg, x), x


def _cap_band(ring, x_cap, push, frac_lo, frac_hi, y_frac, depth, bite=0.02):
    """(size, centre) for a panel laid onto the end cap between two height
    fractions.  ``push`` is -1 for the nose, +1 for the tail; ``bite`` is how
    far the panel's inner face reaches back INTO the cap, so lamps and grilles
    are set into the bodywork instead of hovering in front of it."""
    z_bot, z_top = ring[4], ring[5]
    h = z_top - z_bot
    za = z_bot + h * frac_lo
    zb = z_bot + h * frac_hi
    y_out = min(_half_w(ring, za), _half_w(ring, zb)) * y_frac
    y_in = max(0.0, y_out - 0.30)
    size = (depth, y_out - y_in, zb - za)
    center = (x_cap + push * (depth * 0.5 - bite), (y_out + y_in) * 0.5,
              (za + zb) * 0.5)
    return size, center


def _cap_pane(m, ring, x_cap, push, z0, z1, y_frac, lift, thick, color):
    """A flat panel on an end cap whose width TAPERS with the cap.

    A ``_cap_band`` box spans a constant width, so on a tall nose it would poke
    through the rounded shoulders.  Sizing each edge with ``_half_w`` keeps the
    panel on the cap silhouette at both heights.

    ``x_cap`` is the plane the panel's OUTER face sits on; the slab grows
    ``thick`` BACKWARD along ``-push``, into the cap.  Passing the *front* of
    the bumper (not the cap plane) is what stops a number plate from growing
    the car's overall length.
    """
    ya = _half_w(ring, z0) * y_frac
    yb = _half_w(ring, z1) * y_frac
    x = x_cap + push * lift
    quad = [(x, -ya, z0), (x, ya, z0), (x, yb, z1), (x, -yb, z1)]
    return _slab(m, quad, (-push * thick, 0.0, 0.0), color)


def _face_screen(m, cfg, front, fracs, y_frac, color, lift=0.016, thick=0.05):
    """A full-height screen standing on a vertical end face.

    A bus does not have a raked windscreen: its front is a flat wall with a
    huge two-piece screen dropped into it.  ``_ramp_glass`` (a pane on a
    sloping ramp) is the wrong primitive for that, so the flat-cap version
    gets its own call.
    """
    ring, x_cap = _cap_ring(cfg, front)
    push = -1.0 if front else 1.0
    z0, z1 = _cap_fracs(ring, fracs[0], fracs[1])
    return _cap_pane(m, ring, x_cap, push, z0, z1, y_frac, lift, thick, color)


def _cap_bumper(ring, x_cap, push, height=0.19, depth=0.16, bite=0.055):
    """A bumper bar wrapped around the end cap, as wide as the cap allows.

    ``bite`` is how far the bar's inner face reaches back INTO the body, so the
    bumper is bolted on rather than floating a gap in front of the cap.
    Returns ``(size, centre, outermost_plane)`` -- the last one is what the
    number plate has to sit on.
    """
    y = _half_w(ring, ring[4] + (ring[5] - ring[4]) * 0.30) * 1.02
    z = ring[4] + (ring[5] - ring[4]) * 0.10
    size = (depth, y * 2.0, height)
    center = (x_cap + push * (depth * 0.5 - bite), 0.0, z)
    return size, center, x_cap + push * (depth - bite)


def _cap_fracs(ring, lo, hi):
    z_bot, z_top = ring[4], ring[5]
    h = z_top - z_bot
    return z_bot + h * lo, z_bot + h * hi


# ---------------------------------------------------------------- wheels ----

def _spoke_mesh(ri, ro, wi, wo, t, color):
    """One tapered wheel spoke as a closed 4-gon lofted along its own radius."""
    tmp = C.Mesh()
    inner = [(-t * 0.5, -wi * 0.5), (t * 0.5, -wi * 0.5),
             (t * 0.5, wi * 0.5), (-t * 0.5, wi * 0.5)]
    outer = [(-t * 0.5, -wo * 0.5), (t * 0.5, -wo * 0.5),
             (t * 0.5, wo * 0.5), (-t * 0.5, wo * 0.5)]
    # Ring order is CCW in (Y, Z) so its right-hand normal is +X, which is the
    # direction loft() advances in.
    C.loft(tmp,
           [[(ri, y, z) for (y, z) in inner], [(ro, y, z) for (y, z) in outer]],
           color, cap_start_flip=True, cap_end_flip=False)
    return tmp


def _wheels_cfg(cfg):
    """Per-corner wheel stations as ``(x, track, width, has_rim)``.

    Defaults to one wheel per axle at ``cfg["wheel"]``; a vehicle with duals
    (the bus) lists them out explicitly, which is cheaper and far clearer than
    a second radius table.  ``has_rim`` is False for the inner wheel of a
    duals pair: it is never seen, so it only needs the tyre carcass.
    """
    rows = cfg.get("wheels")
    if rows:
        return [tuple(r) for r in rows]
    _wr, ww, track = cfg["wheel"]
    return [(ax, track, ww, True) for ax in cfg["axles"]]


def _wheel(cfg, tyres, hubs, x, track, ww, sy, spokes=5, rim=True):
    """One wheel: tyre plus a real five-spoke rim.

    THE TYRE IS A SOLID CYLINDER, so its end cap is a full disc of radius ``wr``
    at the wheel's outer plane.  Anything drawn at or behind that plane is
    invisible -- which is exactly what a naively centred hub is, and the
    reason a "detailed" wheel renders as a flat dark circle.  Every piece of
    the rim is therefore placed FORWARD of the tyre face ``yf``, dished back
    in three steps so the wheel still reads as a wheel and not a disc:

        rim ring  yf + 0.004  (proudest: the outer lip)
        spokes    yf - 0.006
        hub disc  yf - 0.014  (recessed, and dark)

    ``rim`` is False for an inner wheel of a duals pair: that one is hidden
    behind its partner and only needs the tyre carcass.
    """
    wr = cfg["wheel"][0]
    C.cylinder(tyres.mesh, wr, ww, seg=12, center=(x, sy * track, wr),
               color=TYRE, axis="Y")
    if not rim:
        return tyres
    yf = track + ww * 0.5                  # outer plane of the tyre
    n = max(3, int(spokes))
    # rim ring: a faceted torus, flat shaded so it reads as a machined lip.
    C.torus(hubs.mesh, wr * 0.68, wr * 0.090, seg_u=10, seg_v=3,
            center=(x, sy * (yf + 0.004), wr), color=RIM, axis="Y",
            smooth=False)
    # dark centre disc, recessed between the spokes
    C.cylinder(hubs.mesh, wr * 0.21, 0.044, seg=8,
               center=(x, sy * (yf - 0.014), wr), color=HUB_DK, axis="Y")
    spoke = _spoke_mesh(wr * 0.17, wr * 0.66, wr * 0.24, wr * 0.11, 0.066,
                        HUB)
    for k in range(n):
        _merge_xform(hubs, spoke, center=(x, sy * (yf - 0.006), wr),
                     rot=(("Y", 90.0 * k / n + 18.0),))
    return tyres


def _arch_lip(cfg, trim, ax):
    """A thin flare ring swept round one wheel arch.

    ``core.tube`` with ``smooth=False`` is a faceted closed shell for 22
    triangles, and the path is sampled off the SAME arch parabola the shell
    sill uses, so the flare can never drift away from the opening.
    """
    ah = cfg["arch_half"]
    path = []
    for i in range(11):
        t = 0.10 + 0.80 * (i / 10.0)
        x = ax - ah + 2.0 * ah * t
        z = _arch_z(x, cfg) + 0.018
        y = _flank_y(cfg, x, z) + 0.006
        path.append((x, y, z))
    for sy in (-1, 1):
        C.tube(trim.mesh, [(p[0], sy * p[1], p[2]) for p in path],
               0.026, seg=5, color=cfg.get("lip_color", TRIM_DK), caps=True,
               smooth=False)
    return trim


# -------------------------------------------------------- front-end detail --

def _grille(trim, ring, x_cap, push, fracs, n_slats, y_frac=0.96):
    """Real horizontal slats in the grille aperture.

    A single dark box is a decal; the value break only appears once there is a
    near-black backing panel AND bars standing a few millimetres in front of
    it, so the eye reads depth in a region that is otherwise one flat value.
    """
    z0, z1 = _cap_fracs(ring, fracs[0], fracs[1])
    relief = min(0.030, (z1 - z0) * 0.30)
    _cap_pane(trim.mesh, ring, x_cap, push, z0, z1, y_frac,
              relief * 0.35, relief * 1.6, GRILLE_DK)
    n = max(1, int(n_slats))
    gap = (z1 - z0) / (n + 1)
    bh = min(0.024, gap * 0.40)
    for i in range(n):
        z = z0 + gap * (i + 1)
        hw = _half_w(ring, z) * y_frac * 0.93
        _cbx(trim, (relief * 1.1, hw * 2.0, bh),
             (x_cap + push * (relief * 0.72), 0.0, z), GRILLE_SLAT,
             bevel=relief * 0.28)
    return trim


def _intake(trim, ring, x_cap, push, fracs, n_slats, y_frac=0.90, face=0.0):
    """Lower front air intake plus the splitter lip under it.

    ``face`` is the outermost bumper plane: the splitter is measured BACK from
    it so the lip cannot lengthen the car.
    """
    z0, z1 = _cap_fracs(ring, fracs[0], fracs[1])
    relief = min(0.026, (z1 - z0) * 0.28)
    _cap_pane(trim.mesh, ring, x_cap, push, z0, z1, y_frac,
              relief * 0.35, relief * 1.6, GRILLE_DK)
    n = max(1, int(n_slats))
    for i in range(n):
        z = z0 + (z1 - z0) * ((i + 1) / float(n + 1))
        hw = _half_w(ring, z) * y_frac * 0.92
        _cbx(trim, (relief * 1.0, hw * 2.0, 0.018),
             (x_cap + push * (relief * 0.70), 0.0, z), GRILLE_SLAT,
             bevel=relief * 0.26)
    # splitter: a wide chamfered lip that reads as a separate aero part
    zc = z0 - 0.028
    hw = _half_w(ring, zc) * min(1.0, y_frac + 0.10)
    x_spl = x_cap + push * min(0.060, max(0.0, push * (x_cap - face) * 0.5))
    _cbx(trim, (0.130, hw * 2.0, 0.046), (x_spl, 0.0, zc), TRIM_DK,
         bevel=0.016)
    return trim


def _exhaust(trim, ring, x_cap, push, face):
    """Short chrome tailpipe tucked under the rear bumper, off to one side."""
    z = ring[4] + (ring[5] - ring[4]) * 0.26
    x_out = x_cap + push * max(0.0, push * (x_cap - face) * 0.5) - push * 0.02
    C.cylinder(trim.mesh, 0.036, 0.20, seg=8,
               center=(x_out, -0.30, z), color=CHROME, axis="X")
    return trim


def _fuel_cap(trim, cfg, x, z, color=None):
    """A small body-colour filler disc with a dark bezel, on the rear quarter.

    ``x`` and ``z`` are config-supplied because the only safe spot is the flat
    panel BETWEEN the rear arch and the tail: put it any further back and the
    body is already tapering, so the disc disappears into the shell.
    """
    col = color or cfg.get("fuel_color", cfg.get("paint", WHITE))
    for sy in (-1, 1):
        y = _flank_y(cfg, x, z)
        C.cylinder(trim.mesh, 0.082, 0.020, seg=10,
                   center=(x, sy * (y + 0.004), z), color=TRIM_DK, axis="Y")
        C.cylinder(trim.mesh, 0.066, 0.026, seg=10,
                   center=(x, sy * (y + 0.012), z), color=col, axis="Y")
    return trim


def _plates(a, cfg, front_faces):
    """A number-plate recess on BOTH bumper faces: dark well, pale plate.

    ``front_faces`` carries the OUTERMOST bumper plane for each end, so the
    plate sits ON the bumper face: the pale plate's outer face is flush with
    it and the dark recess is cut back into the bumper rather than stacked
    forward of it.  A plate that protrudes would add its own thickness to the
    car's measured length, and ``src/collision.rs`` sizes the car from that.
    """
    plates = _part(a, "plates", base_color=PLATE, roughness=0.45)
    for _front, x_face, ring, push, zc in front_faces:
        # dark well, recessed 12 mm back from the bumper face
        _cap_pane(plates.mesh, ring, x_face, push, zc - 0.088, zc + 0.088,
                  0.300, -0.012, 0.045, TRIM_DK)
        # pale plate, face flush with the bumper
        _cap_pane(plates.mesh, ring, x_face, push, zc - 0.070, zc + 0.070,
                  0.255, 0.0, 0.036, PLATE)
    return plates


def _wipers(trim, cfg):
    """Two thin blades raked across the base of the windscreen."""
    ws = cfg.get("glass", {}).get("windshield")
    if not ws:
        return trim
    xa, za, ha, xb, zb, hb = ws
    dx, dz = xb - xa, zb - za
    ln = math.hypot(dx, dz)
    if ln < 1e-6:
        return trim
    ux, uz = dx / ln, dz / ln
    ramp = -math.degrees(math.atan2(uz, ux))
    for sy, skew in ((-1, 26.0), (1, 14.0)):
        t = 0.11
        px, pz = xa + dx * t, za + dz * t
        nx, nz = -uz, ux
        cy = sy * ha * 0.52
        for (ang, size, off, col) in (
                (skew, (0.46, 0.024, 0.018), 0.030, TRIM_DK),
                (skew * 0.30, (0.30, 0.016, 0.014), 0.016, GRILLE_SLAT)):
            tmp = C.Mesh()
            C.chamfer_box(tmp, size, center=(0, 0, 0), color=col, bevel=0.005)
            _merge_xform(trim, tmp,
                         center=(px + nx * off, cy, pz + nz * off),
                         rot=(("Z", ang), ("Y", ramp)))
    return trim


# ------------------------------------------------------------ the vehicle ---

def _vehicle(cfg):
    """Assemble one vehicle from its config dictionary."""
    a = C.Asset(cfg["id"], "vehicle")
    paint = cfg["paint"]
    trim_c = cfg.get("trim", CHROME)
    spokes = cfg.get("spokes", 5)

    # ---- body shell + greenhouse (one material, one flat part) -------------
    body = a.part("body", base_color=paint, roughness=0.40)
    C.loft(body.mesh,
           _shell_rings(cfg["shell"], lambda x: _shell_z0(cfg, x),
                        _arch_xs(cfg), cfg.get("n_corner", 3)),
           paint, cap_start_flip=True, cap_end_flip=False)
    cab = cfg.get("cabin")
    if cab:
        C.loft(body.mesh, _shell_rings(cab["ctrl"], lambda x: cab["z0"], ()),
               paint, cap_start_flip=True, cap_end_flip=False)
    _zone_paint(body, cfg, paint)

    # ONE trim part for the whole vehicle: bumpers, grille, mirrors, handles,
    # pillars, arch lips.  A material-per-part renderer collapses duplicates
    # anyway, and the exporter rejects them outright, so extras hooks append
    # through ``_part`` rather than opening a second "trim".
    trim = a.part("trim", base_color=trim_c, roughness=0.36, metallic=0.55)

    # ---- glass ------------------------------------------------------------
    g = cfg["glass"]
    glass = a.part("glass", base_color=GLASS, roughness=0.12, metallic=0.30)
    for key in ("windshield", "backlight"):
        if g.get(key):
            _ramp_glass(glass.mesh, *g[key], g["ref"], g.get("tint", GLASS))
    for (x1, x2, z1, z2) in g.get("side", ()):
        _side_glass(glass.mesh, g["side_hw"], x1, x2, z1, z2,
                    g.get("tint", GLASS))
    row = g.get("window_row")
    if row:
        # the GLAZING goes in the glass part; the PILLARS between the panes
        # are body trim, and are what turns a black stripe into a window row
        _window_row(cfg, glass.mesh, *row[:5], g.get("tint", GLASS),
                    pillar=cfg.get("pillar_w", 0.075), part=trim)
    drv = g.get("side_driver")
    if drv:
        # a driver's window: kerb side only, ahead of the first door
        _flank_pane(cfg, glass.mesh, drv[0], drv[1], drv[2], drv[3],
                    g.get("tint", GLASS), lift=0.012, thick=0.06, nseg=1,
                    sides=(1,))
    # a bus (or any flat-fronted body) drops its screens into the end wall
    for fs in (g.get("front_screen"), g.get("rear_screen")):
        if fs:
            _face_screen(glass.mesh, cfg, True if fs is g.get("front_screen")
                         else False, fs[:2], fs[2], g.get("tint", GLASS))

    # ---- wheels -----------------------------------------------------------
    tyres = a.part("tyres", base_color=TYRE, roughness=0.88)
    hubs = a.part("hubs", base_color=HUB, roughness=0.32, metallic=0.65)
    for (x, track, ww, rim) in _wheels_cfg(cfg):
        for sy in (-1, 1):
            _wheel(cfg, tyres, hubs, x, track, ww, sy, spokes, rim)
    # ---- trim: bumpers, grille, intake, mirrors, cut-lines, handles -------
    for ax in cfg["axles"]:
        _arch_lip(cfg, trim, ax)

    # ---- lamps, laid onto the two end caps -------------------------------
    faces = {}
    for front, key, color, emis in (
            (True, "front", cfg.get("lamp_front", HEADLIGHT),
             cfg.get("emis_front", (0.85, 0.78, 0.55))),
            (False, "rear", cfg.get("lamp_rear", TAILLIGHT),
             cfg.get("emis_rear", (0.62, 0.06, 0.06)))):
        face = cfg["face"][key]
        ring, x_cap = _cap_ring(cfg, front)
        push = -1.0 if front else 1.0
        size, center = _cap_band(ring, x_cap, push, *face["lamp"])
        part = a.part(key + "lights", base_color=color, roughness=0.20,
                      emissive=emis)
        _pair(part, size, center, color)
        faces[front] = (ring, x_cap, push, center[2])

    plate_faces = []
    for front, key in ((True, "front"), (False, "rear")):
        ring, x_cap = _cap_ring(cfg, front)
        push = -1.0 if front else 1.0
        height, depth, bite = cfg["face"][key].get("bumper",
                                                  (0.19, 0.16, 0.055))
        size, center, x_face = _cap_bumper(ring, x_cap, push, height, depth,
                                            bite)
        _cbx(trim, size, center, cfg.get("bumper_color", trim_c), bevel=0.022)
        plate_faces.append((front, x_face, ring, push, center[2]))

    ring, x_cap = _cap_ring(cfg, True)
    _grille(trim, ring, x_cap, -1.0, cfg["face"]["front"]["grille"],
            cfg.get("grille_slats", 4), cfg.get("grille_w", 0.96))
    f_face = plate_faces[0][1]
    if cfg.get("intake", True):
        g0, g1 = cfg["face"]["front"]["grille"][:2]
        _intake(trim, ring, x_cap, -1.0, (g0 * 0.22, max(0.04, g0 - 0.055)),
                cfg.get("intake_slats", 2), cfg.get("intake_w", 0.90), f_face)
    if cfg.get("exhaust", True):
        rring, rcap = _cap_ring(cfg, False)
        _exhaust(trim, rring, rcap, 1.0, plate_faces[-1][1])
    _plates(a, cfg, plate_faces)
    if cfg.get("wipers", True):
        _wipers(trim, cfg)

    mx, mz, msz, m_arm = cfg["mirror"]
    _flank_pair(cfg, trim, mx, mz, (msz[0], 0.0, msz[2]), trim_c,
                out=0.0, thick=0.030, chamfer=True)
    _flank_pair(cfg, trim, mx, mz, msz, cfg.get("mirror_color", paint),
                out=m_arm, chamfer=True)

    for x in cfg.get("doors", ()):
        z0, z1 = cfg.get("door_span", (0.30, 0.98))
        _flank_line(cfg, trim, x, z0, z1, TRIM_DK)
    for (x, z, dx) in cfg.get("handles", ()):
        # a dark recessed base with a body-colour pull bar standing off it
        _flank_pair(cfg, trim, x, z, (dx + 0.020, 0.0, 0.070), TRIM_DK,
                    out=0.004, thick=0.020, chamfer=True)
        _flank_pair(cfg, trim, x, z, (dx, 0.0, 0.028),
                    cfg.get("handle_color", paint), out=0.026, thick=0.038,
                    chamfer=True)
    if cfg.get("beltline"):
        x0, x1, z = cfg["beltline"]
        _flank_strip(cfg, trim, x0, x1, z, cfg.get("belt_color", trim_c))

    # fender louvres behind the leading edge of the front arch
    vent = cfg.get("vents")
    if vent:
        vx, vz, vn = vent
        for k in range(vn):
            _flank_pair(cfg, trim, vx, vz + k * 0.058, (0.150, 0.0, 0.030),
                        TRIM_DK, out=0.004, thick=0.024, chamfer=True)
    if cfg.get("fuel_cap"):
        _fuel_cap(trim, cfg, *cfg["fuel_cap"])

    # ---- bonnet / boot shut lines ----------------------------------------
    panel = cfg.get("panels")
    if panel:
        cuts = a.part("shutlines", base_color=C.shade(paint, 0.30),
                      roughness=0.60, outward=("dir", (0.0, 0.0, 1.0)))
        for (x0, x1, y_frac) in panel:
            xm = (x0 + x1) * 0.5
            hw = min(_half_w(_body_ring(cfg, xm), _body_ring(cfg, xm)[5] - 0.02),
                     _field(cfg["shell"], xm, 1))
            for sy in (-1, 1):
                _top_line_x(cfg, cuts, x0, x1, sy * hw * y_frac, 0.018,
                            C.shade(paint, 0.30))
            _top_line_y(cfg, cuts, x1 - 0.022, -hw * y_frac, hw * y_frac,
                        0.018, C.shade(paint, 0.30))

    # ---- per-vehicle extras: livery, cargo bed, roof mounts --------------
    for hook in cfg.get("extras", ()):
        hook(a, cfg)
    return a


# ----------------------------------------------------- silhouette tables ----

_SEDAN_SHELL = [
    (-2.178, 0.610, 0.760, 0.220),
    (-2.042, 0.814, 0.815, 0.200),
    (-1.886, 0.881, 0.860, 0.160),
    (-1.614, 0.901, 0.895, 0.150),
    (-1.342, 0.903, 0.920, 0.140),
    (-1.050, 0.903, 0.945, 0.140),
    (-0.895, 0.891, 1.000, 0.160),
    (-0.535, 0.887, 1.000, 0.160),
    (0.097, 0.887, 1.000, 0.160),
    (0.681, 0.891, 1.000, 0.160),
    (0.914, 0.901, 0.960, 0.140),
    (1.128, 0.903, 0.940, 0.140),
    (1.303, 0.903, 0.925, 0.140),
    (1.536, 0.901, 0.910, 0.140),
    (1.789, 0.887, 0.895, 0.160),
    (2.022, 0.834, 0.875, 0.190),
    (2.178, 0.610, 0.800, 0.220),
]

_SEDAN_CABIN = [
    (-1.02, 0.800, 1.040, 0.045),
    (-0.76, 0.790, 1.400, 0.075),
    (-0.60, 0.784, 1.450, 0.090),
    (0.38, 0.784, 1.450, 0.090),
    (0.54, 0.790, 1.420, 0.075),
    (0.92, 0.800, 1.040, 0.045),
]

_SEDAN_COMMON = {
    "shell": _SEDAN_SHELL,
    "cabin": {"ctrl": _SEDAN_CABIN, "z0": 0.70},
    "sill": 0.26, "ends": ((-2.178, 0.44), (2.178, 0.48)), "end_span": 0.20,
    "arch_top": 0.76, "arch_half": 0.42, "axles": (-1.330, 1.320),
    "wheel": (0.34, 0.22, 0.771),
    "glass": {
        "windshield": (-1.00, 1.040, 0.700, -0.750, 1.400, 0.665),
        "backlight": (0.90, 1.040, 0.700, 0.525, 1.420, 0.670),
        "side": ((-0.55, -0.04, 1.070, 1.325),
                 (0.02, 0.41, 1.070, 1.325)),
        "side_hw": 0.777, "ref": (-0.10, 0.90),
    },
    "face": {
        "front": {"lamp": (0.52, 0.92, 0.74, 0.075),
                  "grille": (0.30, 0.60, 0.52, 0.06),
                  "bumper": (0.19, 0.16, 0.055)},
        "rear": {"lamp": (0.55, 0.92, 0.80, 0.075),
                 "bumper": (0.19, 0.16, 0.055)},
    },
    "mirror": (-0.78, 1.010, (0.150, 0.085, 0.100), 0.060),
    "doors": (-0.86, 0.06, 0.88),
    "door_span": (0.44, 0.80),
    "handles": ((-0.49, 0.855, 0.17), (0.34, 0.855, 0.17)),
    "beltline": (-0.90, 0.92, 1.045),
    "vents": (-0.98, 0.545, 3),
    "fuel_cap": (1.93, 0.775),
    "panels": ((-1.93, -1.12, 0.60), (1.00, 2.04, 0.60)),
}


# ---------------------------------------------------------------------------
# Distinct bodies for the police cruiser and the taxi.
#
# Previously both were `_SEDAN_COMMON` verbatim: identical 4.57 x 1.85 shell,
# byte-identical side profile across all 14 sampled stations (measured mean
# |delta| = 0.0000). On screen they were the same car in different paint --
# a light bar and a roof sign do not change the read at street distance.
#
# A police cruiser wants a squared-off, upright, long-roofline body; a city
# taxi wants a tall short-bonnet one-box profile. Both are built from the
# same (z, sill_y, roof_y, half_width) station format as _SEDAN_SHELL.
# ---------------------------------------------------------------------------

# Upright three-box cruiser: flat hood, near-vertical windscreen, long flat
# roof carried well past the B-pillar, abrupt drop at the tail.
# The roof line is carried FLAT at 1.075 from the A-pillar all the way to
# x=+1.30, then drops almost vertically to the short deck. That hard break is
# what makes it read "box" rather than "long sedan": a render measured against
# the sedan showed the cruiser's mid-roof sitting *lower* than the sedan's,
# because a longer body with the same roof height just reads longer-and-lower.
_POLICE_SHELL = [
    (-2.240, 0.660, 0.760, 0.235),
    (-2.120, 0.940, 0.830, 0.220),
    (-1.980, 1.000, 0.905, 0.195),
    (-1.760, 1.030, 0.960, 0.185),
    (-1.520, 1.045, 1.005, 0.180),
    (-1.240, 1.055, 1.030, 0.178),
    (-0.980, 1.058, 1.075, 0.185),
    (-0.560, 1.058, 1.075, 0.190),
    (0.320, 1.058, 1.075, 0.190),
    (0.940, 1.058, 1.075, 0.185),
    (1.180, 1.055, 1.070, 0.180),
    (1.330, 1.050, 1.030, 0.178),
    (1.620, 1.045, 0.995, 0.180),
    (1.940, 1.030, 0.955, 0.190),
    (2.240, 0.860, 0.880, 0.235),
]

# Flat windscreen, tall roof, boxy tail -- the one-box city cab.
_TAXI_SHELL = [
    (-2.010, 0.660, 0.780, 0.235),
    (-1.900, 0.900, 0.830, 0.220),
    (-1.760, 0.960, 0.900, 0.195),
    (-1.520, 0.985, 0.950, 0.185),
    (-1.180, 0.990, 0.985, 0.180),
    (-0.880, 0.992, 1.060, 0.190),
    (-0.420, 0.994, 1.060, 0.195),
    (0.240, 0.994, 1.060, 0.195),
    (0.760, 0.992, 1.060, 0.190),
    (1.020, 0.990, 1.010, 0.180),
    (1.400, 0.988, 0.985, 0.180),
    (1.720, 0.985, 0.960, 0.185),
    (1.950, 0.975, 0.925, 0.200),
]

# Cruiser cabin: upright glasshouse, roof stays high all the way to the tail.
_POLICE_CABIN = [
    (-1.02, 0.980, 1.170, 0.050),
    (-0.80, 0.972, 1.480, 0.088),
    (-0.64, 0.966, 1.535, 0.105),
    (0.86, 0.966, 1.535, 0.105),
    (1.04, 0.972, 1.480, 0.088),
    (1.20, 0.980, 1.170, 0.050),
]

# Cab cabin: tall, upright, short bonnet -- one continuous glasshouse.
_TAXI_CABIN = [
    (-0.92, 0.920, 1.130, 0.050),
    (-0.70, 0.912, 1.470, 0.085),
    (-0.54, 0.906, 1.520, 0.100),
    (0.60, 0.906, 1.520, 0.100),
    (0.80, 0.912, 1.470, 0.085),
    (1.02, 0.920, 1.130, 0.050),
]

_POLICE_COMMON = {
    "shell": _POLICE_SHELL,
    "cabin": {"ctrl": _POLICE_CABIN, "z0": 0.84},
    "sill": 0.28, "ends": ((-2.240, 0.46), (2.240, 0.50)), "end_span": 0.21,
    "arch_top": 0.78, "arch_half": 0.43, "axles": (-1.360, 1.390),
    "wheel": (0.35, 0.23, 0.783),
    "glass": {
        "windshield": (-1.02, 1.170, 0.780, -0.80, 1.480, 0.740),
        "backlight": (0.86, 1.170, 0.780, 0.60, 1.490, 0.740),
        "side": ((-0.58, -0.06, 1.190, 1.420),
                 (0.02, 0.48, 1.190, 1.420)),
        "side_hw": 0.880, "ref": (-0.12, 1.02),
    },
    "face": {
        "front": {"lamp": (0.54, 0.94, 0.76, 0.078),
                  "grille": (0.32, 0.62, 0.54, 0.06),
                  "bumper": (0.20, 0.17, 0.055)},
        "rear": {"lamp": (0.57, 0.94, 0.82, 0.078),
                 "bumper": (0.20, 0.17, 0.055)},
    },
    "mirror": (-0.80, 1.130, (0.155, 0.090, 0.105), 0.062),
    "doors": (-0.92, 0.04, 0.94),
    "door_span": (0.46, 0.82),
    "handles": ((-0.52, 0.880, 0.17), (0.38, 0.880, 0.17)),
    "beltline": (-0.92, 1.02, 1.165),
    "vents": (-1.02, 0.580, 3),
    "fuel_cap": (1.99, 0.815),
    "panels": ((-1.97, -1.16, 0.62), (1.04, 2.10, 0.62)),
}

_TAXI_COMMON = {
    "shell": _TAXI_SHELL,
    "cabin": {"ctrl": _TAXI_CABIN, "z0": 0.86},
    "sill": 0.27, "ends": ((-2.010, 0.45), (1.950, 0.49)), "end_span": 0.20,
    "arch_top": 0.77, "arch_half": 0.42, "axles": (-1.260, 1.230),
    "wheel": (0.34, 0.22, 0.781),
    "glass": {
        "windshield": (-0.92, 1.130, 0.760, -0.70, 1.470, 0.720),
        "backlight": (0.80, 1.130, 0.760, 0.56, 1.470, 0.720),
        "side": ((-0.54, -0.04, 1.150, 1.390),
                 (0.00, 0.44, 1.150, 1.390)),
        "side_hw": 0.848, "ref": (-0.10, 1.00),
    },
    "face": {
        "front": {"lamp": (0.50, 0.93, 0.75, 0.072),
                  "grille": (0.31, 0.61, 0.53, 0.06),
                  "bumper": (0.19, 0.16, 0.055)},
        "rear": {"lamp": (0.56, 0.93, 0.81, 0.078),
                 "bumper": (0.19, 0.16, 0.055)},
    },
    "mirror": (-0.76, 1.090, (0.150, 0.086, 0.100), 0.060),
    "doors": (-0.80, 0.06, 0.86),
    "door_span": (0.45, 0.81),
    "handles": ((-0.46, 0.890, 0.17), (0.34, 0.890, 0.17)),
    "beltline": (-0.86, 0.97, 1.125),
    "vents": (-0.94, 0.600, 3),
    "fuel_cap": (1.79, 0.855),
    "panels": ((-1.78, -1.02, 0.62), (0.94, 1.82, 0.62)),
}


# ------------------------------------------------------------------ liveries

def _police_livery(a, cfg):
    """Black-and-white cruiser: rocker band and door shields."""
    p = a.part("livery", base_color=POLICE_BLACK, roughness=0.38)
    for sy in (-1, 1):
        n, dx = 9, 2.70 / 9.0
        for i in range(n):
            x = -1.35 + i * dx
            z = 0.380
            y = _flank_y(cfg, x, z) + 0.022
            _bx(p, (dx + 0.004, 0.022, 0.150), (x, sy * y, z), POLICE_BLACK)
        for cx in (-0.44, 0.42):
            for i in range(3):
                z = 0.520 + i * 0.140
                y = _flank_y(cfg, cx, z) + 0.024
                _bx(p, (0.820, 0.024, 0.140), (cx, sy * y, z), POLICE_BLACK)
    return


def _taxi_livery(a, cfg):
    """Cab-yellow checker band along the flanks plus dark door plates."""
    p = a.part("livery", base_color=POLICE_BLACK, roughness=0.45)
    x, i = -0.85, 0
    while x < 0.85 - 1e-9:
        if i % 2 == 0:
            z = 0.845
            y = _flank_y(cfg, x, z) + 0.024
            for sy in (-1, 1):
                _bx(p, (0.185, 0.024, 0.135), (x, sy * y, z), POLICE_BLACK)
        x += 0.185
        i += 1
    for cx in (-0.44, 0.42):
        z = 0.620
        y = _flank_y(cfg, cx, z) + 0.025
        for sy in (-1, 1):
            _bx(p, (0.700, 0.026, 0.300), (cx, sy * y, z), TRIM_DK)
    return


def _lightbar_mount(a, cfg):
    """Low plinth the light-bar lenses bolt onto (kept under 1.60 m overall)."""
    x = -0.06
    z0 = _field(cfg["cabin"]["ctrl"], x, 2)
    m = a.part("lightbar_mount", base_color=POLICE_BLACK, roughness=0.55)
    _cbx(m, (0.660, 1.180, 0.032), (x, 0.0, z0 + 0.016), POLICE_BLACK,
         bevel=0.010)
    for sy in (-1, 1):
        _cbx(m, (0.480, 0.050, 0.070), (x, sy * 0.520, z0 - 0.012), TRIM_DK,
             bevel=0.010)
    return


def _lightbar(a, cfg):
    """A REAL light bar: red half and blue half, both strongly emissive.

    The engine only reads the per-part emissive, so two lens parts are all it
    takes for the existing night glow pass to wash the roof in red and blue.
    Overall height stays 1.582 m -- the sedan roof is 1.45 m and a bar that
    climbed higher would read as a van.
    """
    x = -0.06
    z0 = _field(cfg["cabin"]["ctrl"], x, 2) + 0.032
    h = 0.100
    for name, col, emis, cy in (
            ("lightbar_red", POLICE_RED, (0.98, 0.10, 0.10), -0.305),
            ("lightbar_blue", POLICE_BLUE, (0.14, 0.32, 0.98), 0.305)):
        p = a.part(name, base_color=col, roughness=0.18, emissive=emis)
        _cbx(p, (0.560, 0.560, h), (x, cy, z0 + h * 0.5), col, bevel=0.022)
    # dark divider + end caps so the two halves read as separate modules
    bar = _part(a, "lightbar_mount")
    _bx(bar, (0.580, 0.030, h + 0.014), (x, 0.0, z0 + h * 0.5), TRIM_DK)
    for sy in (-1, 1):
        _bx(bar, (0.580, 0.034, h + 0.014), (x, sy * 0.590, z0 + h * 0.5),
            TRIM_DK)
    return


def _sign_mount(a, cfg):
    """Small bracket on the roof centre -- the taxi sign bolts onto this."""
    x = -0.30
    z0 = _field(cfg["cabin"]["ctrl"], x, 2)
    m = a.part("sign_mount", base_color=TRIM_DK, roughness=0.55)
    _cbx(m, (0.320, 0.260, 0.028), (x, 0.0, z0 + 0.014), TRIM_DK, bevel=0.008)
    return


def _taxi_sign(a, cfg):
    """The roof sign itself: an amber emissive box on the bracket.

    Reads as a lit taxi roof light at night; overall roof height 1.598 m.
    """
    x = -0.30
    z0 = _field(cfg["cabin"]["ctrl"], x, 2) + 0.028
    h = 0.120
    s = a.part("taxi_sign", base_color=AMBER, roughness=0.22,
               emissive=(0.95, 0.55, 0.06))
    _cbx(s, (0.300, 0.620, h), (x, 0.0, z0 + h * 0.5), AMBER, bevel=0.020)
    # dark capping strips top and bottom read as the sign's frame
    m = _part(a, "sign_mount")
    for zz in (z0 + 0.006, z0 + h - 0.006):
        _cbx(m, (0.320, 0.640, 0.014), (x, 0.0, zz), TRIM_DK, bevel=0.005)
    return


def _bed(a, cfg):
    """Pickup cargo box: side rails, head-board, tailgate and a bed floor."""
    col = cfg.get("bed_color", (0.30, 0.34, 0.36))
    b = a.part("bed", base_color=col, roughness=0.45)
    ring = _body_ring(cfg, 1.50)
    hw = min(_half_w(ring, ring[5] - 0.02), _field(cfg["shell"], 1.50, 1))
    z0, z1 = ring[5] + 0.010, ring[5] + 0.395
    zm, h = (z0 + z1) * 0.5, z1 - z0
    for sy in (-1, 1):
        _cbx(b, (1.760, 0.090, h), (1.540, sy * (hw - 0.045), zm), col,
             bevel=0.018)
    _cbx(b, (0.090, hw * 2.0 - 0.090, h), (0.725, 0.0, zm), col, bevel=0.018)
    _cbx(b, (0.100, hw * 2.0 - 0.090, h), (2.415, 0.0, zm), col, bevel=0.018)
    for sy in (-1, 1):
        _bx(b, (1.700, hw - 0.500, 0.050), (1.545, sy * (hw - 0.320),
                                           z0 + 0.025), col)
    return


# --------------------------------------------------------------- the fleet --

_COUPED_SHELL = [
    (-2.115, 0.575, 0.740, 0.180),
    (-1.979, 0.793, 0.780, 0.160),
    (-1.805, 0.860, 0.810, 0.120),
    (-1.591, 0.897, 0.840, 0.110),
    (-1.455, 0.898, 0.855, 0.110),
    (-1.203, 0.898, 0.875, 0.110),
    (-1.048, 0.884, 0.920, 0.130),
    (-0.737, 0.878, 0.930, 0.130),
    (-0.097, 0.878, 0.930, 0.130),
    (0.485, 0.880, 0.930, 0.130),
    (1.018, 0.890, 0.920, 0.130),
    (1.242, 0.898, 0.900, 0.120),
    (1.533, 0.898, 0.875, 0.110),
    (1.785, 0.884, 0.845, 0.130),
    (1.998, 0.813, 0.815, 0.160),
    (2.134, 0.595, 0.770, 0.180),
]

_COUPED_CABIN = [
    (-0.92, 0.790, 1.000, 0.050),
    (-0.68, 0.778, 1.260, 0.070),
    (-0.52, 0.772, 1.300, 0.080),
    (0.28, 0.772, 1.300, 0.080),
    (0.56, 0.780, 1.240, 0.070),
    (1.00, 0.792, 0.950, 0.050),
]

_PICKUP_SHELL = [
    (-2.594, 0.644, 0.900, 0.200),
    (-2.457, 0.858, 0.960, 0.180),
    (-2.282, 0.927, 1.000, 0.140),
    (-2.048, 0.948, 1.020, 0.120),
    (-1.853, 0.952, 1.045, 0.120),
    (-1.716, 0.952, 1.060, 0.120),
    (-1.541, 0.952, 1.080, 0.120),
    (-1.326, 0.952, 1.100, 0.120),
    (-1.151, 0.940, 1.140, 0.140),
    (-0.839, 0.929, 1.160, 0.140),
    (-0.332, 0.929, 1.160, 0.140),
    (0.293, 0.932, 1.160, 0.140),
    (0.780, 0.942, 1.160, 0.130),
    (1.073, 0.948, 1.160, 0.120),
    (1.404, 0.948, 1.150, 0.120),
    (1.755, 0.946, 1.140, 0.120),
    (2.048, 0.932, 1.130, 0.130),
    (2.340, 0.907, 1.120, 0.150),
    (2.555, 0.722, 1.060, 0.200),
]

_PICKUP_CABIN = [
    (-1.00, 0.905, 1.220, 0.060),
    (-0.78, 0.888, 1.720, 0.090),
    (-0.62, 0.880, 1.780, 0.100),
    (0.26, 0.880, 1.780, 0.100),
    (0.46, 0.890, 1.720, 0.090),
    (0.62, 0.905, 1.300, 0.060),
]


def _bus():
    """11.6 m Miami-style transit bus.

    A completely separate silhouette table in the same style: one long slab
    with a big corner radius doing the chamfered roof edge, a flat 2.04 m wide
    front and rear face for the destination panels, and three axles whose
    arches merge into a single 2.6 m rear wheelhouse.
    """
    shell = [
        (-5.80, 1.030, 2.98, 0.18),
        (-5.58, 1.155, 3.00, 0.20),
        (-5.28, 1.218, 3.00, 0.22),
        (-4.80, 1.246, 3.00, 0.24),
        (-4.20, 1.250, 3.00, 0.24),
        (-2.00, 1.250, 3.00, 0.24),
        (0.00, 1.250, 3.00, 0.24),
        (2.00, 1.250, 3.00, 0.24),
        (3.90, 1.250, 3.00, 0.24),
        (4.80, 1.246, 3.00, 0.24),
        (5.28, 1.218, 3.00, 0.22),
        (5.58, 1.155, 3.00, 0.20),
        (5.80, 1.030, 2.98, 0.18),
    ]
    cfg = {
        "id": "bus_city",
        "paint": BUS_CREAM,
        "n_corner": 3,
        "shell": shell,
        "sill": 0.44, "ends": ((-5.80, 0.70), (5.80, 0.72)), "end_span": 0.36,
        "arch_top": 1.06, "arch_half": 0.64, "arch_segs": 7,
        "axles": (-4.10, 2.95, 4.25),
        "wheel": (0.50, 0.28, 1.000),
        # steer axle then a twin rear bogie: the inner rear wheel of each pair
        # is hidden behind its partner, so it carries no rim and costs 24 tris.
        # Tracks keep the outer face of the duals at 1.24 m, inside the body.
        "wheels": ((-4.10, 0.985, 0.28, True),
                   (2.95, 0.880, 0.36, False),
                   (2.95, 1.060, 0.36, True),
                   (4.25, 0.880, 0.36, False),
                   (4.25, 1.060, 0.36, True)),
        "spokes": 5,
        "glass": {
            "tint": BUS_GLASS,
            # Six bays from the driver's window back.  The two front door
            # pockets get their own glazing from _bus_doors, so the row starts
            # behind them and the flank reads as a continuous bus rather than
            # a pane interrupted by a door.
            "window_row": (-1.60, 4.60, 1.86, 2.66, 5),
            # a driver's window ahead of the first door, and a small
            # rear-window slit: a bus has neither a raked pane nor a
            # greenhouse, so every screen here is an end- or wall-panel.
            "side_driver": (-5.15, -4.72, 1.86, 2.66),
            "front_screen": (0.545, 0.815, 0.90),
            "rear_screen": (0.680, 0.800, 0.80),
        },
        "pillar_color": (0.26, 0.28, 0.31),
        "pillar_w": 0.085,
        "face": {
            "front": {"lamp": (0.10, 0.26, 0.46, 0.06),
                      "grille": (0.02, 0.06, 0.50, 0.05)},
            "rear": {"lamp": (0.14, 0.34, 0.52, 0.06),
                     "bumper": (0.30, 0.20, 0.090)},
        },
        "mirror": (-4.55, 2.62, (0.110, 0.060, 0.290), 0.040),
        "mirror_color": TRIM_DK,
        "grille_slats": 3, "grille_w": 0.62,
        "intake": False, "exhaust": False, "wipers": False, "fuel_cap": None,
        "handles": (), "doors": (), "beltline": None, "vents": None,
        "panels": None,
        "stripe": (1.30, 1.62, MIAMI_TEAL),
        "extras": (_bus_livery, _bus_doors, _bus_hvac),
    }
    return _vehicle(cfg)


def _in_arch(cfg, x, z):
    """True when (x, z) is inside a wheel opening, so trim must skip it.

    A moulding band that runs straight across the flank passes THROUGH the
    wheel arch: the arch is a hole in the side, not a dent in it, so anything
    spanning it has to be cut back.  Sampling ``_arch_z`` per segment is what
    lets the band stop at the arch and pick up on the other side.
    """
    for ax in cfg["axles"]:
        if abs(x - ax) < cfg["arch_half"]:
            if z < _arch_z(x, cfg) + 0.012:
                return True
    return False


def _bus_livery(a, cfg):
    """Side band, skirts, destination panels, marker lights and door recesses."""
    p = a.part("livery", base_color=MIAMI_PINK, roughness=0.42)
    # wide lower skirt in a dark tone, plus bright belts above and below the
    # glazing.  Every segment is clipped out of the wheel openings, or the band
    # would pass straight through the arches.
    for sy in (-1, 1):
        for (z, h, col) in ((0.74, 0.44, (0.28, 0.30, 0.33)),
                             (1.30, 0.10, MIAMI_PINK),
                             (2.72, 0.12, MIAMI_PINK)):
            run = None
            for i in range(19):
                x = -5.10 + i * (9.60 / 18.0)
                if _in_arch(cfg, x, z):
                    if run is not None:
                        _bx(p, (x - run[0] + 0.02, 0.020, h),
                            ((run[0] + x) * 0.5, sy * (_flank_y(cfg, x, z)
                                                        + 0.010), z), col)
                        run = None
                else:
                    y = _flank_y(cfg, x, z) + 0.010
                    if run is None:
                        run = (x, y)
            if run is not None:
                _bx(p, (9.60 + 0.02, 0.020, h),
                    ((run[0] + 5.10) * 0.5, sy * run[1], z), col)
    # destination / livery panels on the two flat end faces
    fring, fcap = _cap_ring(cfg, True)
    rring, rcap = _cap_ring(cfg, False)
    for (ring, x_cap, push, z0, z1, yf, col) in (
            (fring, fcap, -1.0, 2.90, 3.02, 0.86, TRIM_DK),
            (rring, rcap, 1.0, 2.84, 2.98, 0.78, TRIM_DK)):
        _cap_pane(p.mesh, ring, x_cap, push, z0, z1, yf, 0.014, 0.05, col)
    # amber side markers along the skirt line
    m = a.part("markers", base_color=AMBER, roughness=0.24,
               emissive=(0.85, 0.42, 0.05))
    for sy in (-1, 1):
        for i in range(5):
            x = -4.20 + i * 2.10
            if _in_arch(cfg, x, 0.95):
                continue
            y = _flank_y(cfg, x, 0.95) + 0.016
            _bx(m, (0.130, 0.024, 0.070), (x, sy * y, 0.95), AMBER)
    return


def _bus_doors(a, cfg):
    """Two kerb-side doors: a recessed reveal, twin leaves, stiles, step rail.

    The door aperture is a hole in the flank, so each leaf gets a SOLID lower
    panel and a glazed upper half.  Frames alone -- jambs and a head rail with
    nothing between them -- read as a skeleton hanging on the body, which is
    worse than no door at all.
    """
    d = a.part("doors", base_color=TRIM_DK, roughness=0.30, metallic=0.45)
    # The door glass goes into the EXISTING "glass" part: the exporter rejects
    # duplicate part names, and one material for every window on the bus is
    # what a renderer wants anyway.
    g = _part(a, "glass", base_color=BUS_GLASS, roughness=0.12, metallic=0.30)
    body_c = cfg.get("pillar_color", (0.30, 0.32, 0.35))
    z_lo, z_mid, z_hi = 1.42, 1.90, 2.50
    for (x, w) in ((-4.05, 1.15), (-2.30, 1.15)):
        y = _flank_y(cfg, x, (z_lo + z_hi) * 0.5)
        for sx in (-1, 1):          # jambs
            _cbx(d, (0.045, 0.030, z_hi - z_lo), (x + sx * w * 0.5, y + 0.016,
                                                  (z_lo + z_hi) * 0.5),
                 TRIM_DK, bevel=0.008)
        _cbx(d, (w, 0.030, 0.045), (x, y + 0.016, z_hi), TRIM_DK, bevel=0.008)
        _cbx(d, (w, 0.030, 0.045), (x, y + 0.016, z_lo), TRIM_DK, bevel=0.008)
        _cbx(d, (0.034, 0.034, z_hi - z_lo), (x, y + 0.022, (z_lo + z_hi) * 0.5),
             CHROME, bevel=0.008)
        for sx in (-1, 1):          # each leaf: solid lower + glazed upper
            cx = x + sx * w * 0.25
            lw = w * 0.47
            _cbx(d, (lw, 0.026, z_mid - z_lo), (cx, y + 0.014,
                                               (z_lo + z_mid) * 0.5),
                 body_c, bevel=0.008)
            quad = [(cx - lw * 0.44, y + 0.020, z_mid + 0.06),
                    (cx + lw * 0.44, y + 0.020, z_mid + 0.06),
                    (cx + lw * 0.44, y + 0.020, z_hi - 0.06),
                    (cx - lw * 0.44, y + 0.020, z_hi - 0.06)]
            _slab(g.mesh, quad, (0.0, -0.055, 0.0), BUS_GLASS)
        # step / kick rail below the door
        _cbx(d, (w + 0.06, 0.070, 0.070), (x, y + 0.030, z_lo - 0.24), CHROME,
             bevel=0.010)
    return


def _bus_hvac(a, cfg):
    """Roof HVAC pod with a louvred front and a dark vent grille on top."""
    h = a.part("hvac", base_color=(0.82, 0.82, 0.80), roughness=0.50)
    z0 = _roof_z(cfg)
    _cbx(h, (2.30, 1.76, 0.200), (1.20, 0.0, z0 + 0.100), (0.82, 0.82, 0.80),
         bevel=0.030)
    for i in range(3):
        _cbx(h, (0.030, 1.50, 0.130), (0.10 + i * 0.10, 0.0, z0 + 0.100),
             TRIM_DK, bevel=0.008)
    for sy in (-1, 1):
        _cbx(h, (1.60, 1.10, 0.026), (1.30, 0.0, z0 + 0.212), TRIM_DK,
             bevel=0.006)
    _cbx(_part(a, "markers", base_color=AMBER, roughness=0.24,
               emissive=(0.85, 0.42, 0.05)),
         (0.120, 0.120, 0.060), (2.42, 0.0, z0 + 0.230), AMBER, bevel=0.010)
    return


# --------------------------------------------------------------- assembly ---

def build_all():
    """Return the ordered list of vehicle assets."""
    out = []

    # 1. four-door family sedan -- the default NEON BAY cab
    out.append(_vehicle(dict(_SEDAN_COMMON,
                             id="car_sedan", paint=MIAMI_PINK)))

    # 2. two-door sports coupe -- long bonnet, fastback tail
    out.append(_vehicle({
        "id": "car_coupe",
        "paint": MIAMI_TEAL,
        "shell": _COUPED_SHELL,
        "cabin": {"ctrl": _COUPED_CABIN, "z0": 0.66},
        "sill": 0.240, "ends": ((-2.115, 0.40), (2.134, 0.42)), "end_span": 0.18,
        "arch_top": 0.720, "arch_half": 0.40, "axles": (-1.450, 1.265),
        "wheel": (0.320, 0.235, 0.772),
        "glass": {
            "windshield": (-0.90, 1.000, 0.680, -0.670, 1.260, 0.650),
            "backlight": (0.98, 0.950, 0.680, 0.548, 1.240, 0.650),
            "side": ((-0.47, 0.23, 0.975, 1.205),),
            "side_hw": 0.765, "ref": (-0.10, 0.85),
        },
        "face": {
            "front": {"lamp": (0.52, 0.92, 0.74, 0.075),
                      "grille": (0.28, 0.58, 0.50, 0.06),
                      "bumper": (0.18, 0.15, 0.050)},
            "rear": {"lamp": (0.55, 0.92, 0.80, 0.075),
                     "bumper": (0.18, 0.15, 0.050)},
        },
        "mirror": (-0.84, 0.950, (0.140, 0.080, 0.090), 0.055),
        "doors": (-0.61, 0.59),
        "door_span": (0.40, 0.76),
        "handles": ((-0.27, 0.790, 0.15),),
        "beltline": (-0.78, 0.84, 0.935),
        "vents": (-1.08, 0.530, 3),
        "fuel_cap": (1.88, 0.700),
        "panels": ((-1.87, -1.04, 0.60), (1.03, 1.97, 0.60)),
    }))

    # 3. full-size pickup -- cab forward, open cargo bed aft
    out.append(_vehicle({
        "id": "truck_pickup",
        "paint": CORAL,
        "bed_color": COPPER_DK,
        "shell": _PICKUP_SHELL,
        "cabin": {"ctrl": _PICKUP_CABIN, "z0": 0.95},
        "sill": 0.340, "ends": ((-2.594, 0.560), (2.555, 0.580)),
        "end_span": 0.26,
        "arch_top": 0.860, "arch_half": 0.46, "axles": (-1.535, 1.400),
        "wheel": (0.380, 0.260, 0.778),
        "glass": {
            "windshield": (-0.99, 1.220, 0.770, -0.77, 1.720, 0.740),
            "backlight": (0.61, 1.300, 0.770, 0.455, 1.720, 0.740),
            "side": ((-0.54, 0.20, 1.200, 1.620),),
            "side_hw": 0.862, "ref": (-0.20, 1.30),
        },
        "face": {
            "front": {"lamp": (0.50, 0.90, 0.80, 0.075),
                      "grille": (0.26, 0.48, 0.62, 0.06),
                      "bumper": (0.20, 0.17, 0.060)},
            "rear": {"lamp": (0.34, 0.66, 0.62, 0.075),
                     "bumper": (0.19, 0.16, 0.055)},
        },
        "bumper_color": (0.66, 0.68, 0.70),
        "mirror": (-0.85, 1.220, (0.190, 0.080, 0.140), 0.070),
        "doors": (-0.60, 0.40),
        "door_span": (0.46, 0.92),
        "handles": ((-0.35, 0.948, 0.19), (0.25, 0.948, 0.19)),
        "beltline": (-0.77, 0.59, 1.185),
        "vents": (-1.14, 0.640, 3),
        "fuel_cap": (2.20, 0.830),
        "panels": ((-2.25, -1.10, 0.62), (1.17, 2.38, 0.62)),
        "extras": (_bed,),
    }))

    # 4. police cruiser -- upright three-box shell (NOT the sedan), light bar
    out.append(_vehicle(dict(_POLICE_COMMON,
                             id="car_police", paint=WHITE,
                             bumper_color=(0.20, 0.21, 0.24),
                             extras=(_police_livery, _lightbar_mount,
                                     _lightbar))))

    # 5. taxi -- tall one-box cab shell (NOT the sedan), checker band + sign
    out.append(_vehicle(dict(_TAXI_COMMON,
                             id="car_taxi", paint=TAXI_YELLOW,
                             trim=(0.45, 0.46, 0.48),
                             belt_color=(0.20, 0.21, 0.24),
                             bumper_color=(0.22, 0.23, 0.26),
                             extras=(_taxi_livery, _sign_mount, _taxi_sign))))

    # 6. 11.6 m city bus
    out.append(_bus())

    return out
