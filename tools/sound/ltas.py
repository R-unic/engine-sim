"""Band balance of driven engine audio by rpm bin: how much low end, body and top end each engine has.

usage: ltas.py DIR [engine ...]    (DIR holds drive_<id>.wav and drive_<id>_rpm.csv from render_drive_scenarios)
Prints, per engine and rpm bin, the level (dB re full scale) of six bands and the low end (30-200 Hz) re the body (200-1500 Hz).
"""
import sys, os
import numpy as np
from scipy import signal
from lab import load

BANDS = [("sub 30-80", 30, 80), ("low 80-200", 80, 200), ("lm 200-500", 200, 500), ("mid .5-1.5k", 500, 1500), ("hi 1.5-4k", 1500, 4000), ("air 4-10k", 4000, 10000)]
BINS = [("idle <1300", 0, 1300), ("1.3-2.5k", 1300, 2500), ("2.5-4k", 2500, 4000), ("4-5.5k", 4000, 5500), (">5.5k", 5500, 99999)]


def band_db(f, S, lo, hi):
    m = (f >= lo) & (f < hi)
    df = f[1] - f[0]
    return 10 * np.log10(S[m].sum() * df + 1e-20)


def main():
    d = sys.argv[1]
    ids = sys.argv[2:] or sorted(n[6:-4] for n in os.listdir(d) if n.startswith("drive_") and n.endswith(".wav"))
    print(f"{'engine':9s} {'rpm bin':10s} " + " ".join(f"{b[0]:>12s}" for b in BANDS) + "   low-body")
    for eid in ids:
        x, sr = load(os.path.join(d, f"drive_{eid}.wav"))
        tr = np.loadtxt(os.path.join(d, f"drive_{eid}_rpm.csv"), delimiter=",")
        nper = 8192
        f, t, S = signal.spectrogram(x, sr, window="hann", nperseg=nper, noverlap=nper // 2, mode="psd")
        rpm = np.interp(t, tr[:, 0], tr[:, 1])
        for name, lo, hi in BINS:
            js = np.where((rpm >= lo) & (rpm < hi) & (t > 3.0))[0]
            if len(js) < 2:
                continue
            row = [np.mean([band_db(f, S[:, j], b[1], b[2]) for j in js]) for b in BANDS]
            low = 10 * np.log10(sum(10 ** (r / 10) for r in row[:2]))
            body = 10 * np.log10(sum(10 ** (r / 10) for r in row[2:4]))
            print(f"{eid:9s} {name:10s} " + " ".join(f"{r:12.1f}" for r in row) + f"   {low - body:7.1f}")


main()
