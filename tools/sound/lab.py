"""Spectral lab for comparing recorded and simulated engine audio."""
import sys, os
import numpy as np
from scipy.io import wavfile
from scipy import signal
from PIL import Image, ImageDraw

SP = os.path.dirname(os.path.abspath(__file__))


def load(path, ch=None):
    sr, x = wavfile.read(path)
    if x.dtype == np.int16:
        x = x.astype(np.float32) / 32768.0
    elif x.dtype == np.int32:
        x = x.astype(np.float32) / 2147483648.0
    else:
        x = x.astype(np.float32)
    if x.ndim == 2:
        x = x[:, ch] if ch is not None else x.mean(axis=1)
    return x, sr


def _cmap(v):
    """v in 0..1 -> RGB (inferno-ish)."""
    stops = np.array([[0, 0, 4], [40, 11, 84], [101, 21, 110], [159, 42, 99], [212, 72, 66],
                      [245, 125, 21], [250, 193, 39], [252, 255, 164]], dtype=np.float32)
    v = np.clip(v, 0, 1) * (len(stops) - 1)
    i = np.minimum(v.astype(int), len(stops) - 2)
    f = (v - i)[..., None]
    return (stops[i] * (1 - f) + stops[i + 1] * f).astype(np.uint8)


def spectrogram_png(x, sr, out, fmin=20, fmax=16000, win=0.093, hop=0.01, db_range=80,
                    title="", height=520, width=1200, t0=None, t1=None, ref_db=None, scale_to=None):
    if t0 is not None or t1 is not None:
        a = int((t0 or 0) * sr)
        b = int((t1 or len(x) / sr) * sr)
        x = x[a:b]
    nper = int(2 ** np.ceil(np.log2(win * sr)))
    f, t, S = signal.spectrogram(x, sr, window="hann", nperseg=nper, noverlap=nper - int(hop * sr),
                                 scaling="spectrum", mode="magnitude")
    D = 20 * np.log10(S + 1e-12)
    top = (D.max() if ref_db is None else ref_db)
    D = D - top
    fmax = min(fmax, sr / 2)
    # log-frequency rows
    rows = np.geomspace(fmin, fmax, height)
    idx = np.clip(np.searchsorted(f, rows), 0, len(f) - 1)
    img = D[idx, :]
    # resample to width columns
    cols = np.linspace(0, img.shape[1] - 1, width).astype(int)
    img = img[:, cols]
    rgb = _cmap((img + db_range) / db_range)[::-1]
    im = Image.fromarray(rgb)
    pad_l, pad_b, pad_t = 52, 22, 18
    canvas = Image.new("RGB", (width + pad_l, height + pad_b + pad_t), (16, 16, 16))
    canvas.paste(im, (pad_l, pad_t))
    d = ImageDraw.Draw(canvas)
    for fr in [30, 50, 100, 200, 300, 500, 1000, 2000, 3000, 5000, 10000]:
        if fmin <= fr <= fmax:
            y = pad_t + height - 1 - int(np.log(fr / fmin) / np.log(fmax / fmin) * (height - 1))
            d.line([(pad_l - 4, y), (pad_l + width, y)], fill=(70, 70, 70))
            d.text((2, y - 5), f"{fr / 1000:g}k" if fr >= 1000 else f"{fr}", fill=(220, 220, 220))
    dur = len(x) / sr
    step = 1 if dur < 12 else 2 if dur < 30 else 5 if dur < 80 else 10
    for s in np.arange(0, dur, step):
        xx = pad_l + int(s / dur * (width - 1))
        d.line([(xx, pad_t), (xx, pad_t + height)], fill=(50, 50, 50))
        d.text((xx + 2, pad_t + height + 5), f"{s + (t0 or 0):g}s", fill=(220, 220, 220))
    d.text((pad_l + 4, 3), f"{title}  [{db_range} dB range, peak {top:.0f} dBFS-ish]", fill=(255, 255, 255))
    canvas.save(out)
    return top


def welch_db(x, sr, nperseg=16384):
    f, P = signal.welch(x, sr, window="hann", nperseg=nperseg, noverlap=nperseg // 2, scaling="spectrum")
    return f, 10 * np.log10(P + 1e-20)


THIRD = [20 * 2 ** (i / 3) for i in range(0, 30)]


def third_octave(x, sr, f_lo=25, f_hi=16000):
    f, P = signal.welch(x, sr, window="hann", nperseg=min(len(x), 32768), noverlap=None, scaling="density")
    centers, levels = [], []
    c = 31.5
    while c < f_hi:
        lo, hi = c / 2 ** (1 / 6), c * 2 ** (1 / 6)
        m = (f >= lo) & (f < hi)
        if m.any() and c >= f_lo:
            centers.append(c)
            levels.append(10 * np.log10(P[m].sum() * (f[1] - f[0]) + 1e-20))
        c *= 2 ** (1 / 3)
    return np.array(centers), np.array(levels)


if __name__ == "__main__":
    path = sys.argv[1]
    out = sys.argv[2]
    x, sr = load(path)
    t0 = float(sys.argv[3]) if len(sys.argv) > 3 else None
    t1 = float(sys.argv[4]) if len(sys.argv) > 4 else None
    top = spectrogram_png(x, sr, out, title=os.path.basename(path), t0=t0, t1=t1)
    print(f"{path}: {len(x) / sr:.2f}s sr={sr} peak={np.abs(x).max():.3f} rms={np.sqrt((x ** 2).mean()):.4f}")
