import sys
from lab import load, spectrogram_png
x, sr = load(sys.argv[1])
fmin, fmax = float(sys.argv[3]), float(sys.argv[4])
t0 = float(sys.argv[5]) if len(sys.argv) > 5 else None
t1 = float(sys.argv[6]) if len(sys.argv) > 6 else None
spectrogram_png(x, sr, sys.argv[2], fmin=fmin, fmax=fmax, win=0.19, hop=0.02, db_range=70, title=sys.argv[1].split("/")[-1], t0=t0, t1=t1, height=480, width=1000)
