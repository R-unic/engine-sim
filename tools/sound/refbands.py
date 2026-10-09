import sys, glob, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from scipy import signal
from lab import load
import warnings; warnings.filterwarnings("ignore")
BANDS=[("30-80",30,80),("80-200",80,200),("200-500",200,500),(".5-1.5k",500,1500),("1.5-4k",1500,4000),("4-10k",4000,10000)]
def run(path, step=1.0):
    x,sr=load(path)
    nper=8192 if sr>=44100 else 4096
    f,t,S=signal.spectrogram(x,sr,window="hann",nperseg=nper,noverlap=nper//2,mode="psd")
    df=f[1]-f[0]
    print(os.path.basename(path), f"{len(x)/sr:.1f}s")
    tt=0.0
    while tt<len(x)/sr-0.5:
        m=(t>=tt)&(t<tt+step)
        if m.any():
            P=S[:,m].mean(axis=1)
            row=[10*np.log10(P[(f>=lo)&(f<min(hi,sr/2))].sum()*df+1e-20) for _,lo,hi in BANDS]
            low=10*np.log10(sum(10**(r/10) for r in row[:2])); body=10*np.log10(sum(10**(r/10) for r in row[2:4]))
            # strongest line 40-400 Hz
            sel=(f>=40)&(f<=400); fp=f[sel][np.argmax(P[sel])]
            print(f" {tt:4.1f}s "+" ".join(f"{r:6.1f}" for r in row)+f"  low-body {low-body:5.1f}  peak {fp:5.0f} Hz")
        tt+=step
for p in sys.argv[1:]:
    run(p)
