"""Level and band balance over time next to the rpm trace: where a sound lingers, jumps or goes harsh.

usage: timeline.py file.wav rpm.csv [step_s] [t0] [t1]
Columns: time, rpm, total level (dBFS), and 30-200 / 200-1500 / 1.5-4k / 4-10k Hz levels (dBFS).
"""
import sys
import numpy as np
from scipy import signal
from lab import load

x, sr = load(sys.argv[1])
tr = np.loadtxt(sys.argv[2], delimiter=",")
step = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
t0 = float(sys.argv[4]) if len(sys.argv) > 4 else 0.0
t1 = float(sys.argv[5]) if len(sys.argv) > 5 else len(x) / sr
bands = [(30, 200), (200, 1500), (1500, 4000), (4000, 10000)]
nper = 4096
f, t, S = signal.spectrogram(x, sr, window="hann", nperseg=nper, noverlap=nper // 2, mode="psd")
df = f[1] - f[0]
print("  t    rpm   total   30-200  .2-1.5k  1.5-4k   4-10k")
for a in np.arange(t0, t1, step):
    seg = x[int(a * sr):int((a + step) * sr)]
    if len(seg) == 0:
        break
    tot = 10 * np.log10(np.mean(seg ** 2) + 1e-20)
    js = np.where((t >= a) & (t < a + step))[0]
    row = []
    for lo, hi in bands:
        m = (f >= lo) & (f < hi)
        row.append(10 * np.log10(S[m][:, js].sum(axis=0).mean() * df + 1e-20))
    rpm = np.interp(a + step / 2, tr[:, 0], tr[:, 1])
    print(f"{a:5.1f} {rpm:6.0f} {tot:7.1f} " + " ".join(f"{r:8.1f}" for r in row))
