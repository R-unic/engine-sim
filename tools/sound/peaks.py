"""Line structure of a (nearly) steady segment: strongest spectral lines, fundamental estimate, tilt, floor."""
import sys
import numpy as np
from scipy import signal
from lab import load, third_octave


def psd(x, sr, nper=None, fmax=None):
    nper = nper or int(2 ** np.ceil(np.log2(sr * 1.5)))
    nper = min(nper, len(x))
    f, P = signal.welch(x, sr, window="hann", nperseg=nper, noverlap=nper // 2, scaling="spectrum")
    return f, P


def comb_f0(f, P, lo, hi, nh=10):
    """f0 maximising the summed log-power at its harmonics (sub-harmonic safe by penalising empty harmonics)."""
    L = 10 * np.log10(P + 1e-20)
    best = (None, -1e9)
    for f0 in np.arange(lo, hi, 0.1):
        s = 0.0
        for h in range(1, nh + 1):
            k = int(round(f0 * h / (f[1] - f[0])))
            if k >= len(f) - 2:
                break
            s += L[k - 1:k + 2].max()
        s /= nh
        if s > best[1]:
            best = (f0, s)
    return best


def report(x, sr, name, f0_lo=15, f0_hi=120, nh=10, topn=14):
    f, P = psd(x, sr)
    f0, sc = comb_f0(f, P, f0_lo, f0_hi, nh)
    L = 10 * np.log10(P + 1e-20)
    pk, pr = signal.find_peaks(L, prominence=8, distance=3)
    order = np.argsort(L[pk])[::-1][:topn]
    print(f"== {name}: {len(x) / sr:.2f}s  comb f0 {f0:.2f} Hz (score {sc:.1f})")
    ref = L[pk[order[0]]]
    for i in sorted(order, key=lambda i: f[pk[i]]):
        fr = f[pk[i]]
        print(f"   {fr:8.1f} Hz  {L[pk[i]] - ref:6.1f} dB   x f0 = {fr / f0:5.2f}")
    c, lv = third_octave(x, sr)
    top = lv.max()
    print("   1/3 oct rel: " + " ".join(f"{cc:.0f}:{l - top:.0f}" for cc, l in zip(c, lv)))
    return f0


if __name__ == "__main__":
    path, t0, t1 = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
    lo = float(sys.argv[4]) if len(sys.argv) > 4 else 15
    hi = float(sys.argv[5]) if len(sys.argv) > 5 else 120
    x, sr = load(path)
    report(x[int(t0 * sr):int(t1 * sr)], sr, f"{path} {t0}-{t1}", lo, hi)
