//! Edge cleanup for vtracer output.
//!
//! vtracer follows the pixel staircase of an edge, so straight diagonals come
//! out slightly wobbly. This module re-fits every closed outline: straight
//! stretches become exact lines (corners snapped to where the lines meet) and
//! curved stretches become a few smooth cubic Béziers. No dependencies.

use std::f64::consts::PI;

type P = (f64, f64);
type Bez = (P, P, P, P);
/// Circular arc: center, radius, signed sweep in radians.
type Circle = (P, f64, f64);

const STEP: f64 = 1.0; // resampling distance (px)
const CORNER_WINDOW: usize = 10; // samples either side used to measure a turn
const CORNER_ANGLE: f64 = 0.55; // radians (~31 degrees) that counts as a corner
const MIN_SIZE: f64 = 24.0; // shapes smaller than this (px) are left alone
const RMS_LIMIT: f64 = 0.8; // RMS deviation (px, after wobble smoothing) still treated as a line / circle
const FIT_MAX: f64 = 2.6; // worst deviation (px, after wobble smoothing) for a line
const ARC_P98: f64 = 1.6; // 98th-percentile deviation (px) of the points that fit a circle
const ARC_MIN_INLIERS: f64 = 0.75; // share of points that must lie on the circle (rest: inward dents)

pub const DEFAULT_TOLERANCE: f64 = 0.8;

fn sub(a: P, b: P) -> P { (a.0 - b.0, a.1 - b.1) }
fn add(a: P, b: P) -> P { (a.0 + b.0, a.1 + b.1) }
fn mul(a: P, s: f64) -> P { (a.0 * s, a.1 * s) }
fn dot(a: P, b: P) -> f64 { a.0 * b.0 + a.1 * b.1 }
fn cross(a: P, b: P) -> f64 { a.0 * b.1 - a.1 * b.0 }
fn norm(a: P) -> f64 { a.0.hypot(a.1) }
fn dist(a: P, b: P) -> f64 { norm(sub(a, b)) }

fn unit(a: P) -> P {
    let n = norm(a);
    if n > 1e-12 { (a.0 / n, a.1 / n) } else { (0.0, 0.0) }
}

fn fmt(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_string() } else { s.to_string() }
}

// ---------------------------------------------------------------- parsing

#[derive(Clone, Copy)]
enum Tok {
    Cmd(char),
    Num(f64),
}

fn tokenize(d: &str) -> Result<Vec<Tok>, ()> {
    let b: Vec<char> = d.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < b.len() {
                let ch = b[i];
                if ch.is_ascii_digit() || ch == '.' {
                    i += 1;
                } else if ch == 'e' || ch == 'E' {
                    i += 1;
                    if i < b.len() && (b[i] == '-' || b[i] == '+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            let s: String = b[start..i].iter().collect();
            out.push(Tok::Num(s.parse::<f64>().map_err(|_| ())?));
        } else if c.is_whitespace() || c == ',' {
            i += 1;
        } else {
            return Err(());
        }
    }
    Ok(out)
}

fn take(toks: &[Tok], i: &mut usize, k: usize) -> Result<Vec<f64>, ()> {
    let mut v = Vec::with_capacity(k);
    for _ in 0..k {
        match toks.get(*i) {
            Some(Tok::Num(x)) => {
                v.push(*x);
                *i += 1;
            }
            _ => return Err(()),
        }
    }
    Ok(v)
}

/// Parse one `M ... Z` subpath (commands M, L, C, Z only) into a dense,
/// closed polyline. Curves are sampled finely.
fn parse_loop(d: &str) -> Result<Vec<P>, ()> {
    let toks = tokenize(d)?;
    let mut pts: Vec<P> = Vec::new();
    let mut i = 0;
    let mut cmd = ' ';
    let mut pos: P = (0.0, 0.0);
    let mut have_m = false;

    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            if !matches!(c, 'M' | 'L' | 'C' | 'Z') {
                return Err(());
            }
            cmd = c;
            i += 1;
            if cmd == 'Z' {
                continue;
            }
        } else if cmd == ' ' || cmd == 'Z' {
            return Err(());
        }

        match cmd {
            'M' => {
                if have_m {
                    return Err(());
                }
                let v = take(&toks, &mut i, 2)?;
                pos = (v[0], v[1]);
                pts.push(pos);
                have_m = true;
                cmd = 'L';
            }
            'L' => {
                let v = take(&toks, &mut i, 2)?;
                pos = (v[0], v[1]);
                pts.push(pos);
            }
            'C' => {
                let v = take(&toks, &mut i, 6)?;
                let p0 = pos;
                let c1 = (v[0], v[1]);
                let c2 = (v[2], v[3]);
                let e = (v[4], v[5]);
                let ctrl = dist(p0, c1) + dist(c1, c2) + dist(c2, e);
                let steps = ((ctrl / 2.0) as usize).clamp(8, 400);
                for s in 1..=steps {
                    let u = s as f64 / steps as f64;
                    let m = 1.0 - u;
                    let (a, b, c, dd) = (m * m * m, 3.0 * m * m * u, 3.0 * m * u * u, u * u * u);
                    pts.push((
                        a * p0.0 + b * c1.0 + c * c2.0 + dd * e.0,
                        a * p0.1 + b * c1.1 + c * c2.1 + dd * e.1,
                    ));
                }
                pos = e;
            }
            _ => return Err(()),
        }
    }

    if pts.len() > 1 && dist(pts[0], pts[pts.len() - 1]) < 1e-9 {
        pts.pop();
    }
    Ok(pts)
}

