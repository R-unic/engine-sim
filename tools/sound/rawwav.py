"""Write the raw 'final' f32 as a 16-bit wav for the other tools (scaled by 1)."""
import sys, numpy as np
from scipy.io import wavfile
x = np.fromfile(sys.argv[1], "<f4")
wavfile.write(sys.argv[2], 44100, (np.clip(x, -1, 1) * 32767).astype(np.int16))
