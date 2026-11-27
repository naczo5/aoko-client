#!/usr/bin/env python3
"""Deterministic generator for the built-in cosmetics/sky assets.

Format contract: .agents/skills/visuals-pipeline/references/asset-format.md

    python3 scripts/visuals/generate_assets.py            # write assets + previews
    python3 scripts/visuals/generate_assets.py --check    # exit 1 if committed files differ

All randomness comes from fixed seeds. No timestamps are written, so two runs
produce byte-identical files (same Pillow/numpy versions assumed).
"""
import argparse
import io
import json
import math
import re
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
OUT = REPO / "Aoko" / "Assets" / "Visuals"
PREVIEW = HERE / "preview"
ID_RE = re.compile(r"^[a-z0-9][a-z0-9_-]{0,31}$")


def C(h):
    h = h.lstrip("#")
    return np.array([int(h[i:i + 2], 16) / 255.0 for i in (0, 2, 4)])


def to_hex(c):
    c = np.clip(np.asarray(c) * 255 + 0.5, 0, 255).astype(int)
    return "#%02x%02x%02x" % tuple(c)


def ramp(t, stops):
    """Piecewise-linear colour ramp. stops: [(pos, '#hex' | rgb)]."""
    pos = [s[0] for s in stops]
    cols = [C(s[1]) if isinstance(s[1], str) else np.asarray(s[1], float) for s in stops]
    t = np.asarray(t, float)
    return np.stack([np.interp(t, pos, [c[k] for c in cols]) for k in range(3)], -1)


def smoothstep(a, b, x):
    t = np.clip((np.asarray(x, float) - a) / (b - a), 0, 1)
    return t * t * (3 - 2 * t)


def png_bytes(arr):
    buf = io.BytesIO()
    Image.fromarray(arr).save(buf, "PNG", compress_level=9)
    return buf.getvalue()


