"""Fold a steady-state raw dump (soundlab steady --dump) at the engine cycle and show the pulse train.

usage: fold.py raw_<id>_<rpm>_<src>.f32 rpm [bins]
Prints the rms in `bins` equal slices of the 720 degree cycle (default 64): a perfectly even engine shows the same
pattern every 720/ncyl degrees; differences between neighbours show up as low orders (and half orders) in the spectrum.
"""
import sys
import numpy as np

SR = 44100.0


def main():
    x = np.fromfile(sys.argv[1], dtype="<f4").astype(np.float64)
    rpm = float(sys.argv[2])
    bins = int(sys.argv[3]) if len(sys.argv) > 3 else 64
    period = 120.0 / rpm * SR  # samples per cycle
    n_cycles = int(len(x) // period)
    x = x[: int(n_cycles * period)]
    idx = (np.arange(len(x)) % period) / period
    b = np.minimum((idx * bins).astype(int), bins - 1)
    e = np.array([np.sqrt(np.mean(x[b == k] ** 2)) for k in range(bins)])
    e /= e.max()
    print(f"{n_cycles} cycles of {period:.1f} samples; rms per slice (1 = largest):")
    for k in range(0, bins, 16):
        print(" ".join(f"{v:4.2f}" for v in e[k : k + 16]))
    # cycle-to-cycle level of the whole thing: the AM that is not locked to the engine
    per = [np.sqrt(np.mean(x[int(c * period) : int((c + 1) * period)] ** 2)) for c in range(n_cycles)]
    print("rms per cycle: mean %.3g, sd/mean %.3f" % (np.mean(per), np.std(per) / np.mean(per)))


main()
