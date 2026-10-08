# Reference recordings

Real engines, used only to *measure* the sound model against (spectra, line structure, noise floor, level versus
load - see `tools/sound/` and the Audio section of the top-level README). They are not played back and not copied
into the sound model; the `.ogg` files themselves are git-ignored, only this list is kept.

| file | what | length | source and license |
|---|---|---|---|
| `maserati_granturismo_s.ogg` | Maserati GranTurismo S 4.7 V8: starts, idles, revs and blips the throttle; close external recording, rolls off below ~120 Hz | 66 s | Wikimedia Commons, `File:Maserati GranTurismo S Exhaust.ogg`; author lmartins (via Freesound); CC BY 4.0 |
| `bmw_m70_v12_idle.ogg` | BMW M70 5.0 L V12 idling (~720 rpm), recorded close to the engine | 4.4 s | Commons, `File:M70.ogg`; author Johannes Maximilian; CC BY-SA 4.0 |
| `chrysler_225_slant6.ogg` | 1965 Chrysler Valiant 225 slant-six: cranks, fires, idles at ~1270 and ~1680 rpm, shuts off; near-field | 9 s | Commons, `File:225 Slant Six.ogg`; author Scheinwerfermann; CC BY-SA 4.0 |
| `trophy_truck_and_v8s.ogg` | Jesse James' Trophy Truck idling and other American V8s, Goodwood Festival of Speed 2009; spectator distance | 82 s | Commons, `File:Trophy Truck and American V8s.ogg`; author Edvvc; CC BY-SA 3.0 |

Decode them with `tools/sound/oggdec` (a 40-line Rust converter on the `lewton` crate that is already in the Bevy
dependency tree) and look at them with the Python tools next to it.

What the recordings are and are not: four different cars, microphones, distances and recorders. They fix the shape of
real spectra (envelope slope, how noisy the top end is, how clean the harmonic lines are, how the top end grows with
load) but not the absolute level or the low end (two are high-passed or near-field), and none is a measured, calibrated
exhaust test. The Maserati, the only external, exhaust-dominated recording of a V8, is the primary target; the others
check that the same effects show up elsewhere.
