# Realism pass, 2026-10-08

A/B renders of the same scripted drive (key turn, idle, launch with auto-shift, wide open, lift-off, braking) through the
live path (sim -> sound model), before and after the comparison with real recordings:

- `drive_v8_before.wav` / `drive_v8_after.wav`, `drive_i4turbo_before.wav` / `drive_i4turbo_after.wav` and their spectrograms
  (log frequency 20 Hz-16 kHz, 80 dB range). After: the 2-10 kHz haze that fills in with load instead of sparse spikes.
- `rev_v8_vs_maserati_before.png` / `_after.png`: long-term spectrum of the model's V8 rev (orange/yellow) over the real
  Maserati GranTurismo S rev (cyan/white), each normalised to its 60 Hz-8 kHz power. The Maserati rolls off below ~120 Hz
  (its microphone), which is the only place they disagree on purpose.
- `ref_maserati_spectrogram.png`: the real recording for comparison.

The recordings themselves are not copied here (see `crates/engine-core/reference/README.md`).