// ------------------------------------------------------ polyline utilities

fn resample(pts: &[P], step: f64) -> Vec<P> {
    let n = pts.len();
    let seg: Vec<f64> = (0..n).map(|k| dist(pts[k], pts[(k + 1) % n])).collect();
    let total: f64 = seg.iter().sum();
    let m = ((total / step).round() as usize).max(8);
    let sp = total / m as f64;
    let mut out = Vec::with_capacity(m);
    let mut k = 0usize;
    let mut acc = 0.0f64;
    for j in 0..m {
        let target = j as f64 * sp;
        while acc + seg[k] < target && k < n - 1 {
            acc += seg[k];
            k += 1;
        }
        let f = if seg[k] > 0.0 { (target - acc) / seg[k] } else { 0.0 };
        let p = pts[k];
        let q = pts[(k + 1) % n];
        out.push((p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f));
    }
    out
}

fn blur(pts: &[P], r: usize) -> Vec<P> {
    let n = pts.len() as isize;
    let w = (2 * r + 1) as f64;
    let r = r as isize;
    (0..n)
        .map(|i| {
            let (mut sx, mut sy) = (0.0, 0.0);
            for o in -r..=r {
                let p = pts[(i + o).rem_euclid(n) as usize];
                sx += p.0;
                sy += p.1;
            }
            (sx / w, sy / w)
        })
        .collect()
}

fn argmax(idx: &[usize], ang: &[f64]) -> usize {
    let mut best = idx[0];
    for &i in idx {
        if ang[i] > ang[best] {
            best = i;
        }
    }
    best
}

fn find_corners(sm: &[P]) -> Vec<usize> {
    let n = sm.len();
    let k = CORNER_WINDOW.min(n / 4);
    let ang: Vec<f64> = (0..n)
        .map(|i| {
            let a = sub(sm[i], sm[(i + n - k) % n]);
            let b = sub(sm[(i + k) % n], sm[i]);
            cross(a, b).atan2(dot(a, b)).abs()
        })
        .collect();
    let flag: Vec<bool> = ang.iter().map(|&x| x > CORNER_ANGLE).collect();
    let all: Vec<usize> = (0..n).collect();
    let start = match flag.iter().position(|&f| !f) {
        Some(s) => s,
        None => return vec![argmax(&all, &ang)],
    };
    let mut corners = Vec::new();
    let mut group: Vec<usize> = Vec::new();
    for o in 1..=n {
        let i = (start + o) % n;
        if flag[i] {
            group.push(i);
        } else if !group.is_empty() {
            corners.push(argmax(&group, &ang));
            group.clear();
        }
    }
    corners.sort();
    corners
}

/// Moving average over an open run. The window shrinks toward the ends so the
/// end points (corners) stay exactly where they are.
fn smooth_open(r: &[P], h: usize) -> Vec<P> {
    let n = r.len();
    (0..n)
        .map(|i| {
            let w = h.min(i).min(n - 1 - i);
            if w == 0 {
                return r[i];
            }
            let seg = &r[i - w..=i + w];
            let c = seg.len() as f64;
            (
                seg.iter().map(|p| p.0).sum::<f64>() / c,
                seg.iter().map(|p| p.1).sum::<f64>() / c,
            )
        })
        .collect()
}

// -------------------------------------- Schneider's curve fitting (1990)

