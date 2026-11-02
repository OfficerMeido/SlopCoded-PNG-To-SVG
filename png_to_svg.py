#!/usr/bin/env python3
"""
png_to_svg - Vectorize PNG files into SVG (color tracing via vtracer).

Usage:
  png_to_svg [--no-smooth] image.png [more.png ...]

Each SVG is written next to its PNG with the same name (image.png -> image.svg).
An existing SVG with that name is overwritten.

Edges are cleaned up after tracing: straight edges become exact straight lines,
corners become sharp, and curves become a few smooth Bezier segments instead of
vtracer's wobbly staircase-following paths. Use --no-smooth to keep the raw
vtracer output.

The tracing runs in a child process, so if vtracer's native code crashes
(segfault) on one code path, the next path is tried instead of dying.
"""

import math
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

SETTINGS = dict(
    colormode="color",
    hierarchical="stacked",
    mode="spline",
    filter_speckle=4,
    color_precision=6,
    layer_difference=16,
    corner_threshold=60,
    length_threshold=4.0,
    splice_threshold=45,
    path_precision=3,
)

# Different ways of feeding the image to vtracer; tried in order.
METHODS = ("file", "bytes", "pixels")


def trace(method: str, src: str, out: str) -> None:
    """Runs inside the child process."""
    import vtracer

    if method == "file":
        vtracer.convert_image_to_svg_py(src, out, **SETTINGS)
    elif method == "bytes":
        svg = vtracer.convert_raw_image_to_svg(
            Path(src).read_bytes(), img_format="png", **SETTINGS)
        Path(out).write_text(svg, encoding="utf-8")
    elif method == "pixels":
        from PIL import Image
        with Image.open(src) as im:
            im = im.convert("RGBA")
            svg = vtracer.convert_pixels_to_svg(
                list(im.getdata()), im.size, **SETTINGS)
        Path(out).write_text(svg, encoding="utf-8")
    else:
        raise ValueError(f"unknown method {method}")


def worker_command(method: str, src: Path, out: Path) -> list:
    if getattr(sys, "frozen", False):  # PyInstaller binary re-runs itself
        base = [sys.executable]
    else:
        base = [sys.executable, str(Path(__file__).resolve())]
    return base + ["--worker", method, str(src), str(out)]


# ---------------------------------------------------------------------------
# Edge cleanup: replaces vtracer's wobbly staircase-following curves with
# straight lines on straight edges, sharp corners, and few smooth Béziers on
# curves. Pure Python, no extra dependencies.
# ---------------------------------------------------------------------------

STEP = 1.0          # resampling distance (px)
CORNER_WINDOW = 10  # samples either side used to measure a turn
CORNER_ANGLE = 0.55  # radians (~31 degrees) turn that counts as a corner
MIN_SIZE = 24.0     # shapes smaller than this (px) are left untouched


def _sub(a, b): return (a[0] - b[0], a[1] - b[1])
def _add(a, b): return (a[0] + b[0], a[1] + b[1])
def _mul(a, s): return (a[0] * s, a[1] * s)
def _dot(a, b): return a[0] * b[0] + a[1] * b[1]
def _cross(a, b): return a[0] * b[1] - a[1] * b[0]
def _len(a): return math.hypot(a[0], a[1])


def _unit(a):
    n = _len(a)
    return (a[0] / n, a[1] / n) if n > 1e-12 else (0.0, 0.0)


