"""Octave-band levels of a window of a recording against another window of it (a key turn against the idle after it).

usage: bandrel.py file.wav:crank_t0:crank_t1:idle_t0:idle_t1 [...]
Prints the per-octave level (dB re full scale) of both windows, their difference and the totals: the shape of a starter
and its level against the engine, from a real recording or a `soundlab start` render.
"""
import sys
import numpy as np
from scipy import signal
sys.path.insert(0, '.')
from lab import load
EDGES = [40, 100, 200, 400, 800, 1600, 3200, 6400, 12000]
def bands(x, sr, t0, t1):
    seg = x[int(t0*sr):int(t1*sr)]
    f, P = signal.welch(seg, sr, nperseg=4096, window='hann')
    df = f[1]-f[0]
    return [10*np.log10(P[(f>=lo)&(f<hi)].sum()*df+1e-20) for lo,hi in zip(EDGES[:-1],EDGES[1:])]
for spec in sys.argv[1:]:
    name,a,b,c,d = spec.split(':')
    x,sr = load(name)
    cr = bands(x,sr,float(a),float(b)); idl = bands(x,sr,float(c),float(d))
    print(name.split('/')[-1])
    print('  crank  ', ' '.join('%6.1f'%v for v in cr))
    print('  idle   ', ' '.join('%6.1f'%v for v in idl))
    print('  crank-idle', ' '.join('%6.1f'%(u-v) for u,v in zip(cr,idl)))
    tot_c = 10*np.log10(sum(10**(v/10) for v in cr)); tot_i=10*np.log10(sum(10**(v/10) for v in idl))
    print('  total crank %.1f idle %.1f diff %.1f'%(tot_c,tot_i,tot_c-tot_i))
print('edges', EDGES)