fn bez(b: &Bez, t: f64) -> P {
    let (p0, p1, p2, p3) = *b;
    let m = 1.0 - t;
    let (a, c, d, e) = (m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t);
    (
        a * p0.0 + c * p1.0 + d * p2.0 + e * p3.0,
        a * p0.1 + c * p1.1 + d * p2.1 + e * p3.1,
    )
}

fn bez1(b: &Bez, t: f64) -> P {
    let (p0, p1, p2, p3) = *b;
    let m = 1.0 - t;
    (
        3.0 * (m * m * (p1.0 - p0.0) + 2.0 * m * t * (p2.0 - p1.0) + t * t * (p3.0 - p2.0)),
        3.0 * (m * m * (p1.1 - p0.1) + 2.0 * m * t * (p2.1 - p1.1) + t * t * (p3.1 - p2.1)),
    )
}

fn bez2(b: &Bez, t: f64) -> P {
    let (p0, p1, p2, p3) = *b;
    let m = 1.0 - t;
    (
        6.0 * (m * (p2.0 - 2.0 * p1.0 + p0.0) + t * (p3.0 - 2.0 * p2.0 + p1.0)),
        6.0 * (m * (p2.1 - 2.0 * p1.1 + p0.1) + t * (p3.1 - 2.0 * p2.1 + p1.1)),
    )
}

fn chord_u(pts: &[P]) -> Vec<f64> {
    let mut u = vec![0.0];
    for k in 1..pts.len() {
        let last = u[k - 1];
        u.push(last + dist(pts[k], pts[k - 1]));
    }
    let tot = if u[u.len() - 1] > 0.0 { u[u.len() - 1] } else { 1.0 };
    u.iter().map(|x| x / tot).collect()
}