def json_bytes(obj):
    return (json.dumps(obj, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


# ---------------------------------------------------------------- capes ----
SS = 8            # supersampling factor for cape art
FW, FH = 20, 32   # face size in 128x64 texels (10x16 vanilla, x2)


def mask(w, h, fn, blur=0.0):
    im = Image.new("L", (w, h), 0)
    fn(ImageDraw.Draw(im))
    if blur:
        im = im.filter(ImageFilter.GaussianBlur(blur))
    return np.asarray(im, dtype=np.float64) / 255.0


class Canvas:
    """Supersampled RGB canvas addressed in face texel units (20 x 32)."""

    def __init__(self, base):
        self.w, self.h = FW * SS, FH * SS
        self.px = base.copy()

    def over(self, m, col, a=1.0):
        col = np.asarray(col, float)
        m = (m * a)[..., None]
        self.px = self.px * (1 - m) + col * m

    def add(self, m, col, a=1.0):
        self.px = self.px + (m * a)[..., None] * np.asarray(col, float)

    def shape(self, fn, col, a=1.0, blur=0.0):
        self.over(mask(self.w, self.h, fn, blur * SS), col, a)

    def glow(self, fn, col, a, blur):
        self.add(mask(self.w, self.h, fn, blur * SS), col, a)

    def down(self):
        return self.px.reshape(FH, SS, FW, SS, 3).mean(axis=(1, 3))


def P(*pts):
    return [(x * SS, y * SS) for x, y in pts]


def ellipse(d, cx, cy, rx, ry=None, fill=255):
    ry = rx if ry is None else ry
    d.ellipse([(cx - rx) * SS, (cy - ry) * SS, (cx + rx) * SS, (cy + ry) * SS], fill=fill)


def vgrad_ss(stops):
    h, w = FH * SS, FW * SS
    t = (np.arange(h) + 0.5) / h
    return np.repeat(ramp(t, stops)[:, None, :], w, axis=1)


def yy_xx():
    h, w = FH * SS, FW * SS
    y, x = np.mgrid[0:h, 0:w]
    return (y + 0.5) / SS, (x + 0.5) / SS  # face texel units


def cape_base(kind):
    if kind == "aoko-glass":
        b = vgrad_ss([(0, "#14355e"), (0.45, "#0c2444"), (1, "#07152b")])
        y, x = yy_xx()
        band = np.exp(-(((x / FW + y / FH * 0.55) - 0.55) / 0.13) ** 2)
        b = b + 0.055 * band[..., None] * C("#9fe0ff")
        b = b + 0.10 * np.exp(-y / 2.2)[..., None] * C("#bfeaff")
        return b
    if kind == "tsukihime-moon":
        b = vgrad_ss([(0, "#090b2a"), (0.5, "#141a4a"), (0.8, "#192860"), (1, "#173a6a")])
        y, x = yy_xx()
        haze = smoothstep(0.62, 1.0, y / FH) ** 1.5
        return b + 0.42 * haze[..., None] * C("#38c4e0")
    if kind == "sakura-drift":
        b = vgrad_ss([(0, "#f5b8d0"), (0.35, "#e79bbd"), (0.7, "#a479b5"), (1, "#5a4690")])
        y, _ = yy_xx()
        hem = smoothstep(25.2, 26.6, y)[..., None]
        b = b * (1 - hem) + C("#2c1d52") * hem
        line = np.exp(-((y - 26.0) / 0.35) ** 2)[..., None]
        return b + 0.16 * line * C("#ffd0e6")
    if kind == "void-star":
        b = vgrad_ss([(0, "#07060f"), (0.5, "#0a0818"), (1, "#0c0a1c")])
        y, x = yy_xx()
        r2 = ((x - 10) / 9) ** 2 + ((y - 12) / 12) ** 2
        return b + 0.14 * np.exp(-r2 * 2.5)[..., None] * C("#6a5bd0")
    if kind == "ember-crest":
        b = vgrad_ss([(0, "#1d1e24"), (0.4, "#2a1519"), (0.75, "#561018"), (1, "#74101f")])
        y, _ = yy_xx()
        return b + 0.12 * smoothstep(0.7, 1.0, y / FH)[..., None] * C("#ff7a2a")
    raise KeyError(kind)


def sparkle(c, rng, n, region, rmin, rmax, cols, amin, amax, avoid=None):
    for _ in range(n):
        while True:
            x = rng.uniform(region[0], region[2])
            y = rng.uniform(region[1], region[3])
            if avoid is None or avoid(x, y):
                break
        r = rng.uniform(rmin, rmax)
        a = rng.uniform(amin, amax)
        col = cols[rng.integers(len(cols))]
        c.shape(lambda d, x=x, y=y, r=r: ellipse(d, x, y, r), col, a)


def cape_detail(kind, c, rng):
    y, x = yy_xx()
    if kind == "aoko-glass":
        # frosted streaks
        for x0, w, a in ((3, 1.6, 0.05), (9, 0.8, 0.04), (14.5, 2.2, 0.045)):
            c.shape(lambda d, x0=x0, w=w: d.polygon(P((x0 + 6, 14), (x0 + 6 + w, 14), (x0 + w - 3, 30), (x0 - 3, 30)), fill=255),
                    C("#bfe9ff"), a, 0.25)
        # inner hairline
        c.shape(lambda d: d.rectangle([2 * SS, 2 * SS, (FW - 2) * SS - 1, (FH - 2) * SS - 1], outline=255, width=max(1, SS // 3)),
                C("#38bdf8"), 0.22)
        # sigil: diamond, inner diamond, dot, ticks
        cx, cy = 10, 8
        sig = C("#cfeeff")
        for rr, wd in ((4.0, 0.55), (2.1, 0.4)):
            pts = P((cx, cy - rr), (cx + rr, cy), (cx, cy + rr), (cx - rr, cy))
            c.glow(lambda d, pts=pts: d.polygon(pts, outline=255, width=int(wd * SS * 2.2)), C("#4cc3ff"), 0.55, 0.9)
            c.shape(lambda d, pts=pts, wd=wd: d.polygon(pts, outline=255, width=max(1, int(wd * SS))), sig, 0.95)
        c.glow(lambda d: ellipse(d, cx, cy, 1.0), C("#6fd0ff"), 0.8, 0.8)
        c.shape(lambda d: ellipse(d, cx, cy, 0.7), C("#ffffff"), 1.0)
        for dy in (-5.8, 5.8):
            c.shape(lambda d, dy=dy: d.rectangle([(cx - 0.25) * SS, (cy + dy - 0.7) * SS, (cx + 0.25) * SS, (cy + dy + 0.7) * SS], fill=255), sig, 0.8)
        for dx in (-5.8, 5.8):
            c.shape(lambda d, dx=dx: d.rectangle([(cx + dx - 0.7) * SS, (cy - 0.25) * SS, (cx + dx + 0.7) * SS, (cy + 0.25) * SS], fill=255), sig, 0.8)
    elif kind == "tsukihime-moon":
        mx, my, R = 9.4, 11.0, 6.4
        ox, oy, r2 = 2.9, -1.3, 5.6
        disc = mask(c.w, c.h, lambda d: ellipse(d, mx, my, R))
        cut = mask(c.w, c.h, lambda d: ellipse(d, mx + ox, my + oy, r2))
        cres = np.clip(disc - cut, 0, 1)
        sparkle(c, rng, 30, (1.5, 1.5, 18.5, 24), 0.12, 0.34, [C("#dfe9ff"), C("#bcd4ff"), C("#ffffff")], 0.4, 1.0,
                avoid=lambda x, y: (x - mx) ** 2 + (y - my) ** 2 > (R + 1.8) ** 2)
        for gx, gy in ((16.2, 4.6), (3.4, 21.0), (17.0, 17.0)):
            c.shape(lambda d, gx=gx, gy=gy: (d.rectangle([(gx - 1.3) * SS, (gy - 0.07) * SS, (gx + 1.3) * SS, (gy + 0.07) * SS], fill=255),
                                              d.rectangle([(gx - 0.07) * SS, (gy - 1.3) * SS, (gx + 0.07) * SS, (gy + 1.3) * SS], fill=255)),
                    C("#e8f0ff"), 0.8, 0.12)
        c.over(disc * (1 - cut), C("#aab8ff"), 0.07)  # earthshine on the dark side
        gl = np.asarray(Image.fromarray((cres * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(2.4 * SS))) / 255.0
        c.add(gl, C("#7fa6ff"), 0.9)
        shade = np.clip(0.88 + 0.12 * ((mx - x) / R), 0.7, 1.05)
        c.over(cres, C("#eef4ff")[None, None, :] * shade[..., None], 1.0)
    elif kind == "sakura-drift":
        c.add(np.exp(-(((x - 6) / 7) ** 2 + ((y - 5) / 7) ** 2)), C("#ffe6f0"), 0.12)
        cols = [C("#ffe6f0"), C("#ffd1e3"), C("#ffc0d8"), C("#fff2f7")]
        for i in range(17):
            px, py = rng.uniform(1.6, 18.4), rng.uniform(1.6, 24.6)
            s = rng.uniform(1.5, 2.7)
            ang = rng.uniform(0, 2 * math.pi)
            th = np.linspace(0, 2 * math.pi, 40, endpoint=False)
            rr = 1 - 0.28 * np.exp(-(np.minimum(th, 2 * math.pi - th) / 0.35) ** 2)
            lx = 0.5 * s * np.cos(th) * rr
            ly = 0.34 * s * np.sin(th) * (1 + 0.3 * np.cos(th)) * rr
            ca, sa = math.cos(ang), math.sin(ang)
            pts = P(*[(px + a * ca - b * sa, py + a * sa + b * ca) for a, b in zip(lx, ly)])
            far = i % 4 == 0
            c.shape(lambda d, pts=pts: d.polygon([(p[0] + 0.4 * SS, p[1] + 0.5 * SS) for p in pts], fill=255), C("#7a3f7a"), 0.16, 0.3)
            c.shape(lambda d, pts=pts: d.polygon(pts, fill=255), cols[i % 4], 0.6 if far else 0.94, 0.35 if far else 0.0)
    elif kind == "void-star":
        e = np.minimum.reduce([x, FW - x, y, FH - y])
        c.add(np.exp(-e / 2.0) * 0.42, C("#6f5fd8"), 1.0)
        sparkle(c, rng, 60, (1.5, 1.5, 18.5, 30.5), 0.12, 0.3, [C("#cfd6ff"), C("#e9e4ff"), C("#a9b4ff")], 0.2, 0.7)
        sx, sy = 10, 12
        c.glow(lambda d: ellipse(d, sx, sy, 1.6), C("#8f7dff"), 0.9, 2.4)
        c.glow(lambda d: ellipse(d, sx, sy, 0.9), C("#c9bfff"), 0.7, 1.0)
        for L, W, a in ((7.0, 0.45, 0.95), (5.0, 0.4, 0.95)):
            c.shape(lambda d, L=L, W=W: (d.polygon(P((sx, sy - L), (sx + W, sy), (sx, sy + L), (sx - W, sy)), fill=255),), C("#f1eeff"), a, 0.1)
        c.shape(lambda d: d.polygon(P((sx - 5.0, sy), (sx, sy + 0.4), (sx + 5.0, sy), (sx, sy - 0.4)), fill=255), C("#f1eeff"), 0.95, 0.1)
        for dx, dy in ((1, 1), (1, -1), (-1, 1), (-1, -1)):
            c.shape(lambda d, dx=dx, dy=dy: d.line(P((sx, sy), (sx + dx * 2.0, sy + dy * 2.0)), fill=255, width=max(1, SS // 4)), C("#b9adff"), 0.45, 0.1)
        c.shape(lambda d: ellipse(d, sx, sy, 0.75), C("#ffffff"), 1.0)
    elif kind == "ember-crest":
        gold_hi, gold_lo = C("#ffe08a"), C("#b8862e")
        cols = [C("#ff7a2a"), C("#ffb347"), C("#ff5a1f")]
        for _ in range(34):
            ey = 32 * rng.random() ** 0.55
            ex = rng.uniform(1.5, 18.5)
            r = rng.uniform(0.2, 0.46)
            col = cols[rng.integers(3)]
            a = rng.uniform(0.55, 1.0)
            c.glow(lambda d, ex=ex, ey=ey, r=r: ellipse(d, ex, ey, r * 2.2), col, 0.45 * a, 0.6)
            c.shape(lambda d, ex=ex, ey=ey, r=r: ellipse(d, ex, ey, r), col, a)
        shield = [(5, 8), (10, 7.0), (15, 8), (15, 16), (13.6, 19.8), (10, 24), (6.4, 19.8), (5, 16)]
        c.shape(lambda d: d.polygon(P(*shield), fill=255), C("#14070a"), 0.6)
        grad = np.clip(((y - 7) / 17.0), 0, 1)[..., None]
        gold = gold_hi * (1 - grad) + gold_lo * grad
        outl = mask(c.w, c.h, lambda d: d.polygon(P(*shield), outline=255, width=max(2, int(0.62 * SS))))
        c.over(outl, gold, 1.0)
        inner = [(10 + (px - 10) * 0.8, 15.5 + (py - 15.5) * 0.8) for px, py in shield]
        c.over(mask(c.w, c.h, lambda d: d.polygon(P(*inner), outline=255, width=max(1, int(0.3 * SS)))), gold, 0.55)
        flame = [(10, 10), (11.3, 12.2), (12.2, 14.2), (12.0, 16.4), (10, 18.6), (8.0, 16.4), (7.8, 14.2), (8.8, 12.2)]
        c.glow(lambda d: d.polygon(P(*flame), fill=255), C("#ff8a2a"), 0.7, 1.2)
        c.shape(lambda d: d.polygon(P(*flame), fill=255), gold, 0.95, 0.12)
        fi = [(10, 13.2), (11.0, 15.0), (10.6, 16.8), (10, 17.6), (9.4, 16.8), (9.0, 15.0)]
        c.shape(lambda d: d.polygon(P(*fi), fill=255), C("#ff4a1a"), 0.95, 0.1)
    return c


def cape_rim(kind, base_rows, top_row, bottom_row):
    """Border colours (face-edge ring and the edge strips share them)."""
    if kind == "aoko-glass":
        cy = C("#38bdf8")
        return base_rows * 0.4 + cy * 0.6, top_row * 0.15 + C("#8fe0ff") * 0.85, bottom_row * 0.5 + cy * 0.5
    if kind == "tsukihime-moon":
        return base_rows * 0.7 + C("#7f9eff") * 0.3, top_row * 0.7 + C("#7f9eff") * 0.3, bottom_row * 0.6 + C("#38c4e0") * 0.4
    if kind == "sakura-drift":
        return base_rows * 0.85, top_row * 0.8 + C("#ffe6f0") * 0.2, bottom_row * 0.7 + C("#a479b5") * 0.3
    if kind == "void-star":
        v = C("#7a68e8")
        return base_rows * 0.3 + v * 0.7, top_row * 0.3 + v * 0.7, bottom_row * 0.3 + v * 0.7
    if kind == "ember-crest":
        g = C("#8a6a2c")
        return base_rows * 0.55 + g * 0.45, top_row * 0.55 + g * 0.45, bottom_row * 0.55 + g * 0.45
    raise KeyError(kind)


CAPE_IDS = ["aoko-glass", "tsukihime-moon", "sakura-drift", "void-star", "ember-crest"]


def render_cape(kind, seed):
    rng = np.random.default_rng(seed)
    out_c = cape_detail(kind, Canvas(cape_base(kind)), rng)
    inn_c = Canvas(cape_base(kind))
    res = []
    for cv, k in ((out_c, 1.0), (inn_c, 0.55)):
        face = cv.down()
        rows = face[:, :1, :].mean(axis=1) * 0 + vgrad_rows(kind)
        side, top, bottom = cape_rim(kind, rows, rows[0], rows[-1])
        face = face.copy()
        face[:, :2] = side[:, None, :]
        face[:, -2:] = side[:, None, :]
        face[:2, :] = top
        face[-2:, :] = bottom
        res.append((face * k, side * k, top * k, bottom * k))
    return res


def vgrad_rows(kind):
    b = cape_base(kind).reshape(FH, SS, FW * SS, 3).mean(axis=(1, 2))
    return b


def build_cape(kind, seed):
    (face, side, top, bottom), (inner, iside, itop, ibottom) = render_cape(kind, seed)
    rng = np.random.default_rng(seed + 1000)
    img = np.zeros((64, 128, 4), np.float64)

    def put(x, y, arr):
        img[y:y + arr.shape[0], x:x + arr.shape[1], :3] = arr
        img[y:y + arr.shape[0], x:x + arr.shape[1], 3] = 1.0

    put(2, 2, face)                                       # outer face
    put(24, 2, inner)                                     # inner face
    put(0, 2, np.repeat(side[:, None, :], 2, axis=1))     # left edge
    put(22, 2, np.repeat(side[:, None, :], 2, axis=1))    # right edge
    put(2, 0, np.repeat(top[None, None, :], 2, 0).repeat(20, 1))      # top edge
    put(22, 0, np.repeat(bottom[None, None, :], 2, 0).repeat(20, 1))  # bottom edge
    px = img.copy()
    px[..., :3] = np.floor(np.clip(img[..., :3], 0, 1) * 255 + rng.random(img[..., :3].shape))
    px[..., :3] = np.clip(px[..., :3], 0, 255)
    px[..., 3] = img[..., 3] * 255
    px[img[..., 3] == 0, :3] = 0
    return png_bytes(px.astype(np.uint8))


def build_capes(files):
    for i, kind in enumerate(CAPE_IDS):
        assert ID_RE.match(kind)
        files[f"capes/{kind}.png"] = build_cape(kind, 100 + i)


# ----------------------------------------------------------------- skies ----
N = 512  # face size
FACES = ["+X", "-X", "+Y", "-Y", "+Z", "-Z"]
ATLAS = {"+X": (0, 0), "-X": (1, 0), "+Y": (2, 0), "-Y": (0, 1), "+Z": (1, 1), "-Z": (2, 1)}
# normal, sc axis, tc axis: the spec's OpenGL cubemap table (sc = +-x/y/z, tc likewise)
BASIS = {
    "+X": ((1, 0, 0), (0, 0, -1), (0, -1, 0)),
    "-X": ((-1, 0, 0), (0, 0, 1), (0, -1, 0)),
    "+Y": ((0, 1, 0), (1, 0, 0), (0, 0, 1)),
    "-Y": ((0, -1, 0), (1, 0, 0), (0, 0, -1)),
    "+Z": ((0, 0, 1), (1, 0, 0), (0, -1, 0)),
    "-Z": ((0, 0, -1), (-1, 0, 0), (0, -1, 0)),
}


def face_dirs(face, n=N):
    """Unit direction for every pixel centre of a face: inverts the sc/tc table."""
    nrm, scv, tcv = (np.array(v, float) for v in BASIS[face])
    s = 2 * (np.arange(n) + 0.5) / n - 1
    sc, tc = np.meshgrid(s, s)  # [row(v), col(u)]
    d = nrm + sc[..., None] * scv + tc[..., None] * tcv
    return d / np.linalg.norm(d, axis=-1, keepdims=True)


def dir_to_face(d):
    """Forward table: direction -> (face index into FACES, u, v) in 0..1."""
    d = np.asarray(d, float)
    ad = np.abs(d)
    ax = np.argmax(ad, axis=-1)
    idx = np.zeros(d.shape[:-1], int)
    u = np.zeros(d.shape[:-1])
    v = np.zeros(d.shape[:-1])
    for fi, f in enumerate(FACES):
        nrm, scv, tcv = (np.array(q, float) for q in BASIS[f])
        ma = d @ nrm
        sel = (ax == int(np.argmax(np.abs(nrm)))) & (ma > 0)
        with np.errstate(all="ignore"):
            uu = ((d @ scv) / ma + 1) / 2
            vv = ((d @ tcv) / ma + 1) / 2
        idx[sel], u[sel], v[sel] = fi, uu[sel], vv[sel]
    return idx, u, v


def sample_atlas(atlas, d):
    """Nearest sample of a (2N x 3N x 3) atlas by direction."""
    n = atlas.shape[0] // 2
    idx, u, v = dir_to_face(d)
    out = np.zeros(d.shape[:-1] + (3,))
    px = np.clip((u * n).astype(int), 0, n - 1)
    py = np.clip((v * n).astype(int), 0, n - 1)
    for fi, f in enumerate(FACES):
        sel = idx == fi
        c, r = ATLAS[f]
        out[sel] = atlas[r * n + py[sel], c * n + px[sel]]
    return out


class Perlin:
    def __init__(self, seed):
        rng = np.random.default_rng(seed)
        self.perm = rng.permutation(256)
        g = rng.normal(size=(256, 3))
        self.grad = g / np.linalg.norm(g, axis=1, keepdims=True)
        q, _ = np.linalg.qr(rng.normal(size=(3, 3)))
        self.rot = q

    def __call__(self, p):
        p = np.asarray(p, float)
        pf = np.floor(p)
        f = p - pf
        pi = pf.astype(np.int64) & 255
        w = f * f * f * (f * (f * 6 - 15) + 10)
        perm, grad = self.perm, self.grad
        fx, fy, fz = f[..., 0], f[..., 1], f[..., 2]
        x0, y0, z0 = pi[..., 0], pi[..., 1], pi[..., 2]
        x1, y1, z1 = (x0 + 1) & 255, (y0 + 1) & 255, (z0 + 1) & 255

        def corner(xi, yi, zi, dx, dy, dz):
            g = grad[perm[(perm[(perm[xi] + yi) & 255] + zi) & 255]]
            return g[..., 0] * (fx - dx) + g[..., 1] * (fy - dy) + g[..., 2] * (fz - dz)

        def lerp(a, b, t):
            return a + (b - a) * t

        wx, wy, wz = w[..., 0], w[..., 1], w[..., 2]
        a = lerp(corner(x0, y0, z0, 0, 0, 0), corner(x1, y0, z0, 1, 0, 0), wx)
        b = lerp(corner(x0, y1, z0, 0, 1, 0), corner(x1, y1, z0, 1, 1, 0), wx)
        c = lerp(corner(x0, y0, z1, 0, 0, 1), corner(x1, y0, z1, 1, 0, 1), wx)
        d = lerp(corner(x0, y1, z1, 0, 1, 1), corner(x1, y1, z1, 1, 1, 1), wx)
        return lerp(lerp(a, b, wy), lerp(c, d, wy), wz)

    def fbm(self, p, octaves=5, lac=2.0, gain=0.5):
        """Roughly in [0, 1], centred on 0.5 (value of 1.4x the raw sum)."""
        tot, amp, norm = 0.0, 1.0, 0.0
        p = np.asarray(p, float)
        for o in range(octaves):
            tot = tot + amp * self(p + 11.7 * o)
            norm += amp
            p = (p @ self.rot) * lac
            amp *= gain
        return np.clip(0.5 + 1.4 * tot / norm, 0, 1)


def angle_between(d, m):
    return 2 * np.arcsin(np.clip(np.linalg.norm(d - m, axis=-1) / 2, 0, 1))


def unit(*v):
    v = np.array(v, float)
    return v / np.linalg.norm(v)


STAR_TINTS = [np.array(c) for c in ((0.78, 0.86, 1.0), (0.92, 0.95, 1.0), (1.0, 0.97, 0.92), (1.0, 0.88, 0.72))]


def gen_stars(seed, groups):
    """groups: list of dicts(n, amp=(lo,hi,power), sigma_deg=(lo,hi), spikes=bool, spike_deg)."""
    rng = np.random.default_rng(seed)
    stars = []
    for g in groups:
        for _ in range(g["n"]):
            z = rng.uniform(-1, 1)
            ph = rng.uniform(0, 2 * math.pi)
            r = math.sqrt(1 - z * z)
            d = np.array([r * math.cos(ph), z, r * math.sin(ph)])
            lo, hi, pw = g["amp"]
            amp = lo + (hi - lo) * rng.random() ** pw
            sig = math.radians(rng.uniform(*g["sigma_deg"]))
            tint = STAR_TINTS[rng.choice(4, p=[0.35, 0.35, 0.2, 0.1])]
            stars.append((d, amp, sig, tint, g.get("spikes", False), math.radians(g.get("spike_deg", 0)), rng.uniform(0, math.pi)))
    return stars


def splat_stars(face, stars, n=N):
    layer = np.zeros((n, n, 3))
    D = face_dirs(face, n)
    nrm, scv, tcv = (np.array(q, float) for q in BASIS[face])
    for s, amp, sig, tint, spikes, L, psi in stars:
        ma = s @ nrm
        if ma < 0.35:
            continue
        sc, tc = (s @ scv) / ma, (s @ tcv) / ma
        if abs(sc) > 1.8 or abs(tc) > 1.8:
            continue
        ext = max(4 * sig, 3 * L)
        r = int(math.ceil(ext / ((2 / n) / (1 + sc * sc + tc * tc)))) + 1
        cx, cy = int((sc + 1) / 2 * n), int((tc + 1) / 2 * n)
        x0, x1, y0, y1 = max(0, cx - r), min(n, cx + r + 1), max(0, cy - r), min(n, cy + r + 1)
        if x0 >= x1 or y0 >= y1:
            continue
        delta = D[y0:y1, x0:x1] - s
        c2 = (delta ** 2).sum(-1)
        val = amp * np.exp(-c2 / (2 * sig * sig))
        if spikes:
            ref = np.array([0.0, 1.0, 0.0]) if abs(s[1]) < 0.9 else np.array([1.0, 0.0, 0.0])
            t1 = np.cross(s, ref)
            t1 /= np.linalg.norm(t1)
            t2 = np.cross(s, t1)
            e1 = math.cos(psi) * t1 + math.sin(psi) * t2
            e2 = -math.sin(psi) * t1 + math.cos(psi) * t2
            a, b = delta @ e1, delta @ e2
            w = max(sig * 0.9, 0.0022)
            val = val + amp * 0.8 * (np.exp(-np.abs(a) / L) * np.exp(-b * b / (2 * w * w)) +
                                     np.exp(-np.abs(b) / L) * np.exp(-a * a / (2 * w * w)))
            val = val + amp * 0.3 * np.exp(-np.sqrt(c2) / (sig * 6))
        layer[y0:y1, x0:x1] += val[..., None] * tint
    return layer


def render_atlas(bg_fn, stars, seed_dither):
    """bg_fn(D) -> (rgb float image, star visibility). Returns float atlas (0..1)."""
    atlas = np.zeros((2 * N, 3 * N, 3))
    for f in FACES:
        D = face_dirs(f)
        img, vis = bg_fn(D)
        img = img + splat_stars(f, stars) * vis[..., None]
        c, r = ATLAS[f]
        atlas[r * N:(r + 1) * N, c * N:(c + 1) * N] = img
    return np.clip(atlas, 0, 1)


def encode_atlas(atlas, seed):
    rng = np.random.default_rng(seed)
    noise = rng.random(atlas.shape) - rng.random(atlas.shape)  # triangular, +-1 LSB
    a = np.clip(np.floor(atlas * 255 + 0.5 + 1.1 * noise), 0, 255).astype(np.uint8)
    buf = io.BytesIO()
    Image.fromarray(a).save(buf, "JPEG", quality=88, subsampling=0, optimize=True)
    data = buf.getvalue()
    if len(data) > 600_000:
        buf = io.BytesIO()
        Image.fromarray(a).save(buf, "JPEG", quality=88, subsampling=2, optimize=True)
        data = buf.getvalue()
    return data


def edge_pixels(face, side, n):
    """(row, col) index arrays of the edge pixels and their outside-extended sc/tc."""
    i = np.arange(n)
    s = 2 * (i + 0.5) / n - 1
    ext = 1 + 1.0 / n
    if side == "L":
        return (i, np.zeros(n, int)), (np.full(n, -ext), s)
    if side == "R":
        return (i, np.full(n, n - 1)), (np.full(n, ext), s)
    if side == "T":
        return (np.zeros(n, int), i), (s, np.full(n, -ext))
    return (np.full(n, n - 1), i), (s, np.full(n, ext))


def seam_stats(atlas255):
    """Mean/p99 absolute difference across each of the 12 cube edges, plus interior reference."""
    n = atlas255.shape[0] // 2
    tiles = {f: atlas255[ATLAS[f][1] * n:(ATLAS[f][1] + 1) * n, ATLAS[f][0] * n:(ATLAS[f][0] + 1) * n] for f in FACES}
    edges = {}
    for f in FACES:
        nrm, scv, tcv = (np.array(q, float) for q in BASIS[f])
        for side in "LRTB":
            (r, c), (sc, tc) = edge_pixels(f, side, n)
            a = tiles[f][r, c]
            d = nrm + sc[:, None] * scv + tc[:, None] * tcv
            fi, u, v = dir_to_face(d)
            b = np.zeros_like(a)
            g = np.zeros(n, int)
            for k in range(n):
                f2 = FACES[fi[k]]
                g[k] = fi[k]
                b[k] = tiles[f2][min(int(v[k] * n), n - 1), min(int(u[k] * n), n - 1)]
            for k in set(g.tolist()):
                assert FACES[k] != f
            key = frozenset([f, FACES[g[0]]])
            assert len(set(g.tolist())) == 1, "edge maps to several faces"
            dr, dc = {"L": (0, 1), "R": (0, -1), "T": (1, 0), "B": (-1, 0)}[side]
            a2 = tiles[f][r + dr, c + dc]
            edges.setdefault(key, []).append((np.abs(a - b).mean(-1), np.abs(a - a2).mean(-1)))
    assert len(edges) == 12, len(edges)
    out = {}
    for key, lst in edges.items():
        diffs = np.concatenate([x[0] for x in lst])
        ref = np.concatenate([x[1] for x in lst])
        out["/".join(sorted(key))] = (float(diffs.mean()), float(np.percentile(diffs, 99)), float(ref.mean()), float(np.percentile(ref, 99)))
    inter = np.concatenate([np.abs(t[:, 1:] - t[:, :-1]).mean(-1).ravel() for t in tiles.values()])
    return out, (float(inter.mean()), float(np.percentile(inter, 99)))


def roundtrip_check():
    for f in FACES:
        idx, u, v = dir_to_face(face_dirs(f, 64))
        s = (np.arange(64) + 0.5) / 64
        assert (idx == FACES.index(f)).all(), f
        assert np.allclose(u, s[None, :], atol=1e-9) and np.allclose(v, s[:, None], atol=1e-9), f


NOISE = Perlin(7)
TAU = 2 * math.pi


def elev_deg(D):
    return np.degrees(np.arcsin(np.clip(D[..., 1], -1, 1)))


def moon_layer(D, m, r_deg, col, shade_noise=True, maria=0.5, glow=0.45, glow_scale=2.2, wide=0.18):
    """Returns (disc colour, disc mask, glow value 0..1). Depends only on D."""
    ang = np.degrees(angle_between(D, m))
    r = r_deg
    disc = 1 - smoothstep(r - 0.12, r + 0.12, ang)
    c = np.broadcast_to(col, D.shape).copy()
    if shade_noise:
        mar = NOISE.fbm(D * 7.0 + 3.0, 5)
        f = smoothstep(0.46, 0.66, mar) * maria
        grain = NOISE.fbm(D * 38.0, 3) - 0.5
        c = c * (1 - f[..., None]) + np.array([0.56, 0.66, 0.82]) * f[..., None]
        c = c * (1 + 0.10 * grain[..., None])
    limb = 1 - 0.20 * np.clip(ang / r, 0, 1) ** 3
    c = c * limb[..., None]
    gl = glow * np.exp(-np.maximum(ang - r, 0) / glow_scale) + wide * np.exp(-ang / 22.0)
    return c, disc, gl


def sky_tsukihime(D):
    e = elev_deg(D)
    base = ramp(e, [(-90, "#03050f"), (-20, "#050a1e"), (-3, "#0a1a38"), (0, "#14305a"), (8, "#101f4a"),
                    (25, "#0c1440"), (55, "#090d2e"), (90, "#06082a")])
    haze = 0.34 * np.exp(-(((np.abs(e) - 1.5) / 3.6) ** 2)) + 0.17 * np.exp(-np.abs(e) / 13.0)
    haze = haze * np.where(e < 0, 0.45, 1.0)
    img = base + haze[..., None] * np.array([0.50, 0.82, 1.0]) * 0.75
    cl = smoothstep(0.45, 0.8, NOISE.fbm(D * 2.2 + 5, 4))
    img = img + 0.05 * cl[..., None] * np.array([0.25, 0.30, 0.9]) * smoothstep(-0.1, 0.3, D[..., 1])[..., None]
    m = unit(0.45, 0.42, -0.78)
    mc, disc, gl = moon_layer(D, m, 15.0, np.array([0.93, 0.96, 1.0]), maria=0.42)
    img = img + gl[..., None] * np.array([0.50, 0.68, 1.0]) * 0.8
    img = img * (1 - disc[..., None]) + mc * disc[..., None]
    vis = smoothstep(-0.02, 0.14, D[..., 1]) * (1 - np.clip(haze * 1.2, 0, 1)) * (1 - disc) * (1 - np.clip(gl * 1.1, 0, 0.85))
    return img, vis


def sky_aurora(D):
    e = elev_deg(D)
    base = ramp(e, [(-90, "#010204"), (-5, "#040810"), (0, "#0b1626"), (15, "#071026"), (45, "#040a1c"), (90, "#02040f")])
    a = np.arctan2(D[..., 0], -D[..., 2])
    er = np.arcsin(np.clip(D[..., 1], -1, 1))
    img = base.copy()
    inten = np.zeros(e.shape)
    rays_n = NOISE.fbm(np.stack([D[..., 0] * 16, D[..., 1] * 0.5, D[..., 2] * 16], -1), 3)
    rays = 0.45 + 0.55 * smoothstep(0.25, 0.75, rays_n)
    pole = 1 - smoothstep(0.86, 0.97, D[..., 1])
    ribbons = [  # e0 deg, (A1 deg, k1, ph1), (A2 deg, k2, ph2), height rad, strength, colour set
        (30, (10, 2, 0.4), (4, 5, 1.7), 0.30, 0.95, ("#7dffc4", "#2fd9a0", "#4aa0ff", "#8d5cff")),
        (44, (8, 3, 2.4), (3, 6, 0.3), 0.26, 0.70, ("#6fffb0", "#35d6b4", "#5a8cff", "#a060ff")),
        (17, (6, 2, 4.0), (3, 4, 2.2), 0.22, 0.55, ("#8affd0", "#3de0b0", "#58a0ff", "#9a70ff")),
    ]
    for j, (e0, (A1, k1, p1), (A2, k2, p2), h, strength, cs) in enumerate(ribbons):
        warp = (NOISE.fbm(D * 1.6 + 20 * j, 2) - 0.5) * 0.16
        ec = math.radians(e0) + math.radians(A1) * np.sin(k1 * a + p1) + math.radians(A2) * np.sin(k2 * a + p2) + warp
        t = (er - ec) / h
        shape = np.where(t < 0, np.exp(-(t / 0.12) ** 2), np.exp(-np.clip(t, 0, None) * 1.7))
        bm = 0.62 + 0.38 * NOISE.fbm(D * 1.4 + 40 * j, 2)
        col = ramp(np.clip(t, 0, 1.2), [(0, cs[0]), (0.2, cs[1]), (0.6, cs[2]), (1.1, cs[3])])
        w = (shape * rays * bm * strength * pole)
        img = img + col * w[..., None] * 0.95
        inten = inten + w
    img = img + 0.05 * np.exp(-np.abs(e) / 7.0)[..., None] * np.array([0.1, 0.8, 0.5]) * np.clip(inten.mean(), 0, 1) * 0 
    vis = smoothstep(-0.02, 0.12, D[..., 1]) * (1 - np.clip(inten * 0.8, 0, 0.9))
    return img, vis


def sky_sakura(D):
    e = elev_deg(D)
    img = ramp(e, [(-90, "#1a1030"), (-15, "#3d2150"), (-2, "#a35a72"), (0, "#ff9f5c"), (5, "#ff9a70"), (14, "#f58aa0"),
                   (28, "#c273a8"), (48, "#7c58b0"), (70, "#4a3a8f"), (90, "#2c2468")])
    s = unit(0.55, -0.05, 0.83)
    ang = np.degrees(angle_between(D, s))
    glow = 0.40 * np.exp(-ang / 20.0) + 0.22 * np.exp(-ang / 6.0)
    img = img + glow[..., None] * np.array([1.0, 0.60, 0.28])
    m = unit(-0.45, 0.50, -0.74)
    mc, disc, gl = moon_layer(D, m, 2.4, np.array([0.99, 0.96, 0.92]), maria=0.35, glow=0.30, glow_scale=1.6, wide=0.0)
    img = img + gl[..., None] * np.array([1.0, 0.85, 0.85]) * 0.6
    img = img * (1 - disc[..., None]) + mc * disc[..., None]
    p = np.stack([D[..., 0] * 2.0, D[..., 1] * 7.0, D[..., 2] * 2.0], -1)
    warp = NOISE.fbm(D * 3.0 + 9, 3) - 0.5
    p = p + np.stack([warp, 0 * warp, warp], -1) * 1.6
    c0 = NOISE.fbm(p, 5)
    c1 = NOISE.fbm(p + np.array([0.0, 0.30, 0.0]), 5)
    env = smoothstep(0.012, 0.06, D[..., 1]) * (1 - smoothstep(0.26, 0.5, D[..., 1]))
    dens = smoothstep(0.47, 0.68, c0) * env
    shade = np.clip(0.9 + 2.0 * (c0 - c1), 0.7, 1.15)
    ccol = ramp(e, [(0, "#ffc08a"), (8, "#ff9fa8"), (20, "#e08ab8"), (40, "#a07ac0")]) * shade[..., None]
    img = img * (1 - 0.85 * dens[..., None]) + ccol * 0.85 * dens[..., None]
    vis = smoothstep(0.50, 0.95, D[..., 1]) * 0.8 * (1 - dens) * (1 - disc) * (1 - np.clip(gl, 0, 1))
    return img, vis


def sky_nebula(D):
    img = ramp(D[..., 1], [(-1, "#03030c"), (0, "#05051a"), (1, "#04041a")])
    wv = np.stack([NOISE.fbm(D * 1.3 + o, 4) - 0.5 for o in (3.0, 17.0, 31.0)], -1)
    dens = NOISE.fbm(D * 1.8 + 1.5 * wv, 6)
    d1 = smoothstep(0.42, 0.88, dens) ** 1.6
    hm = smoothstep(0.35, 0.65, NOISE.fbm(D * 1.1 + 7, 3))[..., None]
    col = np.array([0.10, 0.22, 0.62]) * (1 - hm) + np.array([0.45, 0.15, 0.70]) * hm
    dust = smoothstep(0.55, 0.75, NOISE.fbm(D * 3.2 + wv, 4))
    neb = col * (d1 * (1 - 0.55 * dust) * 1.15)[..., None]
    core = smoothstep(0.70, 1.0, dens)
    neb = neb + core[..., None] * np.array([0.85, 0.65, 1.0]) * 0.28
    ridge = (1 - np.abs(NOISE.fbm(D * 4.5 + wv, 4) * 2 - 1)) ** 5
    neb = neb + (ridge * d1 * 0.35)[..., None] * np.array([0.30, 0.50, 1.0])
    img = img + neb * 0.9
    vis = 1 - 0.3 * d1
    return img, vis


SKIES = {
    "tsukihime": dict(fn=sky_tsukihime, name="Tsukihime", time=18000, seed=11,
                      stars=[dict(n=7000, amp=(0.25, 0.9, 2.2), sigma_deg=(0.13, 0.20)),
                             dict(n=120, amp=(0.7, 1.0, 1), sigma_deg=(0.22, 0.34))],
                      colors_extra=dict(sunset="#6fd4f2", cloud_mix=0.5), rotation=None, clear=True),
    "aurora": dict(fn=sky_aurora, name="Aurora", time=18000, seed=12,
                   stars=[dict(n=4500, amp=(0.2, 0.9, 2.4), sigma_deg=(0.13, 0.20)),
                          dict(n=60, amp=(0.7, 1.0, 1), sigma_deg=(0.22, 0.34))],
                   colors_extra=dict(sunset="#3fbf9a", cloud_mix=0.5), rotation=[0.0, 0.3, 0.0], clear=False),
    "sakura-dusk": dict(fn=sky_sakura, name="Sakura Dusk", time=12800, seed=13,
                        stars=[dict(n=1200, amp=(0.3, 1.0, 1.8), sigma_deg=(0.13, 0.20))],
                        colors_extra=dict(sunset="#ff8f6b", cloud_mix=0.2), rotation=None, clear=False),
    "nebula": dict(fn=sky_nebula, name="Nebula", time=18000, seed=14,
                   stars=[dict(n=14000, amp=(0.10, 0.75, 3.0), sigma_deg=(0.13, 0.20)),
                          dict(n=700, amp=(0.5, 1.0, 1.5), sigma_deg=(0.2, 0.32)),
                          dict(n=34, amp=(0.9, 1.0, 1), sigma_deg=(0.30, 0.45), spikes=True, spike_deg=2.6)],
                   colors_extra=dict(sunset="#7a4cff", cloud_mix=0.5), rotation=[0.0, 0.3, 0.0], clear=False),
}
COLOR_SKIES = {
    "glass": dict(name="Glass",
                  colors={"day": {"sky": "#8fd3f4", "fog": "#d3f1ff", "cloud": "#ffffff", "sunset": "#f08fb4"},
                          "night": {"sky": "#050f26", "fog": "#0b1a38", "cloud": "#16284a", "sunset": "#58c8e8"}}),
    "ember": dict(name="Ember",
                  colors={"day": {"sky": "#ffa24a", "fog": "#ffc27a", "cloud": "#ffe0b0", "sunset": "#ff6a1f"},
                          "night": {"sky": "#2a060c", "fog": "#3a0c12", "cloud": "#4a1a1c", "sunset": "#ff4a12"}}),
}
# Seam test on the decoded JPEG, per cube edge: compare |pixel - pixel across the edge| with
# |pixel - next pixel inward| on the same edge strip (both are ~1 px apart in angle).
SEAM_MEAN_RATIO = 1.5   # seam mean <= ratio * inner mean + SEAM_MEAN_SLACK
SEAM_MEAN_SLACK = 0.8   # 0..255 levels, absorbs JPEG/dither noise
SEAM_P99_RATIO = 1.6    # seam p99 <= ratio * inner p99 + SEAM_P99_SLACK
SEAM_P99_SLACK = 6.0


def band_median(atlas, lo, hi, n=4000):
    rng = np.random.default_rng(5)
    a = rng.uniform(0, TAU, n)
    y = rng.uniform(lo, hi, n)
    r = np.sqrt(1 - y * y)
    return np.median(sample_atlas(atlas, np.stack([r * np.cos(a), y, r * np.sin(a)], -1)), axis=0)


def lum(c):
    return c @ np.array([0.2126, 0.7152, 0.0722])


def verify_sky(sid, atlas255, report):
    atlas = atlas255 / 255.0
    stats, inter = seam_stats(atlas255)
    worst = 0.0
    for k, (m, p, im, ip) in stats.items():
        worst = max(worst, m / (im + SEAM_MEAN_SLACK))
        assert m <= SEAM_MEAN_RATIO * im + SEAM_MEAN_SLACK, (sid, k, m, im)
        assert p <= SEAM_P99_RATIO * ip + SEAM_P99_SLACK, (sid, k, p, ip)
    mx = max(stats.items(), key=lambda kv: kv[1][0] - kv[1][2])
    report.append(f"  {sid}: seam mean/inner mean worst edge {mx[0]}: {mx[1][0]:.2f} vs {mx[1][2]:.2f}; "
                  f"max seam mean {max(v[0] for v in stats.values()):.2f}, max seam p99 {max(v[1] for v in stats.values()):.1f}")
    if sid in ("sakura-dusk", "tsukihime"):
        hor = band_median(atlas, -0.04, 0.06)
        zen = band_median(atlas, 0.92, 1.0)
        nad = band_median(atlas, -1.0, -0.92)
        report.append(f"    horizon lum {lum(hor):.3f} zenith lum {lum(zen):.3f} nadir lum {lum(nad):.3f}")
        assert lum(hor) > lum(zen) * 1.3, "+Y is not the zenith / horizon not brighter"
        assert lum(hor) > lum(nad) * 1.3
    if sid == "sakura-dusk":
        assert band_median(atlas, 0.0, 0.05)[0] > band_median(atlas, 0.0, 0.05)[2], "horizon not warm"
    for sid_m, m in (("tsukihime", unit(0.45, 0.42, -0.78)), ("sakura-dusk", unit(-0.45, 0.50, -0.74))):
        if sid == sid_m:
            v = sample_atlas(atlas, m[None, :])[0]
            report.append(f"    moon sample lum {lum(v):.3f} at elevation {math.degrees(math.asin(m[1])):.0f} deg")
            assert lum(v) > 0.6, "moon not where the forward table says it should be"


def build_skies(files, report, cache):
    roundtrip_check()
    for sid, s in SKIES.items():
        assert ID_RE.match(sid)
        stars = gen_stars(s["seed"], s["stars"])
        atlas = render_atlas(s["fn"], stars, s["seed"])
        jpg = encode_atlas(atlas, s["seed"] + 500)
        dec = np.asarray(Image.open(io.BytesIO(jpg)).convert("RGB"), dtype=np.float64)
        cache[sid] = dec
        verify_sky(sid, dec, report)
        zen = band_median(dec / 255, 0.88, 1.0)
        hor = band_median(dec / 255, -0.06, 0.08)
        ex = s["colors_extra"]
        grp = {"sky": to_hex(zen), "fog": to_hex(hor), "cloud": to_hex(hor * ex["cloud_mix"] + zen * (1 - ex["cloud_mix"])),
               "sunset": ex["sunset"]}
        sky = {"name": s["name"], "time": s["time"]}
        if s["clear"]:
            sky["clearWeather"] = True
        sky["colors"] = {"day": dict(grp), "night": dict(grp)}
        sky["stars"] = 1.0
        sky["skybox"] = {"texture": "sky.jpg", "dayBrightness": 1.0, "nightBrightness": 1.0}
        if s["rotation"]:
            sky["skybox"]["rotation"] = s["rotation"]
        files[f"skies/{sid}/sky.json"] = json_bytes(sky)
        files[f"skies/{sid}/sky.jpg"] = jpg
        report.append(f"  {sid}: jpg {len(jpg) // 1024} KiB, sky {grp['sky']} fog {grp['fog']}")
    for sid, s in COLOR_SKIES.items():
        assert ID_RE.match(sid)
        files[f"skies/{sid}/sky.json"] = json_bytes({"name": s["name"], "colors": s["colors"], "stars": 1.0})


def sky_previews(cache):
    PREVIEW.mkdir(parents=True, exist_ok=True)
    W, H = 1024, 512
    lon = (np.arange(W) + 0.5) / W * TAU - math.pi
    lat = math.pi / 2 - (np.arange(H) + 0.5) / H * math.pi
    lo, la = np.meshgrid(lon, lat)
    d = np.stack([np.cos(la) * np.sin(lo), np.sin(la), -np.cos(la) * np.cos(lo)], -1)
    for sid, atlas in cache.items():
        eq = np.clip(sample_atlas(atlas, d), 0, 255).astype(np.uint8)
        Image.fromarray(eq).save(PREVIEW / f"sky_{sid}_equirect.png")
        t = 256
        cross = Image.new("RGB", (4 * t, 3 * t), (30, 30, 30))
        for f, (cx, cy) in {"+Y": (1, 0), "-X": (0, 1), "+Z": (1, 1), "+X": (2, 1), "-Z": (3, 1), "-Y": (1, 2)}.items():
            c, r = ATLAS[f]
            tile = Image.fromarray(atlas[r * N:(r + 1) * N, c * N:(c + 1) * N].astype(np.uint8)).resize((t, t), Image.LANCZOS)
            cross.paste(tile, (cx * t, cy * t))
        cross.save(PREVIEW / f"sky_{sid}_cross.png")


# ------------------------------------------------------------- cosmetics ----
TEX = 64


def box_regions(u, v, w, h, d):
    """Java/Bedrock box-UV layout: region (x, y, w, h) per face.
    The region at (u, v+d) belongs to the -x face, (u+d+w, v+d) to +x (spec: Java model x)."""
    return {
        "up": (u + d, v, w, d), "down": (u + d + w, v, w, d),
        "-x": (u, v + d, d, h), "north": (u + d, v + d, w, h),
        "+x": (u + d + w, v + d, d, h), "south": (u + d + w + d, v + d, w, h),
    }


def box_extent(u, v, w, h, d):
    return u, v, 2 * (d + w), d + h


class Tex:
    def __init__(self):
        self.a = np.zeros((TEX, TEX, 4), np.uint8)

    def fill(self, region, fn):
        x, y, w, h = region
        for j in range(h):
            for i in range(w):
                self.a[y + j, x + i] = fn(i, j, w, h)

    def box(self, u, v, w, h, d, fn):
        for face, reg in box_regions(u, v, w, h, d).items():
            self.fill(reg, lambda i, j, rw, rh, face=face: fn(face, i, j, rw, rh))


def rgba(hexc, a=255, k=1.0):
    c = np.clip(C(hexc) * 255 * k + 0.5, 0, 255)
    return (int(c[0]), int(c[1]), int(c[2]), a)


def mixc(c1, c2, t):
    return tuple(int(round(c1[i] * (1 - t) + c2[i] * t)) for i in range(4))


class Geo:
    def __init__(self, ident):
        self.ident = ident
        self.bones = []
        self.rects = []   # (u, v, w, h, label) uv extents for validation

    def bone(self, name, pivot, rotation=None, parent=None):
        b = {"name": name, "pivot": pivot}
        if parent:
            b["parent"] = parent
        if rotation:
            b["rotation"] = rotation
        b["cubes"] = []
        self.bones.append(b)
        return b

    def cube(self, bone, origin, size, uv, pivot=None, rotation=None):
        c = {"origin": origin, "size": size, "uv": uv}
        if pivot is not None:
            c["pivot"] = pivot
        if rotation is not None:
            c["rotation"] = rotation
        bone["cubes"].append(c)
        w, h, d = size
        self.rects.append(box_extent(uv[0], uv[1], int(w), int(h), int(d)))

    def doc(self):
        return {"format_version": "1.12.0", "minecraft:geometry": [{
            "description": {"identifier": self.ident, "texture_width": TEX, "texture_height": TEX,
                            "visible_bounds_width": 4, "visible_bounds_height": 4, "visible_bounds_offset": [0, 1.5, 0]},
            "bones": self.bones}]}


def validate_geo(doc, rects):
    g = doc["minecraft:geometry"][0]
    bones = g["bones"]
    ncubes = sum(len(b.get("cubes", [])) for b in bones)
    assert len(bones) <= 64 and ncubes <= 512, (len(bones), ncubes)
    names = {b["name"] for b in bones}
    assert len(names) == len(bones)
    for b in bones:
        assert b.get("parent") in (None, *names)
    for u, v, w, h in rects:
        assert 0 <= u and 0 <= v and u + w <= TEX and v + h <= TEX, ("uv outside texture", (u, v, w, h))
    uniq = sorted(set(rects))   # identical rectangles are intentional shared texels
    for i, a in enumerate(uniq):
        for b in uniq[i + 1:]:
            ov = a[0] < b[0] + b[2] and b[0] < a[0] + a[2] and a[1] < b[1] + b[3] and b[1] < a[1] + a[3]
            assert not ov, ("uv boxes overlap", a, b)
    json.loads(json.dumps(doc))
    return len(bones), ncubes


# --- halo: octagonal ring, 8 flat cubes rotated about y in 45 degree steps ---
def halo():
    geo = Geo("geometry.aoko.halo")
    ring = geo.bone("ring", [0, 33.5, 0])
    # Centre-line apothem 4.5, radial thickness 1 -> outer apothem 5.0. Each cube is 4 long:
    # an ideal mitred octagon edge would be 3.73 (centre line), so neighbours overlap by
    # ~0.27 px on the inner side and leave no gap on the outer side.
    for k in range(8):
        geo.cube(ring, [-2, 33, -5], [4, 1, 1], [0, 0], pivot=[0, 33.5, 0], rotation=[0, 45 * k, 0])
    t = Tex()
    gold, white, warm = rgba("#ffd36a"), rgba("#fff8e0"), rgba("#ffb347", 200)

    def paint(face, i, j, w, h):
        if face in ("north", "south"):
            return mixc(white, gold, 0.15 + 0.1 * abs(i - (w - 1) / 2) / (w / 2))
        if face == "up":
            return mixc(white, gold, 0.25)
        if face == "down":
            return warm
        return gold   # end caps
    t.box(0, 0, 4, 1, 1, paint)
    # soft glow swatch (reference gradient, unused by the model)
    for j in range(8):
        for i in range(40):
            r = abs(i - 19.5) / 20
            t.a[12 + j, 12 + i] = mixc(rgba("#fff8e0", 255), rgba("#ffd36a", 0), min(1, r * 1.2 + j / 9 * 0.0))
    cos = {"name": "Halo", "slot": "hat", "anchor": "head", "model": "model.geo.json", "texture": "texture.png",
           "scale": 1.0, "offset": [0, 0, 0], "fullbright": True, "translucent": True, "hideWithHelmet": False,
           "animations": [{"bone": "ring", "spin": [0, 40, 0], "bob": [0, 0.5, 0], "speed": 0.5, "phase": 0}]}
    return geo, t, cos


# --- cat ears: 3 stacked narrowing cubes per ear, tilted outward ---
def cat_ears():
    geo = Geo("geometry.aoko.cat_ears")
    t = Tex()
    dark, dark2, pink, pink2 = rgba("#2b2733"), rgba("#3a3544"), rgba("#f7a8c4"), rgba("#ffd0e0")
    tiers = [  # (x offset from ear centre, width, y, height, depth, z)
        (1.5, 3, 32, 2, 2, -2.0), (1.0, 2, 34, 2, 2, -2.0), (0.5, 1, 36, 1, 1, -1.5)]
    # tiers of each ear get their own UV block; all faces share one packing row
    blocks = {}
    cur_u, cur_v = 0, 0
    for side in ("left", "right"):
        for ti, (_, w, _, h, d, _) in enumerate(tiers):
            bw, bh = 2 * (d + w), d + h
            if cur_u + bw > TEX:
                cur_u, cur_v = 0, cur_v + 5
            blocks[(side, ti)] = (cur_u, cur_v)
            cur_u += bw
    for side, sx, rot in (("left", 1, -8), ("right", -1, 8)):
        cx = 2.5 * sx
        bone = geo.bone(f"ear_{side}", [cx, 32, -1.5])
        for ti, (off, w, y, h, d, z) in enumerate(tiers):
            ox = cx - w / 2
            u, v = blocks[(side, ti)]
            geo.cube(bone, [ox, y, z], [w, h, d], [u, v], pivot=[cx, 32, -1.5], rotation=[0, 0, rot])

            def paint(face, i, j, rw, rh, ti=ti):
                if face == "north" and ti < 2:   # inner ear on the front
                    edge = i == 0 or i == rw - 1
                    if rw >= 3 and edge and j == 0:
                        return dark
                    if ti == 1 and j == 0:
                        return pink2
                    return pink if j else mixc(pink, pink2, 0.5)
                return mixc(dark, dark2, 0.5 if face in ("up",) else 0.0 if j else 0.3)
            t.box(u, v, w, h, d, paint)
    cos = {"name": "Cat Ears", "slot": "hat", "anchor": "head", "model": "model.geo.json", "texture": "texture.png",
           "scale": 1.0, "offset": [0, 0, 0], "fullbright": False, "translucent": False, "hideWithHelmet": True,
           "animations": [
               {"bone": "ear_left", "swing": [0, 0, 6], "speed": 0.35, "phase": 0},
               {"bone": "ear_right", "swing": [0, 0, -6], "speed": 0.35, "phase": 0}]}
    return geo, t, cos


# --- angel wings ---
# The wing plane is x/y (fans outward and up from the shoulder pivot). Each wing hinges about
# the vertical (y) axis through the shoulder, so a swing moves the tips in z: open away from
# the back (+z) and sideways. Rest pose is rotated 25 deg towards +z so a +-18 deg sine never
# pushes a wing into the body. Rotations are standard matrices (Ry(t): z' = -x sin t + z cos t),
# so the +x (left) wing needs a negative y angle to move its tip to +z; the right wing gets the
# opposite sign.
WING_LEN = [13, 11, 9, 7, 5]
WING_ANG = [58, 38, 18, -2, -22]
WING_H = [3, 3, 3, 3, 3]


def angel_wings():
    geo = Geo("geometry.aoko.angel_wings")
    t = Tex()
    white, blue, vein = rgba("#f6f9ff"), rgba("#d6e4fb"), rgba("#c3d6f5")

    def paint(face, i, j, w, h):
        if face in ("north", "south"):
            base = mixc(white, blue, j / max(1, h - 1) * 0.7)
            return mixc(base, vein, 0.6) if j == h // 2 and i % 3 != 1 else base
        if face in ("up", "down"):
            return mixc(white, blue, 0.35)
        return blue
    for si, (side, sx, rest) in enumerate((("left", 1, -25), ("right", -1, 25))):
        px = 2.0 * sx
        bone = geo.bone(f"wing_{side}", [px, 22, 2], rotation=[0, rest, 0])
        for k, (L, a, hh) in enumerate(zip(WING_LEN, WING_ANG, WING_H)):
            z = 2.4 + 0.3 * k   # shorter feathers sit further back: no coplanar overlap
            ox = px if sx > 0 else px - L
            u, v = 32 * si, 4 * k
            geo.cube(bone, [ox, 22 - hh / 2, z], [L, hh, 1], [u, v], pivot=[px, 22, 2], rotation=[0, 0, a * sx])
            t.box(u, v, L, hh, 1, paint)
    cos = {"name": "Angel Wings", "slot": "back", "anchor": "body", "model": "model.geo.json", "texture": "texture.png",
           "scale": 1.0, "offset": [0, 0, 0], "fullbright": False, "translucent": False, "hideWithHelmet": False,
           "animations": [
               {"bone": "wing_left", "swing": [0, -18, 0], "speed": 0.6, "phase": 0},
               {"bone": "wing_right", "swing": [0, 18, 0], "speed": 0.6, "phase": 0}]}
    return geo, t, cos


COSMETICS = {"halo": halo, "cat-ears": cat_ears, "angel-wings": angel_wings}


def build_cosmetics(files, report, cache):
    for cid, fn in COSMETICS.items():
        assert ID_RE.match(cid)
        geo, tex, cos = fn()
        doc = geo.doc()
        nb, nc = validate_geo(doc, geo.rects)
        assert len(cos["animations"]) <= 16
        bones = {b["name"] for b in doc["minecraft:geometry"][0]["bones"]}
        assert all(a["bone"] in bones for a in cos["animations"])
        files[f"cosmetics/{cid}/cosmetic.json"] = json_bytes(cos)
        files[f"cosmetics/{cid}/model.geo.json"] = json_bytes(doc)
        files[f"cosmetics/{cid}/texture.png"] = png_bytes(tex.a)
        cache[cid] = (doc, tex.a, cos)
        report.append(f"  {cid}: {nb} bones, {nc} cubes, uv ok")


# --- software preview (orthographic, z-buffer, box-UV conventions above) ---
def rot_zyx(deg):
    x, y, z = (math.radians(a) for a in deg)
    cx, sx, cy, sy, cz, sz = math.cos(x), math.sin(x), math.cos(y), math.sin(y), math.cos(z), math.sin(z)
    Rx = np.array([[1, 0, 0], [0, cx, -sx], [0, sx, cx]])
    Ry = np.array([[cy, 0, sy], [0, 1, 0], [-sy, 0, cy]])
    Rz = np.array([[cz, -sz, 0], [sz, cz, 0], [0, 0, 1]])
    return Rz @ Ry @ Rx


def faces_of(origin, size):
    ox, oy, oz = origin
    w, h, d = size
    # (tl, tr, bl) corners as seen from outside; assumed viewing convention, see notes
    return {
        "north": ((ox + w, oy + h, oz), (ox, oy + h, oz), (ox + w, oy, oz)),
        "south": ((ox, oy + h, oz + d), (ox + w, oy + h, oz + d), (ox, oy, oz + d)),
        "+x": ((ox + w, oy + h, oz + d), (ox + w, oy + h, oz), (ox + w, oy, oz + d)),
        "-x": ((ox, oy + h, oz), (ox, oy + h, oz + d), (ox, oy, oz)),
        "up": ((ox + w, oy + h, oz + d), (ox, oy + h, oz + d), (ox + w, oy + h, oz)),
        "down": ((ox + w, oy, oz), (ox, oy, oz), (ox + w, oy, oz + d)),
    }


def collect_faces(doc, cos, pose_t=None):
    bones = {b["name"]: b for b in doc["minecraft:geometry"][0]["bones"]}
    anim = {}
    if pose_t is not None:
        for a in cos.get("animations", []):
            ph = 2 * math.pi * (pose_t * a.get("speed", 1) + a.get("phase", 0))
            e = anim.setdefault(a["bone"], {"rot": np.zeros(3), "tr": np.zeros(3)})
            e["rot"] += np.array(a.get("swing", [0, 0, 0])) * math.sin(ph) + np.array(a.get("spin", [0, 0, 0])) * pose_t
            e["tr"] += np.array(a.get("bob", [0, 0, 0])) * math.sin(ph)
    out = []
    for b in bones.values():
        chain, cur = [], b
        while cur:
            chain.append(cur)
            cur = bones.get(cur.get("parent"))
        for c in b.get("cubes", []):
            w, h, d = c["size"]
            u, v = c["uv"]
            regs = box_regions(u, v, int(w), int(h), int(d))
            for name, (tl, tr, bl) in faces_of(c["origin"], c["size"]).items():
                pts = np.array([tl, tr, bl], float)
                if "rotation" in c:
                    pv = np.array(c.get("pivot", [0, 0, 0]), float)
                    pts = (pts - pv) @ rot_zyx(c["rotation"]).T + pv
                for bn in chain:
                    pv = np.array(bn["pivot"], float)
                    rot = np.array(bn.get("rotation", [0, 0, 0]), float) + anim.get(bn["name"], {}).get("rot", 0)
                    pts = (pts - pv) @ rot_zyx(rot).T + pv + anim.get(bn["name"], {}).get("tr", 0)
                out.append((pts, regs[name], name))
    return out


def render_view(faces, tex, az, el, translucent, scale=12, size=(420, 460), centre=(0, 26, 0), fullbright=False):
    az, el = math.radians(az), math.radians(el)
    cam = np.array([math.sin(az) * math.cos(el), math.sin(el), -math.cos(az) * math.cos(el)])
    fwd = -cam
    right = np.cross(fwd, [0, 1, 0])
    if np.linalg.norm(right) < 1e-6:
        right = np.array([1.0, 0, 0])
    right /= np.linalg.norm(right)
    up = np.cross(right, fwd)
    W, H = size
    img = np.zeros((H, W, 3))
    img[:] = (28, 30, 40)
    depth = np.full((H, W), -1e9)
    ctr = np.array(centre, float)

    def proj(p):
        q = p - ctr
        return np.array([W / 2 + (q @ right) * scale, H / 2 - (q @ up) * scale]), q @ cam

    ref = []
    for o, s, col in (([-4, 24, -4], [8, 8, 8], (150, 150, 160)), ([-4, 12, -2], [8, 12, 4], (120, 125, 150))):
        for name, (tl, tr, bl) in faces_of(o, s).items():
            ref.append((np.array([tl, tr, bl], float), None, name, col))
    jobs = [(p, None, n, c) for p, _, n, c in ref] + [(p, r, n, None) for p, r, n in faces]
    order = sorted(range(len(jobs)), key=lambda i: (jobs[i][1] is not None, (jobs[i][0].mean(0) - ctr) @ cam))
    shade = {"up": 1.0, "down": 0.6, "north": 0.85, "south": 0.8, "+x": 0.7, "-x": 0.7}
    for i in order:
        pts, reg, name, col = jobs[i]
        s0, _ = proj(pts[0])
        s1, _ = proj(pts[1])
        s2, _ = proj(pts[2])
        ea, eb = s1 - s0, s2 - s0
        det = ea[0] * eb[1] - ea[1] * eb[0]
        if abs(det) < 1e-6:
            continue
        xs = [s0[0], s1[0], s2[0], s1[0] + s2[0] - s0[0]]
        ys = [s0[1], s1[1], s2[1], s1[1] + s2[1] - s0[1]]
        x0, x1 = max(0, int(min(xs))), min(W, int(max(xs)) + 1)
        y0, y1 = max(0, int(min(ys))), min(H, int(max(ys)) + 1)
        if x0 >= x1 or y0 >= y1:
            continue
        Y, X = np.mgrid[y0:y1, x0:x1]
        px, py = X + 0.5 - s0[0], Y + 0.5 - s0[1]
        a = (px * eb[1] - py * eb[0]) / det
        b = (ea[0] * py - ea[1] * px) / det
        inside = (a >= 0) & (a < 1) & (b >= 0) & (b < 1)
        world = pts[0] + a[..., None] * (pts[1] - pts[0]) + b[..., None] * (pts[2] - pts[0])
        dz = (world - ctr) @ cam
        sh = shade[name] if not fullbright or col else 1.0
        if reg is None:
            colr = np.broadcast_to(np.array(col, float) * shade[name], px.shape + (3,))
            alpha = np.ones(px.shape)
        else:
            rx, ry, rw, rh = reg
            tx = np.clip((a * rw).astype(int), 0, rw - 1) + rx
            ty = np.clip((b * rh).astype(int), 0, rh - 1) + ry
            texel = tex[ty, tx].astype(float)
            colr, alpha = texel[..., :3] * sh, texel[..., 3] / 255
        ok = inside & (dz >= depth[y0:y1, x0:x1])
        sub, dsub = img[y0:y1, x0:x1], depth[y0:y1, x0:x1]
        if reg is not None and translucent:
            m = ok & (alpha > 0.02)
            sub[m] = sub[m] * (1 - alpha[m, None]) + colr[m] * alpha[m, None]
        else:
            m = ok & (alpha >= 0.5)
            sub[m] = colr[m]
            dsub[m] = dz[m]
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))


def cape_previews(files):
    PREVIEW.mkdir(parents=True, exist_ok=True)
    S = 4
    sheet = Image.new("RGB", (2 * 128 * S + 12, 3 * 64 * S + 16), (60, 60, 66))
    for i, kind in enumerate(CAPE_IDS):
        im = Image.open(io.BytesIO(files[f"capes/{kind}.png"])).convert("RGBA")
        t = Image.new("RGBA", im.size, (60, 60, 66, 255))
        t.alpha_composite(im)
        sheet.paste(t.convert("RGB").resize((128 * S, 64 * S), Image.NEAREST), (4 + (i % 2) * (128 * S + 4), 4 + (i // 2) * (64 * S + 4)))
    sheet.save(PREVIEW / "capes.png")


def cosmetic_previews(cache):
    PREVIEW.mkdir(parents=True, exist_ok=True)
    from PIL import ImageFont
    views = [("front", 0, 12), ("back", 180, 12), ("side", 90, 8), ("top", 0, 72)]
    for cid, (doc, tex, cos) in cache.items():
        rows = []
        for label, pose in (("rest", None), ("anim peak", 0.25)):
            faces = collect_faces(doc, cos, None if pose is None else pose / cos["animations"][0].get("speed", 1))
            row = Image.new("RGB", (420 * 4, 460), (28, 30, 40))
            for i, (vn, az, el) in enumerate(views):
                im = render_view(faces, tex, az, el, cos["translucent"], fullbright=cos["fullbright"])
                ImageDraw.Draw(im).text((8, 6), f"{cid} {vn} ({label})", fill=(200, 210, 230))
                row.paste(im, (i * 420, 0))
            rows.append(row)
        sheet = Image.new("RGB", (1680, 920))
        sheet.paste(rows[0], (0, 0))
        sheet.paste(rows[1], (0, 460))
        sheet.save(PREVIEW / f"cosmetic_{cid}.png")
        bg = Image.new("RGBA", (TEX * 8, TEX * 8), (60, 60, 70, 255))
        bg.alpha_composite(Image.fromarray(tex).resize((TEX * 8, TEX * 8), Image.NEAREST))
        bg.convert("RGB").save(PREVIEW / f"cosmetic_{cid}_texture.png")


# ------------------------------------------------------------------ main ----
def generate(only, ids=None, report=None, cache=None, ccache=None):
    files = {}
    report = [] if report is None else report
    cache = {} if cache is None else cache
    ccache = {} if ccache is None else ccache
    global SKIES
    if ids:
        SKIES = {k: v for k, v in SKIES.items() if k in ids}
    if only in (None, "capes"):
        build_capes(files)
    if only in (None, "skies"):
        build_skies(files, report, cache)
    if only in (None, "cosmetics"):
        build_cosmetics(files, report, ccache)
    return files


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="exit 1 if committed outputs differ")
    ap.add_argument("--only", choices=["capes", "skies", "cosmetics"], help="development filter")
    ap.add_argument("--ids", help="comma separated textured sky ids (development filter)")
    ap.add_argument("--no-preview", action="store_true")
    args = ap.parse_args()
    report, cache, ccache = [], {}, {}
    files = generate(args.only, args.ids.split(",") if args.ids else None, report, cache, ccache)
    print("\n".join(report))
    if args.check:
        bad = []
        for rel, data in sorted(files.items()):
            p = OUT / rel
            if not p.exists():
                bad.append(f"missing  {rel}")
                continue
            disk = p.read_bytes()
            if rel.endswith(".json"):   # git may check text out with CRLF
                disk = disk.replace(b"\r\n", b"\n")
            if disk != data:
                bad.append(f"differs  {rel}")
        if args.only is None and not args.ids:
            for p in sorted(OUT.rglob("*")):
                if p.is_file() and p.relative_to(OUT).as_posix() not in files:
                    bad.append(f"stale    {p.relative_to(OUT).as_posix()}")
        print("\n".join(bad) if bad else f"ok: {len(files)} files match")
        return 1 if bad else 0
    for rel, data in files.items():
        p = OUT / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"wrote {len(files)} files")
    if not args.no_preview and cache:
        sky_previews(cache)
    if not args.no_preview and args.only in (None, 'capes'):
        cape_previews(files)
    if not args.no_preview and ccache:
        cosmetic_previews(ccache)
    return 0


if __name__ == "__main__":
    sys.exit(main())
