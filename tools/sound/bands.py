import sys, numpy as np
from scipy import signal
from lab import load
x, sr = load(sys.argv[1]); step = float(sys.argv[2]) if len(sys.argv) > 2 else 1.0
nper = 4096
f, t, S = signal.spectrogram(x, sr, window="hann", nperseg=nper, noverlap=nper//2, mode="psd")
df = f[1]-f[0]
bands = [(150,700),(700,2000),(2000,5000),(5000,10000)]
def lv(lo,hi,j):
    m=(f>=lo)&(f<hi); return 10*np.log10(S[m,j].sum()*df+1e-20)
# dominant ladder spacing: strongest peak in 80-450 Hz
print("   t    150-700  700-2k  2k-5k  5k-10k |  HF(2-5k)-LF   strongest 80-450 Hz")
for t0 in np.arange(0, t[-1]-step, step):
    js = np.where((t>=t0)&(t<t0+step))[0]
    row=[np.mean([lv(lo,hi,j) for j in js]) for lo,hi in bands]
    sp = S[:, js].mean(axis=1); m=(f>80)&(f<450); fp=f[m][np.argmax(sp[m])]
    print(f"{t0:5.1f}  {row[0]:7.1f} {row[1]:7.1f} {row[2]:6.1f} {row[3]:6.1f} | {row[2]-row[0]:8.1f}      {fp:6.1f}")
