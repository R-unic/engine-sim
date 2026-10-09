"""Zoomed spectrogram of a time window: zoomspec.py file.wav out.png fmin fmax win_s t0 t1  (win 0.01-0.02 s shows single events)."""
import os
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lab import load, spectrogram_png
x, sr = load(sys.argv[1])
spectrogram_png(x, sr, sys.argv[2], fmin=float(sys.argv[3]), fmax=float(sys.argv[4]), win=float(sys.argv[5]), hop=0.002, t0=float(sys.argv[6]), t1=float(sys.argv[7]), width=1200, height=420)
