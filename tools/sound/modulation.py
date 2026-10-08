"""Amplitude modulation of a high band: envelope depth at the firing frequency and its harmonics."""
import sys, numpy as np
from scipy import signal
from lab import load
path, t0, t1, f_fire = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
x, sr = load(path); x = x[int(t0*sr):int(t1*sr)]
for lo, hi in [(1000, 2000), (2000, 4000), (4000, 8000)]:
    sos = signal.butter(4, [lo, hi], "bp", fs=sr, output="sos")
    y = signal.sosfilt(sos, x)
    env = np.abs(signal.hilbert(y))
    env = env[int(0.2*sr):]
    m = env.mean()
    e = env - m
    f, P = signal.welch(e, sr, nperseg=min(len(e), 32768), noverlap=None)
    df = f[1]-f[0]
    def amp(fc):
        k = (f > fc*0.93) & (f < fc*1.07)
        return np.sqrt(P[k].sum()*df)/m if k.any() else 0
    tot = np.sqrt(P[(f>1)&(f<400)].sum()*df)/m
    print(f"{lo}-{hi} Hz: envelope rms/mean {np.sqrt((e**2).mean())/m:.2f}; depth at f_fire {amp(f_fire):.2f}, 2x {amp(2*f_fire):.2f}, f_fire/2 {amp(f_fire/2):.2f}; crest {env.max()/m:.1f}")
