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
| `ltas.py` | band balance (30-80 / 80-200 / 200-500 / 0.5-1.5k / 1.5-4k / 4-10k Hz) by engine and rpm bin of the drive renders, and the low end re the body: `ltas.py DIR [engine ...]` |
| `timeline.py` | level and four band levels over time next to the rpm trace: `timeline.py file.wav rpm.csv [step] [t0 t1]` |
| `bandrel.py` | per-octave level of one window against another of the same file (a key turn against the idle after it), their difference and totals: `bandrel.py file.wav:t0:t1:t0:t1` |
| `tonality.py` | how far the strongest spectral line stands out of its surroundings (dB), where it is, the share of 40-1500 Hz power in the three strongest lines and how many lines are within 15 dB, by engine and rpm bin: a drone is a line 25+ dB out carrying a third of the power: `tonality.py DIR [engine ...]` |
| `fold.py`, `foldplot.py` | fold a steady `soundlab --dump` at the engine cycle (rms per slice / the averaged waveform as a PNG with the firing events marked): shows which firings are loud and which are weak, i.e. where the half orders come from |
| `zoomspec.py` | spectrogram of a short window with a short FFT, to see single events (a pop, a flutter chirp, one diesel clatter): `file.wav out.png fmin fmax win_s t0 t1` |
| `pops.py` | detects bangs in the 1.5-8 kHz band (a 2 ms envelope more than `THR` dB, default 12, over its 200 ms median) and prints their level, decay, centroid and spacing: `file.wav[:t0:t1] ...` |
| `hfcrest.py` | how peaky the 1.5-8 kHz and 0.3-1.5 kHz envelopes are: their 90th / 99th / 99.9th percentile over the median, dB. Rally cars on the overrun: p99 17-32, p99.9 25-37; steady engine noise 8 / 10 |
| `refbands.py` | the six band levels, the low end re the body and the strongest low line, second by second, of recordings: `file.wav ...` |
| `scan.py` | artifact scanner: clicks and pops, waveform discontinuities, persistent stray tones: `file.wav ...` |
| `rawwav.py` | raw f32 (`soundlab --dump`) to a 16-bit WAV for the tools above |

Numbers that mean something (from the recordings): the envelope of a 1-8 kHz band of a real engine at idle has an
rms/mean of 0.53-0.57 and a crest factor of 3.9-4.5 (Gaussian noise: 0.52 and ~4); a train of sharp pulses is above 1.3 and 10.
Spectral flatness of a white-noise frame is about -2.5 dB, a harmonic comb is far lower. With a native Windows Python,
pass relative paths when a tool takes `file:t0:t1` (MSYS-style `/c/...` paths are not translated inside it).
