"""Waveform of one engine cycle (average over the cycles of a steady dump) as a PNG, for seeing which firing events are
loud and which are weak.

usage: foldplot.py raw.f32 rpm out.png [ncyl]
Vertical lines mark the firing events (720/ncyl degrees apart) if ncyl is given.
"""
import sys
import numpy as np
from PIL import Image, ImageDraw

SR = 44100.0


def main():
    x = np.fromfile(sys.argv[1], dtype="<f4").astype(np.float64)
    rpm = float(sys.argv[2])
    ncyl = int(sys.argv[4]) if len(sys.argv) > 4 else 0
    period = 120.0 / rpm * SR
    n = int(len(x) // period)
    p = int(round(period))
    acc = np.zeros(p)
    for c in range(n):
        s = int(round(c * period))
        acc += x[s : s + p]
    acc /= n
    h = 360
    img = Image.new("RGB", (p, h), (16, 16, 24))
    d = ImageDraw.Draw(img)
    if ncyl:
        for k in range(ncyl):
            xx = int(k * p / ncyl)
            d.line([(xx, 0), (xx, h)], fill=(60, 60, 90))
    m = np.abs(acc).max()
    pts = [(i, h / 2 - acc[i] / m * (h / 2 - 4)) for i in range(p)]
    d.line(pts, fill=(255, 200, 80))
    img.save(sys.argv[3])
    print("cycle samples", p, "peak", m)


main()
