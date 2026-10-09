import sys, os, warnings
warnings.filterwarnings("ignore")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from scipy import signal, ndimage
from lab import load
def analyse(path, t0=0, t1=None, thr_db=float(os.environ.get("THR","12"))):
    x,sr=load(path)
    x=x[int(t0*sr):int((t1 or len(x)/sr)*sr)]
    sos=signal.butter(4,[1500,8000],btype="band",fs=sr,output="sos")
    y=signal.sosfilt(sos,x)
    env=np.sqrt(ndimage.uniform_filter1d(y*y,int(0.002*sr)))
    med=ndimage.median_filter(env[::int(0.005*sr)],size=41)  # ~200 ms
    med=np.interp(np.arange(len(env)),np.arange(len(med))*int(0.005*sr),med)+1e-9
    ratio=20*np.log10(env/med)
    pk,_=signal.find_peaks(ratio,height=thr_db,distance=int(0.025*sr))
    print(os.path.basename(path),f"{len(x)/sr:.1f}s  events {len(pk)}")
    out=[]
    for p in pk:
        # decay: time for envelope to fall 10 dB below peak
        seg=env[p:p+int(0.3*sr)]; d=np.where(seg<seg[0]*0.316)[0]
        dec=(d[0]/sr*1000) if len(d) else 300
        # low-band vs high-band at the event
        a=int(p-0.005*sr); b=int(p+0.04*sr)
        w=x[max(a,0):b]
        F=np.abs(np.fft.rfft(w*np.hanning(len(w))))**2; fr=np.fft.rfftfreq(len(w),1/sr)
        cen=(fr*F).sum()/F.sum()
        out.append((p/sr+t0,ratio[p],dec,cen))
    for o in out[:40]: print(f"   t={o[0]:6.2f}s  +{o[1]:4.1f} dB  decay10dB {o[2]:5.1f} ms  centroid {o[3]:6.0f} Hz")
    if len(out)>2:
        ia=np.diff([o[0] for o in out]); print("   inter-arrival (ms): median %.0f  p10 %.0f  p90 %.0f ; level dB median %.1f max %.1f"%(np.median(ia)*1000,np.percentile(ia,10)*1000,np.percentile(ia,90)*1000,np.median([o[1] for o in out]),max(o[1] for o in out)))
for a in sys.argv[1:]:
    p=a.split(":")
    analyse(p[0],float(p[1]) if len(p)>1 else 0,float(p[2]) if len(p)>2 else None)
