"""Artifact scanner for engine audio: clicks/pops, stray tones (drones, whines), HF line spikes.

usage: scan.py file.wav [--rpm trace.csv --ncyl N]   (rpm trace: lines "t,rpm" to classify tones as engine-locked)
Prints a short report; every finding is a time and a number so a fix can be checked against it.
"""
import sys
import numpy as np
from scipy import signal
from lab import load


def clicks(x, sr, hp=300.0, win_ms=1.0, ctx_ms=60.0, thresh_db=14.0, top=12):
    """Short bursts that stand out from their surroundings: energy in 1 ms windows against the median of the
    surrounding 60 ms, after a high-pass (a pop or click is broadband and brief; engine pulses are periodic)."""
    sos = signal.butter(4, hp, "hp", fs=sr, output="sos")
    y = signal.sosfilt(sos, x)
    w = max(1, int(win_ms * 1e-3 * sr))
    e = np.convolve(y * y, np.ones(w) / w, mode="same")
    c = int(ctx_ms * 1e-3 * sr)
    # running median of the energy on a decimated grid
    step = max(1, w)
    ed = e[::step]
    k = max(3, (c // step) | 1)
    med = signal.medfilt(ed, k)
    ratio = 10 * np.log10((ed + 1e-14) / (med + 1e-14))
    idx = np.where(ratio > thresh_db)[0]
    # merge neighbours
    out = []
    last = -10
    for i in idx:
        if i - last > 3:
            out.append([i, ratio[i], ed[i]])
        elif ratio[i] > out[-1][1]:
            out[-1][1] = ratio[i]
            out[-1][0] = i
        last = i
    out.sort(key=lambda r: -r[1])
    return [(r[0] * step / sr, r[1], 10 * np.log10(r[2] + 1e-14)) for r in out[:top]], len(out)


def sample_jumps(x, sr, k=8.0, top=8):
    """Discontinuities: second difference far above the local level (a waveform step or a dropout edge)."""
    d2 = np.abs(np.diff(x, 2))
    w = int(0.01 * sr)
    loc = np.sqrt(np.convolve(d2 * d2, np.ones(w) / w, mode="same")) + 1e-9
    r = d2 / loc
    idx = np.where(r > k * 4)[0]
    out = []
    last = -1000
    for i in idx:
        if i - last > 40:
            out.append((i / sr, r[i], d2[i]))
        last = i
    out.sort(key=lambda t: -t[1])
    return out[:top], len(out)


def tones(x, sr, f_lo=150.0, f_hi=9000.0, nper=8192, prom_db=14.0, min_frac=0.5):
    """Spectral lines that persist: peaks standing prom_db above the median of their +-150 Hz surround, found in many
    frames at (nearly) the same frequency. Returns (freq, fraction of frames, mean prominence)."""
    f, t, S = signal.spectrogram(x, sr, window="hann", nperseg=nper, noverlap=nper * 3 // 4, mode="psd")
    L = 10 * np.log10(S + 1e-20)
    df = f[1] - f[0]
    m = (f >= f_lo) & (f <= f_hi)
    fi = np.where(m)[0]
    surround = int(150 / df)
    counts = {}
    for j in range(L.shape[1]):
        col = L[:, j]
        med = signal.medfilt(col, 2 * surround + 1)
        pk, _ = signal.find_peaks(col - med, height=prom_db, distance=3)
        for p in pk:
            if f_lo <= f[p] <= f_hi:
                b = int(round(f[p] / (2 * df)))
                counts.setdefault(b, []).append(col[p] - med[p])
    res = []
    for b, v in counts.items():
        frac = len(v) / L.shape[1]
        if frac >= min_frac:
            res.append((b * 2 * df, frac, float(np.mean(v))))
    res.sort(key=lambda r: -r[2])
    return res


def report(path, ncyl=None):
    x, sr = load(path)
    print(f"== {path}: {len(x) / sr:.1f} s, peak {np.abs(x).max():.3f}")
    cl, n = clicks(x, sr)
    print(f"   clicks/pops (>14 dB over their surround, 1 ms): {n}")
    for t, r, lv in cl[:8]:
        print(f"     t={t:7.3f}s  +{r:4.1f} dB  ({lv:.0f} dBFS^2 burst)")
    sj, n = sample_jumps(x, sr)
    print(f"   waveform discontinuities: {n}")
    for t, r, d in sj[:5]:
        print(f"     t={t:7.3f}s  ratio {r:6.0f}  d2 {d:.4f}")
    tn = tones(x, sr)
    print(f"   persistent lines (>=50% of frames, >=14 dB over surround, 150 Hz-9 kHz): {len(tn)}")
    for fr, frac, pr in tn[:10]:
        print(f"     {fr:7.0f} Hz  in {frac * 100:3.0f}% of frames, +{pr:4.1f} dB")


if __name__ == "__main__":
    for p in sys.argv[1:]:
        report(p)
