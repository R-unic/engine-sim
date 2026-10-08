#!/bin/bash
# Real engines against the sound model: decodes the reference recordings, renders the model at matching operating
# points with soundlab, and prints the numbers the model was calibrated on.
#
#   tools/sound/compare_refs.sh [tag] [quick]       (needs: cargo, python with numpy + scipy + pillow)
#
# Output goes to target/soundlab/<tag>/ (WAVs, raw per-source dumps, overlay PNGs in target/soundlab/img/).
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
TAG=${1:-run}
PY=${PY:-python}
OUT="$ROOT/target/soundlab/$TAG"
REF="$ROOT/target/soundlab/ref"
IMG="$ROOT/target/soundlab/img"
Q=${Q:-high}
mkdir -p "$OUT" "$REF" "$IMG"
cd "$ROOT"

# 1. the recordings as WAV
if [ ! -f "$REF/maserati_granturismo_s.wav" ]; then
  (cd tools/sound/oggdec && cargo build --release -q)
  for f in crates/engine-core/reference/*.ogg; do
    tools/sound/oggdec/target/release/oggdec "$f" "$REF/$(basename "$f" .ogg).wav" > /dev/null
  done
fi

# 2. the model
cargo build --release -q -p engine-core --example soundlab
SL="$ROOT/target/release/examples/soundlab"
steady() { # engine rpm throttle
  "$SL" steady --engine "$1" --rpm "$2" --throttle "$3" --secs 6 --quality "$Q" --dump --out "$OUT" > "$OUT/log_$1_$2.txt" 2>&1
  $PY "$HERE/rawwav.py" "$OUT/raw_$1_$2_final.f32" "$OUT/steady_$1_$2.wav"
}
steady v8 850 0.12
steady v12 720 0.12
steady i6 1270 0.12

cd "$ROOT/target/soundlab"   # relative names below: a native Python cannot read MSYS-style paths joined with :t0:t1
t() { local s="$1"; shift; $PY "$HERE/$s" "$@"; }
echo "== band levels, dB (150-700 / 700-2k / 2-5k / 5-10k) and HF(2-5k) minus LF, steady idle"
echo "Maserati V8 (850 rpm)   :"; t bands.py ref/maserati_granturismo_s.wav 12 | sed -n 3p
echo "sim V8 850              :"; t bands.py $TAG/steady_v8_850.wav 3 | sed -n 2p
echo "BMW M70 V12 (720 rpm)   :"; t bands.py ref/bmw_m70_v12_idle.wav 2 | sed -n 2p
echo "sim V12 720             :"; t bands.py $TAG/steady_v12_720.wav 3 | sed -n 2p
echo "Slant six (1270 rpm)    :"; t bands.py ref/chrysler_225_slant6.wav 3 | sed -n 2p
echo "sim I6 1270             :"; t bands.py $TAG/steady_i6_1270.wav 3 | sed -n 2p
echo "== HF envelope of the model's output at idle (Gaussian noise: rms/mean 0.55, crest ~4; pulse spikes: >1.3, >10)"
t hfstats.py $TAG/raw_v8_850 | tail -1
echo "== line structure (spectral flatness of 93 ms frames; white noise -2.5 dB, a pure comb far lower)"
t flatness.py ref/maserati_granturismo_s.wav:12:18 ref/bmw_m70_v12_idle.wav:0.3:4.3 $TAG/steady_v8_850.wav:1:6 $TAG/steady_v12_720.wav:1:6
if [ "$2" != "quick" ]; then
  cd "$ROOT"
  "$SL" sweep --engine v8 --from 900 --to 6200 --secs 14 --quality "$Q" --out "$OUT" > /dev/null 2>&1
  cd "$ROOT/target/soundlab"
  echo "== V8 rev: model sweep against the Maserati's revving, 1/3-octave, each normalised to its 60 Hz-8 kHz power"
  t compare.py img/rev_$TAG.png ref/maserati_granturismo_s.wav 21 58 $TAG/sweep_v8.wav 1.7 11 Maserati sim | awk '$1>=100'
  echo "== formant ripple of the long-term spectrum (rms dB around its smooth trend)"
  t ripple.py ref/maserati_granturismo_s.wav:21:58 $TAG/sweep_v8.wav:1.7:11
fi