def _fmt(v: float) -> str:
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def _parse_loop(d: str):
    """Parse one 'M ... Z' subpath (M, L, C, Z only) into a dense closed polyline."""
    tokens = re.findall(r"[A-Za-z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?", d)
    pts, i, cmd, pos = [], 0, None, None

    def nums(k):
        nonlocal i
        if i + k > len(tokens):
            raise ValueError("truncated path")
        vals = [float(x) for x in tokens[i:i + k]]
        i += k
        return vals

    while i < len(tokens):
        t = tokens[i]
        if t.isalpha():
            if t not in "MLCZ":
                raise ValueError(f"unsupported command {t}")
            cmd = t
            i += 1
            if cmd == "Z":
                continue
        elif cmd is None or cmd == "Z":
            raise ValueError("bad path")
        if cmd == "M":
            if pts:
                raise ValueError("multiple subpaths")
            pos = tuple(nums(2))
            pts.append(pos)
            cmd = "L"
        elif cmd == "L":
            pos = tuple(nums(2))
            pts.append(pos)
        elif cmd == "C":
            x1, y1, x2, y2, x, y = nums(6)
            p0 = pos
            ctrl = (math.dist(p0, (x1, y1)) + math.dist((x1, y1), (x2, y2))
                    + math.dist((x2, y2), (x, y)))
            steps = min(400, max(8, int(ctrl / 2)))
            for s in range(1, steps + 1):
                u = s / steps
                m = 1 - u
                a, b, c, e = m * m * m, 3 * m * m * u, 3 * m * u * u, u * u * u
                pts.append((a * p0[0] + b * x1 + c * x2 + e * x,
                            a * p0[1] + b * y1 + c * y2 + e * y))
            pos = (x, y)
    if len(pts) > 1 and math.dist(pts[0], pts[-1]) < 1e-9:
        pts.pop()
    return pts


def _resample(pts, step):
    n = len(pts)
    seg = [math.dist(pts[k], pts[(k + 1) % n]) for k in range(n)]
    total = sum(seg)
    m = max(8, int(round(total / step)))
    sp = total / m
    out, k, acc = [], 0, 0.0
    for j in range(m):
        target = j * sp
        while acc + seg[k] < target and k < n - 1:
            acc += seg[k]
            k += 1
        f = (target - acc) / seg[k] if seg[k] > 0 else 0.0
        p, q = pts[k], pts[(k + 1) % n]
        out.append((p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f))
    return out


def _blur(pts, r=3):
    n = len(pts)
    w = 2 * r + 1
    return [(sum(pts[(i + o) % n][0] for o in range(-r, r + 1)) / w,
             sum(pts[(i + o) % n][1] for o in range(-r, r + 1)) / w)
            for i in range(n)]


