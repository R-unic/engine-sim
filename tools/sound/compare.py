"""Overlay the average spectra of two recordings (A = reference, B = simulation) and tabulate 1/3-octave levels.

usage: cmp.py out.png A.wav a0 a1 B.wav b0 b1 [labelA labelB]
"""
import sys
import numpy as np
from scipy import signal
from PIL import Image, ImageDraw
from lab import load, third_octave


def avg_psd(x, sr, nper=32768):
    nper = min(nper, 2 ** int(np.floor(np.log2(len(x)))))
    f, P = signal.welch(x, sr, window="hann", nperseg=nper, noverlap=nper // 2, scaling="spectrum")
    return f, P


def smooth_octave(f, P, frac=12):
    """Power average in +-1/(2*frac) octave windows, evaluated on a log grid."""
    grid = np.geomspace(30, 16000, 600)
    out = np.zeros_like(grid)
    for i, g in enumerate(grid):
        lo, hi = g / 2 ** (0.5 / frac), g * 2 ** (0.5 / frac)
        m = (f >= lo) & (f < hi)
        out[i] = P[m].mean() if m.any() else P[np.searchsorted(f, g)]
    return grid, out


def main():
    out = sys.argv[1]
    A, a0, a1, B, b0, b1 = sys.argv[2], float(sys.argv[3]), float(sys.argv[4]), sys.argv[5], float(sys.argv[6]), float(sys.argv[7])
    la = sys.argv[8] if len(sys.argv) > 8 else "ref"
    lb = sys.argv[9] if len(sys.argv) > 9 else "sim"
    xa, sa = load(A)
    xb, sb = load(B)
    xa = xa[int(a0 * sa):int(a1 * sa)]
    xb = xb[int(b0 * sb):int(b1 * sb)]
    fa, Pa = avg_psd(xa, sa)
    fb, Pb = avg_psd(xb, sb)
    # normalise each to its power in 60 Hz - 8 kHz
    def tot(f, P):
        m = (f > 60) & (f < 8000)
        return P[m].sum()
    na, nb = tot(fa, Pa), tot(fb, Pb)
    W, H = 1200, 520
    pad_l, pad_b, pad_t = 52, 22, 20
    img = Image.new("RGB", (W + pad_l, H + pad_b + pad_t), (16, 16, 16))
    d = ImageDraw.Draw(img)
    lo_db, hi_db = -100, -10
    fmin, fmax = 30, 16000

    def X(fr):
        return pad_l + (np.log(fr / fmin) / np.log(fmax / fmin)) * (W - 1)

    def Y(db):
        return pad_t + (hi_db - db) / (hi_db - lo_db) * H

    for fr in [50, 100, 200, 300, 500, 1000, 2000, 3000, 5000, 10000]:
        d.line([(X(fr), pad_t), (X(fr), pad_t + H)], fill=(55, 55, 55))
        d.text((X(fr) - 8, pad_t + H + 5), f"{fr / 1000:g}k" if fr >= 1000 else str(fr), fill=(200, 200, 200))
    for db in range(lo_db, hi_db + 1, 10):
        d.line([(pad_l, Y(db)), (pad_l + W, Y(db))], fill=(55, 55, 55))
        d.text((2, Y(db) - 5), f"{db}", fill=(200, 200, 200))
    for (f, P, n, col, sr) in [(fa, Pa, na, (80, 200, 255), sa), (fb, Pb, nb, (255, 150, 60), sb)]:
        pts = []
        # fine trace in the lows (max-hold per pixel column), smooth above
        db = 10 * np.log10(P / n + 1e-20)
        cols = {}
        for fr, v in zip(f, db):
            if fr < fmin or fr > min(fmax, sr / 2):
                continue
            c = int(X(fr))
            cols[c] = max(cols.get(c, -999), v)
        pts = [(c, Y(v)) for c, v in sorted(cols.items())]
        d.line(pts, fill=col, width=1)
        g, s = smooth_octave(f, P)
        sm = [(X(gg), Y(10 * np.log10(v / n + 1e-20))) for gg, v in zip(g, s) if gg < sr / 2]
        d.line(sm, fill=(255, 255, 255) if col[0] < 100 else (255, 255, 0), width=2)
    d.text((pad_l + 6, 4), f"cyan/white: {la}    orange/yellow: {lb}    (each normalised to its 60 Hz-8 kHz power; thick = 1/12-octave mean)", fill=(255, 255, 255))
    img.save(out)

    ca, la3 = third_octave(xa, sa)
    cb, lb3 = third_octave(xb, sb)
    ta, tb = la3[(ca > 60) & (ca < 8000)], lb3[(cb > 60) & (cb < 8000)]
    na3 = 10 * np.log10((10 ** (la3 / 10))[(ca > 60) & (ca < 8000)].sum())
    nb3 = 10 * np.log10((10 ** (lb3 / 10))[(cb > 60) & (cb < 8000)].sum())
    print(f"{'band':>7} {la:>8} {lb:>8} {'sim-ref':>8}")
    n = min(len(ca), len(cb))
    for i in range(n):
        if 50 <= ca[i] <= 12000:
            print(f"{ca[i]:7.0f} {la3[i] - na3:8.1f} {lb3[i] - nb3:8.1f} {(lb3[i] - nb3) - (la3[i] - na3):8.1f}")


if __name__ == "__main__":
    main()
