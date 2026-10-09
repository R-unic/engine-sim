import sys, os, warnings
warnings.filterwarnings("ignore")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from scipy import signal, ndimage
from lab import load
for a in sys.argv[1:]:
    p=a.split(":"); x,sr=load(p[0]); t0=float(p[1]) if len(p)>1 else 0; t1=float(p[2]) if len(p)>2 else len(x)/sr
    x=x[int(t0*sr):int(t1*sr)]
    out=[]
    for lo,hi in [(1500,8000),(300,1500)]:
        sos=signal.butter(4,[lo,hi],btype="band",fs=sr,output="sos"); y=signal.sosfilt(sos,x)
        env=np.sqrt(ndimage.uniform_filter1d(y*y,int(0.002*sr)))
        d=lambda q: 20*np.log10(np.percentile(env,q)/np.median(env))
        out.append(f"{lo}-{hi}: p90 {d(90):4.1f} p99 {d(99):4.1f} p99.9 {d(99.9):4.1f} dB re median")
    print(f"{os.path.basename(p[0]):28s} "+" | ".join(out))