def _find_corners(sm):
    n = len(sm)
    k = min(CORNER_WINDOW, n // 4)
    ang = []
    for i in range(n):
        a = _sub(sm[i], sm[(i - k) % n])
        b = _sub(sm[(i + k) % n], sm[i])
        ang.append(abs(math.atan2(_cross(a, b), _dot(a, b))))
    flag = [x > CORNER_ANGLE for x in ang]
    if all(flag):
        return [max(range(n), key=lambda i: ang[i])]
    start = flag.index(False)
    corners, group = [], []
    for o in range(1, n + 1):
        i = (start + o) % n
        if flag[i]:
            group.append(i)
        elif group:
            corners.append(max(group, key=lambda g: ang[g]))
            group = []
    return sorted(corners)


def _smooth_open(r, h):
    """Moving average over an open run; the window shrinks toward the ends so
    the end points (corners) stay exactly where they are."""
    n = len(r)
    out = []
    for i in range(n):
        w = min(h, i, n - 1 - i)
        if w == 0:
            out.append(r[i])
            continue
        seg = r[i - w:i + w + 1]
        out.append((sum(p[0] for p in seg) / len(seg), sum(p[1] for p in seg) / len(seg)))
    return out


# --- Schneider's curve-fitting algorithm (Graphics Gems, 1990) -------------

def _bez(b, t):
    m = 1 - t
    a, c, d, e = m * m * m, 3 * m * m * t, 3 * m * t * t, t * t * t
    return (a * b[0][0] + c * b[1][0] + d * b[2][0] + e * b[3][0],
            a * b[0][1] + c * b[1][1] + d * b[2][1] + e * b[3][1])


def _bez1(b, t):
    m = 1 - t
    return tuple(3 * (m * m * (b[1][j] - b[0][j]) + 2 * m * t * (b[2][j] - b[1][j])
                      + t * t * (b[3][j] - b[2][j])) for j in (0, 1))


def _bez2(b, t):
    m = 1 - t
    return tuple(6 * (m * (b[2][j] - 2 * b[1][j] + b[0][j])
                      + t * (b[3][j] - 2 * b[2][j] + b[1][j])) for j in (0, 1))


def _chord_u(pts):
    u = [0.0]
    for k in range(1, len(pts)):
        u.append(u[-1] + math.dist(pts[k], pts[k - 1]))
    tot = u[-1] or 1.0
    return [x / tot for x in u]


def _gen_bezier(pts, u, t1, t2):
    p0, p3 = pts[0], pts[-1]
    c00 = c01 = c11 = x0 = x1 = 0.0
    for p, ui in zip(pts, u):
        m = 1 - ui
        b0, b1, b2, b3 = m * m * m, 3 * ui * m * m, 3 * ui * ui * m, ui * ui * ui
        a1, a2 = _mul(t1, b1), _mul(t2, b2)
        c00 += _dot(a1, a1)
        c01 += _dot(a1, a2)
        c11 += _dot(a2, a2)
        tmp = _sub(p, _add(_mul(p0, b0 + b1), _mul(p3, b2 + b3)))
        x0 += _dot(a1, tmp)
        x1 += _dot(a2, tmp)
    det = c00 * c11 - c01 * c01
    al = (x0 * c11 - x1 * c01) / det if abs(det) > 1e-12 else 0.0
    ar = (c00 * x1 - c01 * x0) / det if abs(det) > 1e-12 else 0.0
    seg = math.dist(p0, p3)
    if al < 1e-6 * seg or ar < 1e-6 * seg or al > seg or ar > seg:
        al = ar = seg / 3.0
    return (p0, _add(p0, _mul(t1, al)), _add(p3, _mul(t2, ar)), p3)


def _max_err(pts, b, u):
    worst, split = 0.0, len(pts) // 2
    for k in range(1, len(pts) - 1):
        d = _sub(_bez(b, u[k]), pts[k])
        e = d[0] * d[0] + d[1] * d[1]
        if e >= worst:
            worst, split = e, k
    return worst, split


def _newton(b, p, u):
    d = _sub(_bez(b, u), p)
    d1, d2 = _bez1(b, u), _bez2(b, u)
    den = _dot(d1, d1) + _dot(d, d2)
    if abs(den) < 1e-12:
        return u
    return min(1.0, max(0.0, u - _dot(d, d1) / den))


def _fit_cubic(pts, t1, t2, err2):
    if len(pts) == 2:
        dist = math.dist(pts[0], pts[1]) / 3.0
        return [(pts[0], _add(pts[0], _mul(t1, dist)),
                 _add(pts[1], _mul(t2, dist)), pts[1])]
    u = _chord_u(pts)
    b = _gen_bezier(pts, u, t1, t2)
    worst, split = _max_err(pts, b, u)
    if worst < err2:
        return [b]
    if worst < err2 * 16:
        for _ in range(6):
            u = [_newton(b, p, ui) for p, ui in zip(pts, u)]
            b = _gen_bezier(pts, u, t1, t2)
            worst, split = _max_err(pts, b, u)
            if worst < err2:
                return [b]
    ct = _unit(_sub(pts[split - 1], pts[split + 1]))
    return (_fit_cubic(pts[:split + 1], t1, ct, err2)
            + _fit_cubic(pts[split:], _mul(ct, -1), t2, err2))


# --- run classification and assembly ----------------------------------------

def _chord_dev(r):
    a, b = r[0], r[-1]
    d = _sub(b, a)
    L = _len(d)
    if L < 1e-9:
        return float("inf")
    return max(abs(_cross(d, _sub(p, a))) / L for p in r)


def _ls_line(r):
    trim = min(4, len(r) // 5)
    q = r[trim:len(r) - trim] if trim else r
    n = len(q)
    mx = sum(p[0] for p in q) / n
    my = sum(p[1] for p in q) / n
    sxx = sum((p[0] - mx) ** 2 for p in q)
    syy = sum((p[1] - my) ** 2 for p in q)
    sxy = sum((p[0] - mx) * (p[1] - my) for p in q)
    th = 0.5 * math.atan2(2 * sxy, sxx - syy)
    return (mx, my), (math.cos(th), math.sin(th))


def _straight_line(r, tol):
    """Least-squares line through run r if every point stays within tol of it."""
    if len(r) < 8:
        d = _sub(r[-1], r[0])
        if _len(d) < 1e-9 or _chord_dev(r) > tol:
            return None
        return r[0], _unit(d)
    m, d = _ls_line(r)
    trim = min(3, len(r) // 6)
    inner = r[trim:len(r) - trim] if trim else r
    if max(abs(_cross(d, _sub(p, m))) for p in inner) > tol:
        return None
    return m, d


def _project(line, p):
    m, d = line
    return _add(m, _mul(d, _dot(_sub(p, m), d)))


def _intersect(l1, l2):
    (m1, d1), (m2, d2) = l1, l2
    c = _cross(d1, d2)
    if abs(c) < 0.15:
        return None
    s = _cross(_sub(m2, m1), d2) / c
    return _add(m1, _mul(d1, s))


def _smooth_loop(d: str, tol: float):
    """Return new path data for one closed subpath, or None to leave it alone."""
    pts = _parse_loop(d)
    if len(pts) < 3:
        return None
    xs, ys = [p[0] for p in pts], [p[1] for p in pts]
    if max(max(xs) - min(xs), max(ys) - min(ys)) < MIN_SIZE:
        return None
    pts = _resample(pts, STEP)
    n = len(pts)
    sm = _blur(pts)
    corners = _find_corners(sm)
    k = min(CORNER_WINDOW, n // 4)

    def central(i):
        return _unit(_sub(sm[(i + k) % n], sm[(i - k) % n]))

    splits = [(c, True) for c in corners]
    if len(corners) == 0:
        splits = [(0, False), (n // 2, False)]
    elif len(corners) == 1:
        splits.append(((corners[0] + n // 2) % n, False))
    splits.sort()

    runs = []
    for j, (a, a_corner) in enumerate(splits):
        b, b_corner = splits[(j + 1) % len(splits)]
        cnt = (b - a) % n or n
        r = [pts[(a + o) % n] for o in range(cnt + 1)]
        runs.append({"r": r, "ac": a_corner, "bc": b_corner,
                     "ta": None if a_corner else central(a),
                     "tb": None if b_corner else _mul(central(b), -1)})

    for run in runs:
        r = run["r"]
        line = _straight_line(r, tol + 0.8)
        if line:
            run["kind"], run["line"] = "L", line
            continue
        r = _smooth_open(r, 6)  # remove wobble before fitting curves
        m = min(len(r) - 1, 10)
        t1 = run["ta"] or _unit(_sub(r[m], r[0]))
        t2 = run["tb"] or _unit(_sub(r[-1 - m], r[-1]))
        run["kind"], run["segs"] = "C", _fit_cubic(r, t1, t2, tol * tol)

    # Snap corners onto the fitted straight edges (exact line intersection).
    cnt = len(runs)
    verts = [run["r"][0] for run in runs]
    for j in range(cnt):
        prev, nxt = runs[j - 1], runs[j]
        if not (prev["bc"] and nxt["ac"]):
            continue
        v = verts[j]
        if prev["kind"] == "L" and nxt["kind"] == "L":
            p = _intersect(prev["line"], nxt["line"])
            if p and math.dist(p, v) <= 4.0:
                verts[j] = p
        elif prev["kind"] == "L":
            p = _project(prev["line"], v)
            if math.dist(p, v) <= 2.0:
                verts[j] = p
        elif nxt["kind"] == "L":
            p = _project(nxt["line"], v)
            if math.dist(p, v) <= 2.0:
                verts[j] = p

    out = [f"M{_fmt(verts[0][0])} {_fmt(verts[0][1])}"]
    for j, run in enumerate(runs):
        end = verts[(j + 1) % len(runs)]
        if run["kind"] == "L":
            out.append(f"L{_fmt(end[0])} {_fmt(end[1])}")
            continue
        segs = run["segs"]
        for s, (p0, c1, c2, p3) in enumerate(segs):
            if s == len(segs) - 1:
                p3 = end
            if _chord_dev([p0, c1, c2, p3]) < 0.3:
                out.append(f"L{_fmt(p3[0])} {_fmt(p3[1])}")
            else:
                out.append(f"C{_fmt(c1[0])} {_fmt(c1[1])} {_fmt(c2[0])} {_fmt(c2[1])} "
                           f"{_fmt(p3[0])} {_fmt(p3[1])}")
    out.append("Z")
    return " ".join(out)


def smooth_path_data(d: str, tol: float = 0.8) -> str:
    parts = [p for p in re.split(r"(?=M)", d) if p.strip()]
    result = []
    for part in parts:
        try:
            new = _smooth_loop(part, tol)
        except (ValueError, ZeroDivisionError, IndexError):
            new = None
        result.append(new if new else part.strip())
    return " ".join(result)


def smooth_svg(text: str, tol: float = 0.8) -> str:
    return re.sub(r'(<path\s[^>]*?\bd=")([^"]*)(")',
                  lambda m: m.group(1) + smooth_path_data(m.group(2), tol) + m.group(3),
                  text)


def notify(title: str, message: str) -> None:
    """Desktop notification, used when launched from a file manager (no terminal)."""
    if sys.stdout.isatty() or not shutil.which("notify-send"):
        return
    subprocess.run(["notify-send", title, message], check=False)


def convert(src: Path, smooth: bool = True) -> Path:
    dst = src.with_suffix(".svg")
    part = dst.with_name(dst.name + ".part")
    problems = []

    for method in METHODS:
        part.unlink(missing_ok=True)
        r = subprocess.run(worker_command(method, src, part),
                           capture_output=True, text=True)
        if r.returncode == 0 and part.exists() and part.stat().st_size > 0:
            if smooth:
                try:
                    raw = part.read_text(encoding="utf-8")
                    part.write_text(smooth_svg(raw), encoding="utf-8")
                except Exception as e:  # keep the raw trace if cleanup fails
                    print(f"  note: edge smoothing skipped ({e})", file=sys.stderr)
            os.replace(part, dst)
            if problems:
                print(f"  note: '{method}' method worked after: "
                      + "; ".join(problems), file=sys.stderr)
            return dst
        if r.returncode < 0:
            problems.append(f"{method} crashed (signal {-r.returncode})")
        else:
            lines = r.stderr.strip().splitlines()
            problems.append(f"{method} failed: {lines[-1] if lines else 'no output'}")

    part.unlink(missing_ok=True)
    hint = ""
    if sys.version_info >= (3, 14):
        hint = (" Python 3.14 is very new; try a venv with Python 3.12 or 3.13"
                " and reinstall vtracer.")
    raise RuntimeError("; ".join(problems) + "." + hint)


def main() -> int:
    if len(sys.argv) == 5 and sys.argv[1] == "--worker":
        trace(*sys.argv[2:5])
        return 0

    args = sys.argv[1:]
    smooth = "--no-smooth" not in args
    files = [Path(a) for a in args if a != "--no-smooth"]
    if not files:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    ok, failed = 0, 0
    for src in files:
        try:
            if not src.is_file():
                raise FileNotFoundError(f"not found: {src}")
            if src.suffix.lower() != ".png":
                raise ValueError(f"not a PNG: {src.name}")
            dst = convert(src, smooth)
            print(f"{src} -> {dst}")
            ok += 1
        except Exception as e:
            print(f"{src}: FAILED: {e}", file=sys.stderr)
            notify("PNG to SVG failed", f"{src.name}: {e}")
            failed += 1

    if ok:
        notify("PNG to SVG", f"Converted {ok} file{'s' if ok != 1 else ''}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
