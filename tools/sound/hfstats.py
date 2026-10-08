import sys, numpy as np
from scipy import signal
sr = 44100
base = sys.argv[1]
for name in ["tail", "jet", "shell", "mech", "intake", "final"]:
    x = np.fromfile(f"{base}_{name}.f32", "<f4").astype(np.float64)[sr//2:]
    out = [f"{name:6s} rms {np.sqrt((x**2).mean()):.3e}"]
    for lo, hi in [(1000, 2000), (2000, 4000), (4000, 8000)]:
        sos = signal.butter(4, [lo, hi], "bp", fs=sr, output="sos")
        y = signal.sosfilt(sos, x)
        env = np.abs(signal.hilbert(y))[2000:-2000]
        m = env.mean() + 1e-30
        out.append(f"{lo//1000}-{hi//1000}k: lvl {20*np.log10(np.sqrt((y**2).mean())/20e-6+1e-12):5.1f}dB r/m {np.sqrt(((env-m)**2).mean())/m:4.2f} crest {env.max()/m:5.1f}")
    print("  ".join(out))
