"""Tonal prominence of driven engine audio by engine and rpm bin: how far the strongest spectral lines stand out of the
broadband level around them, and how much of the 40-1500 Hz power sits in the three strongest lines.

usage: tonality.py DIR [engine ...]    (DIR holds drive_<id>.wav and drive_<id>_rpm.csv from render_drive_scenarios)
A pure drone is a line that stands 25-35 dB out of its surroundings and carries most of the power; a rich note has many lines
and a flat comb. Columns: prominence (dB) of the strongest line, its frequency, the share of 40-1500 Hz power in the three
strongest lines (%), and the number of lines within 15 dB of the strongest.
"""
import sys, os
import numpy as np
from scipy import signal
from lab import load

BINS = [("idle <1300", 0, 1300), ("1.3-2.5k", 1300, 2500), ("2.5-4k", 2500, 4000), ("4-5.5k", 4000, 5500), (">5.5k", 5500, 99999)]


def main():
    d = sys.argv[1]
    ids = sys.argv[2:] or sorted(n[6:-4] for n in os.listdir(d) if n.startswith("drive_") and n.endswith(".wav"))
    print(f"{'engine':9s} {'rpm bin':10s} {'prominence':>10s} {'at Hz':>7s} {'top3 %':>7s} {'lines':>6s}")
    for eid in ids:
        x, sr = load(os.path.join(d, f"drive_{eid}.wav"))
        tr = np.loadtxt(os.path.join(d, f"drive_{eid}_rpm.csv"), delimiter=",")
        nper = 8192
        f, t, S = signal.spectrogram(x, sr, window="hann", nperseg=nper, noverlap=nper // 2, mode="psd")
        rpm = np.interp(t, tr[:, 0], tr[:, 1])
        m = (f >= 40) & (f <= 1500)
        ff = f[m]
        for name, lo, hi in BINS:
            js = np.where((rpm >= lo) & (rpm < hi) & (t > 3.0))[0]
            rows = []
            for j in js:
                P = S[m, j] + 1e-20
                # local broadband level: median in a +-1/3 octave window, per bin
                med = np.array([np.median(P[(ff > f0 / 1.26) & (ff < f0 * 1.26)]) for f0 in ff])
                prom = 10 * np.log10(P / med)
                pk = signal.find_peaks(P)[0]
                if len(pk) == 0:
                    continue
                order = pk[np.argsort(-P[pk])]
                top = order[0]
                share = P[order[:3]].sum() / P.sum()
                n15 = int((P[pk] > P[top] * 10 ** -1.5).sum())
                rows.append((prom[top], ff[top], share, n15))
            if len(rows) < 2:
                continue
            a = np.array([[r[0], r[1], r[2], r[3]] for r in rows])
            print(f"{eid:9s} {name:10s} {np.median(a[:, 0]):10.1f} {np.median(a[:, 1]):7.0f} {100 * np.median(a[:, 2]):7.0f} {np.median(a[:, 3]):6.0f}")


main()