fn gen_bezier(pts: &[P], u: &[f64], t1: P, t2: P) -> Bez {
    let p0 = pts[0];
    let p3 = pts[pts.len() - 1];
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (p, &ui) in pts.iter().zip(u.iter()) {
        let m = 1.0 - ui;
        let (b0, b1, b2, b3) = (m * m * m, 3.0 * ui * m * m, 3.0 * ui * ui * m, ui * ui * ui);
        let a1 = mul(t1, b1);
        let a2 = mul(t2, b2);
        c00 += dot(a1, a1);
        c01 += dot(a1, a2);
        c11 += dot(a2, a2);
        let tmp = sub(*p, add(mul(p0, b0 + b1), mul(p3, b2 + b3)));
        x0 += dot(a1, tmp);
        x1 += dot(a2, tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let (mut al, mut ar) = if det.abs() > 1e-12 {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (0.0, 0.0)
    };
    let seg = dist(p0, p3);
    if al < 1e-6 * seg || ar < 1e-6 * seg || al > seg || ar > seg {
        al = seg / 3.0;
        ar = seg / 3.0;
    }
    (p0, add(p0, mul(t1, al)), add(p3, mul(t2, ar)), p3)
}

fn max_err(pts: &[P], b: &Bez, u: &[f64]) -> (f64, usize) {
    let mut worst = 0.0;
    let mut split = pts.len() / 2;
    for k in 1..pts.len() - 1 {
        let d = sub(bez(b, u[k]), pts[k]);
        let e = d.0 * d.0 + d.1 * d.1;
        if e >= worst {
            worst = e;
            split = k;
        }
    }
    (worst, split)
}

fn newton(b: &Bez, p: P, u: f64) -> f64 {
    let d = sub(bez(b, u), p);
    let d1 = bez1(b, u);
    let d2 = bez2(b, u);
    let den = dot(d1, d1) + dot(d, d2);
    if den.abs() < 1e-12 {
        return u;
    }
    (u - dot(d, d1) / den).clamp(0.0, 1.0)
}

/// Fit cubic Béziers to `pts`. `t1` points forward at the start, `t2` points
/// backward at the end; `err2` is the squared error tolerance.
fn fit_cubic(pts: &[P], t1: P, t2: P, err2: f64) -> Vec<Bez> {
    if pts.len() == 2 {
        let d = dist(pts[0], pts[1]) / 3.0;
        return vec![(pts[0], add(pts[0], mul(t1, d)), add(pts[1], mul(t2, d)), pts[1])];
    }
    let mut u = chord_u(pts);
    let mut b = gen_bezier(pts, &u, t1, t2);
    let (mut worst, mut split) = max_err(pts, &b, &u);
    if worst < err2 {
        return vec![b];
    }
    if worst < err2 * 16.0 {
        for _ in 0..6 {
            u = pts.iter().zip(u.iter()).map(|(p, &ui)| newton(&b, *p, ui)).collect();
            b = gen_bezier(pts, &u, t1, t2);
            let r = max_err(pts, &b, &u);
            worst = r.0;
            split = r.1;
            if worst < err2 {
                return vec![b];
            }
        }
    }
    let ct = unit(sub(pts[split - 1], pts[split + 1]));
    let mut left = fit_cubic(&pts[..=split], t1, ct, err2);
    let right = fit_cubic(&pts[split..], mul(ct, -1.0), t2, err2);
    left.extend(right);
    left
}

// ------------------------------------------- lines, corners, assembly

fn chord_dev(r: &[P]) -> f64 {
    let a = r[0];
    let d = sub(r[r.len() - 1], a);
    let l = norm(d);
    if l < 1e-9 {
        return f64::INFINITY;
    }
    r.iter().map(|&p| cross(d, sub(p, a)).abs() / l).fold(0.0, f64::max)
}

/// Least-squares line (point, unit direction) through a run.
fn ls_line(r: &[P]) -> (P, P) {
    let trim = 4usize.min(r.len() / 5);
    let q = &r[trim..r.len() - trim];
    let n = q.len() as f64;
    let mx = q.iter().map(|p| p.0).sum::<f64>() / n;
    let my = q.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = q.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    let syy: f64 = q.iter().map(|p| (p.1 - my) * (p.1 - my)).sum();
    let sxy: f64 = q.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let th = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    ((mx, my), (th.cos(), th.sin()))
}

/// The run's line if it stays within `tol` (and about RMS_LIMIT px RMS) of it.
fn straight_line(r: &[P], tol: f64) -> Option<(P, P)> {
    if r.len() < 8 {
        let d = sub(r[r.len() - 1], r[0]);
        if norm(d) < 1e-9 || chord_dev(r) > tol {
            return None;
        }
        return Some((r[0], unit(d)));
    }
    let (m, d) = ls_line(r);
    let trim = 3usize.min(r.len() / 6);
    let inner = &r[trim..r.len() - trim];
    let devs: Vec<f64> = inner.iter().map(|&p| cross(d, sub(p, m)).abs()).collect();
    let worst = devs.iter().cloned().fold(0.0, f64::max);
    let rms = (devs.iter().map(|x| x * x).sum::<f64>() / devs.len() as f64).sqrt();
    if worst > tol || rms > RMS_LIMIT { None } else { Some((m, d)) }
}

fn project(line: (P, P), p: P) -> P {
    let (m, d) = line;
    add(m, mul(d, dot(sub(p, m), d)))
}

fn intersect(l1: (P, P), l2: (P, P)) -> Option<P> {
    let (m1, d1) = l1;
    let (m2, d2) = l2;
    let c = cross(d1, d2);
    if c.abs() < 0.15 {
        return None;
    }
    let s = cross(sub(m2, m1), d2) / c;
    Some(add(m1, mul(d1, s)))
}

// ------------------------------------------------------- circular arcs

/// Least-squares circle (center, radius) through `r`.
fn fit_circle(r: &[P]) -> Option<(P, f64)> {
    let n = r.len();
    if n < 5 {
        return None;
    }
    let nf = n as f64;
    let mx = r.iter().map(|p| p.0).sum::<f64>() / nf;
    let my = r.iter().map(|p| p.1).sum::<f64>() / nf;
    let (mut suu, mut svv, mut suv) = (0.0f64, 0.0f64, 0.0f64);
    let (mut suuu, mut svvv, mut suvv, mut svuu) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for p in r {
        let u = p.0 - mx;
        let v = p.1 - my;
        suu += u * u;
        svv += v * v;
        suv += u * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-9 {
        return None;
    }
    let r1 = 0.5 * (suuu + suvv);
    let r2 = 0.5 * (svvv + svuu);
    let mut cx = mx + (r1 * svv - r2 * suv) / det;
    let mut cy = my + (suu * r2 - suv * r1) / det;

    // Refine on true geometric distance (Gauss-Newton).
    for _ in 0..6 {
        let ds: Vec<f64> = r.iter().map(|p| (p.0 - cx).hypot(p.1 - cy)).collect();
        if ds.iter().cloned().fold(f64::INFINITY, f64::min) < 1e-9 {
            return None;
        }
        let rad = ds.iter().sum::<f64>() / nf;
        let (mut a11, mut a12, mut a22) = (0.0f64, 0.0f64, 0.0f64);
        let (mut b1, mut b2, mut mjx, mut mjy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (p, &dd) in r.iter().zip(ds.iter()) {
            let jx = (cx - p.0) / dd;
            let jy = (cy - p.1) / dd;
            let res = dd - rad;
            a11 += jx * jx;
            a12 += jx * jy;
            a22 += jy * jy;
            b1 += jx * res;
            b2 += jy * res;
            mjx += jx;
            mjy += jy;
        }
        mjx /= nf;
        mjy /= nf;
        a11 -= nf * mjx * mjx;
        a12 -= nf * mjx * mjy;
        a22 -= nf * mjy * mjy;
        let dt = a11 * a22 - a12 * a12;
        if dt.abs() < 1e-12 {
            break;
        }
        let sx = (b1 * a22 - b2 * a12) / dt;
        let sy = (a11 * b2 - a12 * b1) / dt;
        cx -= sx;
        cy -= sy;
        if sx.abs() + sy.abs() < 1e-7 {
            break;
        }
    }
    let rad = r.iter().map(|p| (p.0 - cx).hypot(p.1 - cy)).sum::<f64>() / nf;
    Some(((cx, cy), rad))
}

/// If the run lies on a circular arc, return it.
///
/// The circle is fit robustly: points far from it are ignored, so a localized
/// dent where the tracer cut inside the true circle does not spoil the fit.
/// Only inward dents are forgiven (that is what a tracer's chord shortcuts
/// do); an outward bulge is a real feature and rejects the circle.
fn arc_from_run(r: &[P], tol: f64) -> Option<Circle> {
    let n = r.len();
    let cmp = |a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
    let mut fit = fit_circle(r)?;
    let mut thr = ARC_P98;
    for _ in 0..6 {
        let (c, rad) = fit;
        let sd: Vec<f64> = r.iter().map(|&p| dist(p, c) - rad).collect();
        let mut sorted = sd.clone();
        sorted.sort_by(cmp);
        let med = sorted[n / 2];
        let mut ad: Vec<f64> = sd.iter().map(|x| (x - med).abs()).collect();
        ad.sort_by(cmp);
        thr = ARC_P98.max(3.0 * 1.4826 * ad[n / 2]);
        let inl: Vec<P> = (0..n).filter(|&i| sd[i].abs() <= thr).map(|i| r[i]).collect();
        if (inl.len() as f64) < 0.5 * n as f64 {
            return None;
        }
        let refit = fit_circle(&inl)?;
        let done = dist(refit.0, c) < 1e-3 && (refit.1 - rad).abs() < 1e-3;
        fit = refit;
        if done {
            break;
        }
    }
    let (c, rad) = fit;
    if rad < 6.0 || rad > 1e5 {
        return None;
    }
    let sd: Vec<f64> = r.iter().map(|&p| dist(p, c) - rad).collect();
    let mut inl: Vec<f64> = sd.iter().filter(|x| x.abs() <= thr).map(|x| x.abs()).collect();
    let outl: Vec<f64> = sd.iter().cloned().filter(|x| x.abs() > thr).collect();
    if (inl.len() as f64) < ARC_MIN_INLIERS * n as f64 {
        return None;
    }
    inl.sort_by(cmp);
    let rms = (inl.iter().map(|x| x * x).sum::<f64>() / inl.len() as f64).sqrt();
    let p98 = inl[(0.98 * (inl.len() - 1) as f64) as usize];
    if rms > RMS_LIMIT || p98 > tol {
        return None;
    }
    if !outl.is_empty() {
        let inward = outl.iter().filter(|&&x| x < 0.0).count();
        let deepest = outl.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        if (inward as f64) < 0.9 * outl.len() as f64 || deepest > 12.0f64.max(0.05 * rad) {
            return None;
        }
    }
    let (mut total, mut steps) = (0.0f64, 0.0f64);
    let mut prev = (r[0].1 - c.1).atan2(r[0].0 - c.0);
    for p in &r[1..] {
        let a = (p.1 - c.1).atan2(p.0 - c.0);
        let da = (a - prev + PI).rem_euclid(2.0 * PI) - PI;
        total += da;
        steps += da.abs();
        prev = a;
    }
    if total.abs() < 0.05 || total.abs() > 2.0 * PI + 0.3 || total.abs() < 0.9 * steps {
        return None;
    }
    Some((c, rad, total))
}

/// Where a line crosses a circle: the crossing closest to `near`.
fn line_circle(line: (P, P), c: P, rad: f64, near: P) -> Option<P> {
    let (m, d) = line;
    let f = sub(m, c);
    let b = dot(d, f);
    let disc = b * b - (dot(f, f) - rad * rad);
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    let p1 = add(m, mul(d, -b + sq));
    let p2 = add(m, mul(d, -b - sq));
    Some(if dist(p1, near) <= dist(p2, near) { p1 } else { p2 })
}

/// Cubic Béziers (at most 90 degrees each) along a circle: (c1, c2, end).
fn arc_beziers(c: P, rad: f64, a0: f64, sweep: f64) -> Vec<(P, P, P)> {
    let n = ((sweep.abs() / (PI / 2.0) - 1e-9).ceil() as usize).max(1);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = a0 + sweep * i as f64 / n as f64;
        let b = a0 + sweep * (i + 1) as f64 / n as f64;
        let k = 4.0 / 3.0 * ((b - a) / 4.0).tan();
        let p0 = (c.0 + rad * a.cos(), c.1 + rad * a.sin());
        let p3 = (c.0 + rad * b.cos(), c.1 + rad * b.sin());
        let c1 = (p0.0 - k * rad * a.sin(), p0.1 + k * rad * a.cos());
        let c2 = (p3.0 + k * rad * b.sin(), p3.1 - k * rad * b.cos());
        out.push((c1, c2, p3));
    }
    out
}

fn cubic_cmd(c1: P, c2: P, p3: P) -> String {
    format!(
        "C{} {} {} {} {} {}",
        fmt(c1.0), fmt(c1.1), fmt(c2.0), fmt(c2.1), fmt(p3.0), fmt(p3.1)
    )
}

/// Path commands for a fitted arc between two vertices.
fn arc_commands(arc: Circle, start: P, end: P) -> Vec<String> {
    let (c, rad, sweep) = arc;
    let a0 = (start.1 - c.1).atan2(start.0 - c.0);
    let a1 = (end.1 - c.1).atan2(end.0 - c.0);
    let mut s = a1 - a0;
    s += 2.0 * PI * ((sweep - s) / (2.0 * PI)).round();
    let pieces = arc_beziers(c, rad, a0, s);
    let last = pieces.len() - 1;
    pieces
        .iter()
        .enumerate()
        .map(|(i, &(c1, c2, p3))| cubic_cmd(c1, c2, if i == last { end } else { p3 }))
        .collect()
}

struct Run {
    r: Vec<P>,
    ac: bool, // starts at a real corner
    bc: bool, // ends at a real corner
    ta: Option<P>,
    tb: Option<P>,
    line: Option<(P, P)>,
    arc: Option<Circle>,
    segs: Vec<Bez>,
}

/// Returns new path data for one closed subpath, or None to leave it alone.
fn smooth_loop(d: &str, tol: f64) -> Option<String> {
    let pts0 = parse_loop(d).ok()?;
    if pts0.len() < 3 {
        return None;
    }
    let minx = pts0.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let maxx = pts0.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let miny = pts0.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let maxy = pts0.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    if (maxx - minx).max(maxy - miny) < MIN_SIZE {
        return None;
    }

    let pts = resample(&pts0, STEP);
    let n = pts.len();
    let sm = blur(&pts, 3);
    let corners = find_corners(&sm);
    if corners.is_empty() {
        // No corners at all: maybe the whole outline is one circle.
        let mut closed = blur(&pts, 6);
        closed.push(closed[0]);
        if let Some((c, rad, sweep)) = arc_from_run(&closed, ARC_P98) {
            if (sweep.abs() - 2.0 * PI).abs() < 0.3 {
                let a0 = (pts[0].1 - c.1).atan2(pts[0].0 - c.0);
                let sgn = if sweep > 0.0 { 1.0 } else { -1.0 };
                let start = (c.0 + rad * a0.cos(), c.1 + rad * a0.sin());
                let mut cmds = vec![format!("M{} {}", fmt(start.0), fmt(start.1))];
                for (c1, c2, p3) in arc_beziers(c, rad, a0, sgn * 2.0 * PI) {
                    cmds.push(cubic_cmd(c1, c2, p3));
                }
                cmds.push("Z".to_string());
                return Some(cmds.join(" "));
            }
        }
    }
    let k = CORNER_WINDOW.min(n / 4);
    let central = |i: usize| -> P { unit(sub(sm[(i + k) % n], sm[(i + n - k) % n])) };

    let mut splits: Vec<(usize, bool)> = corners.iter().map(|&c| (c, true)).collect();
    if corners.is_empty() {
        splits = vec![(0, false), (n / 2, false)];
    } else if corners.len() == 1 {
        splits.push(((corners[0] + n / 2) % n, false));
    }
    splits.sort();

    let ns = splits.len();
    let mut runs: Vec<Run> = Vec::with_capacity(ns);
    for j in 0..ns {
        let (a, ac) = splits[j];
        let (b, bc) = splits[(j + 1) % ns];
        let mut cnt = (b + n - a) % n;
        if cnt == 0 {
            cnt = n;
        }
        let r: Vec<P> = (0..=cnt).map(|o| pts[(a + o) % n]).collect();
        runs.push(Run {
            r,
            ac,
            bc,
            ta: if ac { None } else { Some(central(a)) },
            tb: if bc { None } else { Some(mul(central(b), -1.0)) },
            line: None,
            arc: None,
            segs: Vec::new(),
        });
    }

    for run in runs.iter_mut() {
        let r = smooth_open(&run.r, 6); // average out vtracer's wobble first
        if let Some(line) = straight_line(&r, FIT_MAX) {
            run.line = Some(line);
            continue;
        }
        if let Some(arc) = arc_from_run(&r, ARC_P98) {
            run.arc = Some(arc);
            continue;
        }
        let m = (r.len() - 1).min(10);
        let last = r.len() - 1;
        let t1 = run.ta.unwrap_or_else(|| unit(sub(r[m], r[0])));
        let t2 = run.tb.unwrap_or_else(|| unit(sub(r[last - m], r[last])));
        run.segs = fit_cubic(&r, t1, t2, tol * tol);
    }

    // Snap corners onto the fitted straight edges.
    let rc = runs.len();
    let mut verts: Vec<P> = runs.iter().map(|r| r.r[0]).collect();
    for j in 0..rc {
        let prev = &runs[(j + rc - 1) % rc];
        let nxt = &runs[j];
        if !(prev.bc && nxt.ac) {
            continue;
        }
        let v = verts[j];
        let target = match (prev.line, prev.arc, nxt.line, nxt.arc) {
            (Some(pl), _, Some(nl), _) => intersect(pl, nl).filter(|p| dist(*p, v) <= 4.0),
            (Some(pl), _, None, Some((c, rad, _))) => line_circle(pl, c, rad, v)
                .filter(|p| dist(*p, v) <= 4.0)
                .or_else(|| Some(project(pl, v)).filter(|p| dist(*p, v) <= 2.0)),
            (None, Some((c, rad, _)), Some(nl), _) => line_circle(nl, c, rad, v)
                .filter(|p| dist(*p, v) <= 4.0)
                .or_else(|| Some(project(nl, v)).filter(|p| dist(*p, v) <= 2.0)),
            (Some(pl), _, None, None) => Some(project(pl, v)).filter(|p| dist(*p, v) <= 2.0),
            (None, None, Some(nl), _) => Some(project(nl, v)).filter(|p| dist(*p, v) <= 2.0),
            _ => None,
        };
        if let Some(p) = target {
            verts[j] = p;
        }
    }

    let mut out: Vec<String> = vec![format!("M{} {}", fmt(verts[0].0), fmt(verts[0].1))];
    for j in 0..rc {
        let end = verts[(j + 1) % rc];
        let run = &runs[j];
        if run.line.is_some() {
            out.push(format!("L{} {}", fmt(end.0), fmt(end.1)));
            continue;
        }
        if let Some(arc) = run.arc {
            out.extend(arc_commands(arc, verts[j], end));
            continue;
        }
        let count = run.segs.len();
        for (s, seg) in run.segs.iter().enumerate() {
            let (p0, c1, c2, mut p3) = *seg;
            if s + 1 == count {
                p3 = end;
            }
            if chord_dev(&[p0, c1, c2, p3]) < 0.3 {
                out.push(format!("L{} {}", fmt(p3.0), fmt(p3.1)));
            } else {
                out.push(format!(
                    "C{} {} {} {} {} {}",
                    fmt(c1.0), fmt(c1.1), fmt(c2.0), fmt(c2.1), fmt(p3.0), fmt(p3.1)
                ));
            }
        }
    }
    out.push("Z".to_string());
    Some(out.join(" "))
}

// ------------------------------------------------------------ public API

/// Clean up the `d` attribute of one `<path>`; each subpath is handled alone
/// and any subpath that can't be processed is returned unchanged.
pub fn smooth_path_data(d: &str, tol: f64) -> String {
    let mut bounds: Vec<usize> = vec![0];
    for (i, c) in d.char_indices() {
        if c == 'M' && i != 0 {
            bounds.push(i);
        }
    }
    bounds.push(d.len());

    let mut parts: Vec<String> = Vec::new();
    for w in bounds.windows(2) {
        let part = &d[w[0]..w[1]];
        if part.trim().is_empty() {
            continue;
        }
        let new = std::panic::catch_unwind(|| smooth_loop(part, tol)).ok().flatten();
        parts.push(new.unwrap_or_else(|| part.trim().to_string()));
    }
    parts.join(" ")
}

/// Clean up every `<path d="...">` in an SVG document.
pub fn smooth_svg(text: &str, tol: f64) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(p) = rest.find("<path") {
        let after = &rest[p..];
        let tag_end = match after.find('>') {
            Some(e) => e,
            None => break,
        };
        let tag = &after[..tag_end];
        if let Some(dpos) = tag.find(" d=\"") {
            let vstart = dpos + 4;
            if let Some(vlen) = tag[vstart..].find('"') {
                out.push_str(&rest[..p + vstart]);
                out.push_str(&smooth_path_data(&tag[vstart..vstart + vlen], tol));
                rest = &rest[p + vstart + vlen..];
                continue;
            }
        }
        out.push_str(&rest[..p + 5]);
        rest = &rest[p + 5..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_trims_zeros() {
        assert_eq!(fmt(1.5), "1.5");
        assert_eq!(fmt(100.0), "100");
        assert_eq!(fmt(-0.001), "0");
        assert_eq!(fmt(0.0), "0");
    }

    #[test]
    fn square_stays_square() {
        let out = smooth_path_data("M0 0 L100 0 L100 100 L0 100 Z", DEFAULT_TOLERANCE);
        assert!(out.starts_with("M0 0 L100 0 L100 100 L0 100"), "{}", out);
        assert!(!out.contains('C'), "{}", out);
    }

    #[test]
    fn wobbly_diagonal_becomes_straight() {
        // Diagonal edge from (0,0) to (300,150) with +-0.6 px wobble.
        let mut d = String::from("M0 0");
        let (dx, dy) = (300.0f64, 150.0f64);
        let len = dx.hypot(dy);
        let (nx, ny) = (-dy / len, dx / len);
        for i in 1..150 {
            let t = i as f64 / 150.0;
            let w = 0.6 * (t * len / 6.0).sin();
            d.push_str(&format!(" L{:.2} {:.2}", t * dx + nx * w, t * dy + ny * w));
        }
        d.push_str(" L300 150 L300 400 L0 400 Z");
        let out = smooth_path_data(&d, DEFAULT_TOLERANCE);
        assert!(!out.contains('C'), "{}", out);
        assert!(out.matches('L').count() <= 5, "{}", out);
    }

    #[test]
    fn circle_becomes_four_exact_arcs() {
        let mut d = String::from("M600 300");
        for i in 1..400 {
            let a = 2.0 * PI * i as f64 / 400.0;
            d.push_str(&format!(" L{:.2} {:.2}", 500.0 + 100.0 * a.cos(), 300.0 + 100.0 * a.sin()));
        }
        d.push_str(" Z");
        let out = smooth_path_data(&d, DEFAULT_TOLERANCE);
        assert_eq!(out.matches('C').count(), 4, "{}", out);
        assert!(!out.contains('L'), "{}", out);
    }

    #[test]
    fn circle_with_inward_dent_is_still_a_circle() {
        // Radius 200 with a 6 px dent (like a tracer's chord shortcut) over 20 degrees.
        let mut d = String::from("M700 300");
        for i in 1..600 {
            let a = 2.0 * PI * i as f64 / 600.0;
            let deg = a.to_degrees();
            let mut r = 200.0;
            if (100.0..=120.0).contains(&deg) {
                r -= 6.0 * (1.0 - (deg - 110.0).abs() / 10.0);
            }
            d.push_str(&format!(" L{:.2} {:.2}", 500.0 + r * a.cos(), 300.0 + r * a.sin()));
        }
        d.push_str(" Z");
        let out = smooth_path_data(&d, DEFAULT_TOLERANCE);
        assert_eq!(out.matches('C').count(), 4, "{}", out);
        assert!(!out.contains('L'), "{}", out);
    }

    #[test]
    fn unsupported_commands_are_left_alone() {
        let d = "M0 0 H10 V10 Z";
        assert_eq!(smooth_path_data(d, DEFAULT_TOLERANCE), d);
    }
}
