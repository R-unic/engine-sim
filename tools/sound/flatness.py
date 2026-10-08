"""Line-vs-floor structure: spectral flatness of 93 ms frames in bands, and mean peak-to-valley, for a steady segment."""
import sys, numpy as np
from scipy import signal
from lab import load
def run(path, t0, t1, tag):
    x, sr = load(path); x = x[int(t0*sr):int(t1*sr)]
    nper = 4096
    f, t, S = signal.spectrogram(x, sr, window="hann", nperseg=nper, noverlap=nper//2, mode="psd")
    out = [f"{tag:14s}"]
    for lo, hi in [(100, 400), (400, 1000), (1000, 2000), (2000, 4000)]:
        m = (f >= lo) & (f < hi)
        P = S[m] + 1e-20
        sfm = 10*np.log10(np.exp(np.log(P).mean(axis=0)) / P.mean(axis=0))   # per frame
        out.append(f"{lo}-{hi}: SFM {sfm.mean():5.1f} dB")
    print("  ".join(out))
for spec in sys.argv[1:]:
    p, a, b = spec.split(":")[:3]
    run(p, float(a), float(b), p.split("/")[-1][:14])
