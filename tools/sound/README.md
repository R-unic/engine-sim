# Sound analysis kit

Python (numpy, scipy, pillow) and a small Rust converter for looking at engine audio: the sound model's output next to
real recordings. Everything runs from the repository root; output lands in `target/soundlab/`.

```
tools/sound/compare_refs.sh [tag] [quick]     decode the references, render the model at matching operating
                                              points, print band levels, noise statistics, line structure,
                                              rev comparison and formant ripple (PY=python3 to pick the interpreter)
```

| tool | what it does |
|---|---|
| `oggdec/` | `.ogg` to 16-bit WAV (the recordings in `crates/engine-core/reference/` are Vorbis); a standalone cargo project on `lewton` |
| `lab.py` | loader, log-frequency spectrogram PNG, 1/3-octave levels; `python lab.py file.wav out.png [t0 t1]` |
| `spectrogram.py` | zoomed spectrogram: `file.wav out.png fmin fmax [t0 t1]` |
| `compare.py` | overlay of two average spectra plus a 1/3-octave table: `out.png A.wav a0 a1 B.wav b0 b1 [labelA labelB]` |
| `bands.py` | level of 150-700 / 700-2k / 2-5k / 5-10k Hz per window and the top end re the body: `file.wav window_s` |
| `hfstats.py` | envelope statistics (rms/mean and crest of 1-2, 2-4, 4-8 kHz) of each raw source from `soundlab steady --dump`: `target/soundlab/raw_v8_850` |
| `modulation.py` | depth of the amplitude modulation of the high bands at the firing frequency: `file.wav t0 t1 f_fire` |
| `flatness.py` | spectral flatness of 93 ms frames per band (how line-like the spectrum is): `file.wav:t0:t1 ...` |
| `ripple.py` | formant ripple of the long-term spectrum of a rev: `file.wav:t0:t1 ...` |
| `peaks.py` | strongest spectral lines of a steady segment and a comb estimate of the fundamental: `file.wav t0 t1 [f0_lo f0_hi]` |
| `scan.py` | artifact scanner: clicks and pops, waveform discontinuities, persistent stray tones: `file.wav ...` |
| `rawwav.py` | raw f32 (`soundlab --dump`) to a 16-bit WAV for the tools above |

Numbers that mean something (from the recordings): the envelope of a 1-8 kHz band of a real engine at idle has an
rms/mean of 0.53-0.57 and a crest factor of 3.9-4.5 (Gaussian noise: 0.52 and ~4); a train of sharp pulses is above 1.3 and 10.
Spectral flatness of a white-noise frame is about -2.5 dB, a harmonic comb is far lower. With a native Windows Python,
pass relative paths when a tool takes `file:t0:t1` (MSYS-style `/c/...` paths are not translated inside it).
