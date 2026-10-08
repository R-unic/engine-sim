"""Formant ripple: how much the long-term spectrum of a rev wiggles around its own smooth trend (drone/ridge strength)."""
import sys, numpy as np
from scipy import signal
from lab import load
def avg(path, t0, t1):
    x, sr = load(path); x = x[int(t0*sr):int(t1*sr)]
    nper = 32768
    f, P = signal.welch(x, sr, window="hann", nperseg=nper, noverlap=nper//2, scaling="spectrum")
    return f, P
def smooth(f, P, frac):
    g = np.geomspace(120, 3000, 400)
    out = np.empty_like(g)
    for i, c in enumerate(g):
        m = (f >= c/2**(0.5/frac)) & (f < c*2**(0.5/frac))
        out[i] = P[m].mean() if m.any() else np.nan
    return g, 10*np.log10(out)
for spec in sys.argv[1:]:
    p, a, b = spec.split(":")
    f, P = avg(p, float(a), float(b))
    g, fine = smooth(f, P, 12)       # 1/12 octave
    _, coarse = smooth(f, P, 1.5)    # 2/3 octave trend
    r = fine - coarse
    print(f"{p.split('/')[-1][:24]:24s} ripple rms {np.sqrt(np.nanmean(r**2)):4.1f} dB  max peak {np.nanmax(r):4.1f}  deepest notch {np.nanmin(r):5.1f}")
