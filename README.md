# engine-sim

A real-time piston-engine + drivetrain simulator in Rust, with a procedural 3D visualization of the
whole powertrain: crankshaft, pistons, rods, valvetrain, cams, ignition, intake, exhaust, turbo,
clutch, gearbox, driveshaft, differential, wheels and brakes. Ported from the roblox-ts prototype
(`../../roblox/typescript/engine`), then pushed far beyond what Roblox can do: fine crank-angle
stepping, per-angle combustion recording, synthesized engine audio and a PBR render.

```
cargo run --release -p viewer                     # V8, default
cargo run --release -p viewer -- --engine i4turbo # v8 vtwin v6 v12 v16 i6 i4 i4turbo w16 w24 dieseli6
cargo test --release -p engine-core               # physics regression + dyno + ECU/EVAP + acoustics validation
cargo test --release -p viewer                    # live sound path tests (every engine starts, idles on its firing note...)
cargo run --release -p engine-core --example soundlab -- steady --engine v8 --rpm 3000   # sound lab (see Audio)
```

## Layout

| crate | what |
|---|---|
| `crates/engine-core` | Headless sim, no graphics/audio deps (the sound model is plain Rust here too: `acoustics/` is the compressible-flow exhaust and intake solver, `sound/` the radiation and mechanical layers). 0-D single-zone thermodynamics: slider-crank volume, polytropic compression, Wiebe heat release, per-cylinder ignition faults (misfire + backfire), turbo spool, knock, coolant thermal model, finite fuel, closed-loop O2 fuel trim, catalyst/muffler backpressure, EVAP system, an OBD-style ECU (misfire monitor, O2/trim monitors, EVAP leak test, check-engine lamp), clutch-slip gearbox and road-load drivetrain. `tuning.rs` holds the bolt-on parts (intake, exhaust, turbos), `dyno_curve` runs a headless full-throttle sweep. |
| `crates/viewer` | Bevy 0.19 + egui front end: procedural meshes, animation, effects, HUD, audio. |

`tools/sound/` is the analysis kit for the sound model (spectrograms, comparison with the real recordings in
`crates/engine-core/reference/`, an artifact scanner; see the Audio section and its README).

The physics is a faithful port of the TypeScript sim (same constants, same structure) and is
validated against it: the V8 lands within 2% of a real LS3's 430 hp, the 1.4 L turbo I4 at 152 hp,
all 11 engines idle and rev (`crates/engine-core/tests`).

## Controls

`E` hold to crank (or the START button) - `W` throttle - `S`/`Space` brake - `X`/`Z` shift up/down
(auto-shift is on by default) - `N` neutral - `R` reverse - `K` kill engine - `C` cycle camera focus
(engine / turbo(s) / belts+cooling / intake / clutch+gearbox / driveshaft / differential / wheel+brake / exhaust / full)
- `O` orbit - mouse: left orbit, right pan, wheel zoom - `[` `]` slower/faster time - `P` pause -
`Tab` hide HUD - `L` name the parts in the 3D view - `F1` lessons. The toolbar at the top shows and hides each panel
(ENGINE, TUNING, ANALYSIS, TELEMETRY, FAULTS), and every panel can be dragged. The FAULTS panel injects ignition,
catalyst, O2 sensor, muffler and EVAP faults.

## Learning tools

The viewer is meant to teach as well as to play with. Every diagram reads the same per-angle tables, valve timing and
telemetry that drive the 3D rig and the sound, so the pictures cannot disagree with the engine.

- **LEARN** (toolbar or `F1`): ten guided lessons - first drive, the four-stroke cycle, valve timing and overlap,
  air / vacuum / the throttle, firing order and sound, exhaust tuning, turbocharging, torque / power / gearing, the
  drivetrain, and fault finding. A step sets up the engine and camera, explains what is on screen and mostly ends with
  something to try that is ticked off live (`Goal`s in `learn.rs`: "rev above 3000", "make 0.5 bar of boost"...).
- **ANALYSIS -> Cycle**: one cylinder on a 720 degree timeline - cylinder pressure (coloured by stroke), burn fraction,
  the intake and exhaust valve lifts with IVO / IVC / EVO / EVC and the overlap, the spark - with a plain-language
  caption of what the cylinder is doing right now (it notices misfires and a stopped engine) and a strip of every
  cylinder in firing order, coloured by its stroke. **Click or drag the timeline to turn the crank by hand**: the
  piston, valves, flame and spark in 3D follow. The selected cylinder glows in the colour of its stroke, and numbered
  badges over every cylinder show all of their strokes at once.
- **ANALYSIS -> P-V**: the pressure-volume loop of the selected cylinder (linear, log-log or zoomed to the pumping
  loop), filled green where the gas pushes the piston and red where the piston has to pump (throttle vacuum;
  a turbo turns it green), with gross work, pumping work, net work and IMEP per cycle.
- **ANALYSIS -> Dyno / Power**: labelled torque and power curves against the stock engine, and where the fuel's energy
  goes (shaft / friction / heat) with BMEP, mean piston speed, specific power and brake thermal efficiency.
- **Labels** (`L`): names parts in 3D (piston, valves, cams, plug, turbo, cat, gearbox...); hover one for a description
  with live values ("intake valve: 62% open, opens 16 deg before TDC", "turbo: spool 71%, boost 0.8 bar"), click to pin
  the card, double-click to fly the camera to it. With labels off, hovering a part still identifies it.
- **Tooltips** on every control and reading, a **glossary** (`?`), a one-line **coach** above the drive controls that
  explains what is going on (rev limiter, knock, overheating, check-engine lamp, engine braking), **UI size** (`A-`
  `A+`) and a welcome card (`Start the engine` / `Guided tour` / `Explore`; "don't show again" is kept in
  `%APPDATA%/engine-sim/prefs.txt`).
- The valve events drawn here and animated in the 3D valvetrain are `engine_core::acoustics::valve_train`, the same cam
  the sound model flows gas through, so they differ per engine (the V12 has more overlap than the V8, the diesel almost
  none). The P-V work reproduces the engine's own indicated torque to within a few percent
  (`cycle::tests::indicated_work_agrees_with_the_engines_own_indicated_torque`).

## Tuning: swap intake, exhaust and turbos

The TUNING panel (top, beside ENGINE SIM) bolts parts onto the stock engine. Every part changes the
sim for a physical reason *and* the 3D rig, which rebuilds around it (sliders rebuild when released):

| part | options | what it does |
|---|---|---|
| Air filter | stock airbox, cold-air cone, open velocity stack | flow at high rpm, inlet temperature |
| Manifold | long runners, stock, short runners, individual throttle bodies | moves and reshapes the volumetric-efficiency hump (torque vs top end) |
| Header | stock log manifold, shorty, long-tube | scavenging, backpressure, collector geometry, resonances |
| Catalyst | stock, high-flow, delete | backpressure; a deleted cat also pops louder |
| Muffler | stock, sport (dual tips), straight-through, delete | backpressure and the real chambers, baffles and perforated tubes of each can (see Audio) |
| Pipe diameter | 85-135 % | backpressure, and the diameter of every duct the sound model simulates |
| Pipe / header metal | mild steel, 409 / 304 stainless, titanium, Inconel, cast iron | wall heat loss and so gas temperature (hence pitch), shell ring and radiated "tinniness", damping |
| Wall thickness | 50-200 % | heat capacity and shell stiffness: thin walls ring and cool faster |
| Header wrap | on / off | insulates the primaries: hotter gas, higher resonances, damped shell |
| Primary length / diameter | 60-160 % / 80-130 % | header tuning (the scavenging pulse arrives at a different rpm), primary resonances |
| Mid-pipe / tailpipe length | 50-180 % / 40-200 % | the pipe resonances (the cruise drone) and where the pulses land |
| Dual exhaust crossover | none, H-pipe, X-pipe | balance tube or merge-and-split between the banks: pitch, smoothness |
| Turbos | none, 1-4 (never more than cylinders) | spool rate, how long boost holds before the compressors choke |
| Turbo size | 0.6 (quick) - 1.6 (big) | lag vs top-end flow |
| Boost | 0.2-2.2 bar wastegate | manifold pressure, charge heating, knock and EGT |
| Intercooler | none, stock, large front-mount | charge temperature: density and knock margin |

Stock turbo engines keep their stock hardware (the W16 has its four turbos, the V6 its two); an
untouched `Tuning` reproduces the reference dyno figures exactly (`stock_tuning_is_exactly_the_stock_engine`).
Presets: Stock / Street / Sport / Turbo kit / Race. More boost needs more octane: the HUD flags KNOCK, so
swap to E85 or Race 100 in the ENGINE SIM panel.

Header, mid-pipe and tailpipe lengths, every diameter, the metal and wall gauge are not cosmetic: they are
the geometry and boundary conditions of the compressible-flow solver that makes the exhaust sound (below), and the
engine model's backpressure and scavenging follow the same numbers.

The DYNO chart in ANALYSIS is measured, not scraped from your driving: a worker thread re-runs a headless
sweep (`dyno_curve`) a moment after any change and overlays it on the stock curve, with peak figures.

Turbos are laid out in two mirrored columns at the front of the engine (compressors face outward so each
has room for its own filter), stacked in tiers for three or four, and fed from the collector that
actually sits next to them. Per cylinder gas-flow particles follow the real runner -> collector -> turbine
-> downpipe -> cat -> muffler path of whichever parts are fitted.

## How it stays readable at 6,000 rpm

An engine at redline turns ~100 times a second, so a 60 Hz render would strobe. The sim is stepped in
~1 degree crank increments and every cylinder's pressure / burn fraction / spark / misfire / backfire is
recorded into a per-crank-angle table. The renderer plays back from those tables at a *visual* crank
speed (default capped at 420 rpm; the HUD shows the playback ratio), so pistons, valves, cams,
combustion glow, gas-flow particles and sparks all stay in true phase with each other at any speed.
Set "visual cap" to `real` for true-speed, or slow time down to 0.01x.

Rendering: HDR + bloom, studio-lit procedural environment map, cascaded + contact shadows, screen-space
ambient occlusion, SMAA, per-pixel motion blur (rotating parts smear naturally at real speed) and depth
of field that tracks the orbit distance. The car stands in a cyclorama (gradient backdrop, polished floor
that mirrors the engine glow, cyan light strips that scroll with vehicle speed), on a ladder frame with
engine and gearbox mounts, behind a finned radiator with a spinning shrouded fan, and the engine carries a
serpentine belt drive (alternator, A/C compressor, water pump, tensioner) whose pulleys and belt marks
turn in step with the crank.

Burbles and overrun pops show at the tailpipe: each pop the synth fires (`pops_fired` in `AudioState`) sends
a jet of grey exhaust gas out of the tip, lit by a flame when the pop is strong, with a flash of tail light.
A cat or muffler delete makes them bigger. Without sound (`--nosound`, `--shot`) the sim mirrors the synth's
lift-off gate to fire the same pops, so `--lift-at SECONDS` in a `--shot` run catches them.

Parts that block the view fade to ghosts (`seethrough.rs`): rays from the camera to the orbit target find
what is in the way, and those parts (plus the small parts touching them, e.g. a radiator's tanks and rails)
get their own translucent copy of their material, solidifying again when the view clears. Toggle it in the
ENGINE SIM panel or start with `--no-seethrough`.

Visible processes: a spark arc at each plug, port-injector fuel spray (diesel: in-chamber plume), a
flame front that spreads from the plug across the chamber (persisting a few frames so a ~20 ms burn is
visible), exhaust-valve blowdown puffs, manifold flash and tailpipe fireballs with embers and soot on
backfire / decel crackle, and a continuous exhaust plume (white vapour cold, black soot when rich).
Billboard particles live in `viewer/src/puffs.rs` and age in time-scaled time, so slow-mo stretches them.

## Layout is derived, not invented

The sim only knows firing offsets. `viewer/src/layout.rs` recovers the physical engine from them:
cylinders that share a crank throw end up with matching pin angles, so the crankshaft, banks and rod
pairing (e.g. the LS 1-2/3-4/5-6/7-8 throws of a cross-plane V8) come out of the sim data.

## Audio

The sound is not a sample or a synth: it is a fluid-dynamics simulation of the gas in the exhaust and intake,
rendered through a radiation model. Every pipe length, diameter, material, temperature and pressure you
see in the TUNING panel is a real input to it.

**The gas path** (`engine-core/src/acoustics/`)

- *Pipes*: a quasi-1-D compressible Euler solver (MUSCL-Hancock, HLL fluxes, shock-sensing slope blend, AVX2/FMA
  kernels) for every duct of the manifold -> primaries -> collector -> cat -> mid-pipe -> muffler -> tailpipe path,
  with true area changes, tapers, wall friction, and gas properties that vary with temperature (the speed of
  sound follows the gas, so the pitch of every resonance does).
- *Heat*: every cell has a wall temperature. Gas-to-wall convection (Dittus-Boelter with a pulsation factor),
  conduction through the wall, outside convection (still engine-bay air or road airflow), radiation and header
  wrap, with the metal's density, heat capacity, conductivity and emissivity. A cold exhaust sounds lower and
  rises in pitch as it soaks; pipe gas cools from ~800 C at the head to ~500 C at the tip at 3500 rpm.
- *Junctions*: collectors, crossovers, valves and necks are one stagnation-pressure Newton solve per node
  (exact isentropic simple-wave relation per leg, loss coefficients for contraction / expansion / merging), so
  waves reflect and transmit as plane-wave theory says and stay nonlinear at blow-down amplitude.
- *Open ends*: radiate through the Levine-Schwinger end correction (0.61 a), as an inertial slug of air in
  parallel with the radiation resistance, so the open-pipe note includes the end correction.
- *Cylinders*: slider-crank volume at audio rate and the engine's real valve timing and lift drive orifice flow
  between each cylinder and its port, in both directions (reverse flow, overlap and scavenging happen by themselves).
  At exhaust-valve opening the cylinder keeps the mass the intake delivered and takes the burned-gas temperature
  from the engine model, so a throttled engine blows down gently, a boosted one violently.
- *Mufflers*: stock, sport and straight-through cans are built from real chambers, baffle plates, Helmholtz necks
  and perforated tubes (`exhaust.rs`), not filters; the transmission loss falls out of the geometry.
- *Intake*: the same solver for runners, plenum, throttle (PI-controlled to the engine's manifold pressure) and
  airbox/snorkel, drawing air through the intake valves.
- *Pops and burbles*: overrun burble and backfire are armed energetic blow-downs (unburnt charge lighting in the
  hot pipe), so they go through the same pipes: a cat or muffler delete changes them for the right reason.

**Radiation** (`engine-core/src/sound/`): each tailpipe is a monopole (`p = 1.4 rho dQ/dt / 4 pi r`), plus Lighthill
jet noise from the exit velocity, plus the *shells*: each pipe body (header, cat, can, tail) rings in its own
ovalling modes, so material, gauge and wrap change the metallic part of the sound; then intake noise, the
mechanical layer (starter, valves, gears, turbo whistle, knock) and a slow loudness compressor and limiter.

A worker thread runs the solver ahead of the audio device through a lock-free ring buffer. The solver has its own
clock: each step is as long as the CFL limit allows (in hot exhaust 1 to 5 audio samples), and the flow at every tailpipe
is kept as a cubic-Hermite curve through the solver's outputs, differentiated per audio sample, so the radiated
`dQ/dt` is continuous (the straight-line slope between outputs is a staircase whose steps repeat at the solver
rate: a narrow-band tone at the top of the spectrum, about 60 dB SPL at 21 kHz before). The monopole output is band-limited
to 2.8 kHz (fourth-order Butterworth): the 1-D solver resolves no wave motion above a few kHz, and what it radiates up
there is the steepened front of every blow-down pulse spread over a handful of cells - a train of sharp spikes (about
110 a second at idle on a V8, crest factor 12-34 in the 2-8 kHz band: a tick and a crackle that no real engine has).
The top end is supplied by the layers below instead.

**Top end: valve-jet noise and engine noise** (`sound/flow_noise.rs`, `sound/mechanical.rs`). Real engines are *noise*
above ~1.5 kHz, stationary Gaussian-like noise (the envelope of a 2-8 kHz band has an rms/mean of ~0.55 and a crest
factor near 4, with only a few percent of modulation at the firing rate; see the comparison below), and its level
climbs steeply with load. Three sources make it here:

- *Valve jets.* Every valve is a nozzle: the jet speed follows from the pressure ratio the cylinder has across it (the
  exhaust valve opens on 3 to 5 bar of burned gas against the port) and the mass flow the network gives it; a jet's
  acoustic power is a small fraction of its kinetic power that grows as Mach number to the fifth (`jet_power`). That power
  is delayed by the path to the mouth, smeared by the pipe's reverberation, taken down by what is fitted (cat 12 / 6 / 0 dB,
  stock / sport / straight / no muffler 26 / 14 / 8 / 0 dB, an absorptive airbox 24 dB, a turbine wheel 6 dB; what is left
  is darker the more loss there was) and becomes noise with a turbulent-jet spectrum at the listener. A cat and muffler
  delete therefore rasps for the physical reason. On the intake side the same valves make induction noise, gated by the
  throttle: a choked throttle at idle is a wall to sound travelling back up the intake.
- *Engine noise*: combustion, bearings, gears, chain, injectors; stationary broadband noise at a level that follows
  rpm^1.1 and a little the load (diesels 1.6x).
- *Cycle-to-cycle combustion scatter*: each cylinder blows down a charge that differs from the last by a few percent
  (7 % at idle, 2 % under load) plus its own slowly wandering offset, so the exhaust pulses are never exact copies and
  the harmonic comb is not razor sharp. Pure sines are gone from the machinery as well: the turbo whistle and the gear mesh
  wander in frequency and flutter in level (the gear tone has the shaft-rate sidebands of a real mesh).

The efficiency of the jets, the losses of the parts and the level of the engine noise are estimates calibrated against
the recordings below (for a stock car the jet efficiency and the stock losses trade against each other; the rasp of a
deleted exhaust follows from the losses assumed for the parts).

**Solver quality.** The quality selector under the TUNING panel's EXHAUST ACOUSTICS controls has a ladder of five
tiers, ultra / high / balanced / fast / eco (20 / 28 / 40 / 55 / 70 mm cells), and *auto*. Cell size matters more than it
sounds: the fronts of the blow-down pulses are a few cells thick, so a coarse grid takes the top end off the exhaust
note (the solver is heard up to 2.8 kHz; above that the valve-jet layer is the same on every tier). V8 at 3500 rpm WOT,
1/3-octave levels at the tailpipe against the converged 14 mm grid (dB): at 1.3 kHz
-6 on 28 mm cells and -18 on 50 mm, at 1.6 kHz -9 and -28, at 2 kHz -13 and -29; pitch and the low end are identical on
all of them. Overall (dB(A) at 1.5 m) 94.8 / 93.3 / 92.5 / 87.7 / 85.8 from ultra to eco.

*Auto* (`AutoQuality` in `viewer/src/audio.rs`) starts every engine on balanced, watches the fraction of real time
the worker spends rendering, and moves to the finest tier that keeps that near 40 %: it steps coarser (several
tiers at once if need be) when the load stays above 75 % for a second, finer when it stays below 28 % for a few seconds
(a tier costs about 1.6x the next coarser one), never back into a tier that has just overloaded for the next three
minutes. A switch (and a part fitted in the TUNING panel) does not drop the sound out and bring a cold model in: the
replacement is built beside the current one, started from its crank angle, speed glide and compressor state, runs in step
with it for 0.25 s while its pipes settle and is cross-faded in over 40 ms (a cold model dropped in place was a 40 ms
dropout followed by a +4 dB surge). If the audio device ever outruns the worker the output does not cut to zero (two
clicks) but holds the last sample and fades back to the signal over 4 ms. A fixed tier can be
pinned with the buttons. On the development machine (12 threads) the light engines (i4, i6, V-twin, diesel) settle on
ultra at ~30 % of one core, the V6/V8/V12 on high or balanced, the W24 on balanced.
Single-core speed at 3500 rpm WOT (`soundlab steady --rpm 3500 --throttle 0.8 --quality ultra`, times real time):

| | ultra | high | balanced | fast | eco |
|---|---|---|---|---|---|
| i4 | 2.5 | 3.9 | 5.2 | 6.9 | 10 |
| v8 | 1.2 | 1.6 | 2.4 | 3.7 | 4.5 |
| v12 | 1.0 | 1.6 | 2.2 | 2.9 | 3.8 |
| w24 | 0.6 | 0.9 | 1.3 | 1.7 | 2.6 |

**Starting and stopping.** The starter (`sound/starter.rs`) is built from its parts instead of a tone: a click for
every ring-gear tooth that passes the pinion (teeth x crank speed, with tooth-to-tooth scatter and a wandering rate so
the harmonics blur to a rasp), ringing the starter nose and bell housing; commutator and brush hiss pulsed at the segment
rate; a weak, wandering armature whine; the solenoid clack and pinion grind when it throws in and the clunk and
free spin-down when it lets go. Its pitch is the crank's *instantaneous* speed: the sound model integrates the gas torque
of its own cylinders (compression resists, expansion pushes) against the engine model's starter torque curve and
crank inertia, so the whirr sags and surges with every compression stroke (peak-to-peak 10 % on an i4, 3 % on a V8, as in
the engine model; `cranking_sags_and_surges_like_the_engine_model` checks it). After light-off the first firings are
uneven for about a second. With the engine off (`activity` fades the radiation out 0.2 s after the crank stops) the output
is digital silence: the solver's round-off noise, which the loudness compressor would otherwise lift to -65 dBFS, no
longer reaches the speakers. The intake and the exhaust also share one ambient pressure now (the engine model's
atmosphere of 105 kPa at 33 C against the pipe model's 101 kPa had blown a 3 m/s draught through the stopped engine).

The EXHAUST ACOUSTICS controls and the readouts under them show the live result: path length, the quarter-wave
drone frequency of the pipe, the rpm the primaries tune to, gas temperature at the header and tip, the solver's
cost. `SOUND_LOG=1` prints a summary line to the console every two seconds; `--sound` keeps the sound running in `--shot` runs.

**Compared with real engines.** Four freely licensed recordings (`crates/engine-core/reference/`, sources and licenses in
its README) are the yardstick: a Maserati GranTurismo S V8 (external, idling and revving with throttle blips), a BMW M70
V12 idling, a Chrysler slant-six cranking, firing and idling, a Trophy Truck and other V8s at a festival. They are four
different cars, microphones and distances (two are near-field, one is high-passed below ~120 Hz), so they fix the *shape*
of real spectra, not absolute level or the low end; the Maserati is the primary target. `tools/sound/compare_refs.sh`
renders the model at matching operating points and prints the numbers below (`tools/sound/` has the analysis kit:
spectrograms, overlays, band and line statistics, an artifact scanner).

What the first comparison found, and what was done about it (V8 idle and rev against the Maserati; the V12 and I6 idle
against their recordings behave the same way):

| measured | before | real | now |
|---|---|---|---|
| rev, 2-10 kHz, 1/3-octave level re 60 Hz-8 kHz power, sim minus real | -11 dB at 2 kHz, -24 to -35 dB at 4-10 kHz | | within -1.4 .. +3.1 dB over the whole band, -3.5 .. +0.6 dB from 0.8 to 1.6 kHz |
| top end against load (2-5 kHz re the 150-700 Hz band, V8) | idle -36 dB, dipping to -54 dB at mid rpm | idle -25 to -30, revs -16 to -36 dB (median -28) | idle -28 dB, revs -19 to -23 dB, rising with rpm and load without a dip |
| envelope of the 3-9 kHz band at idle (rms/mean, crest) | tailpipe, 4-8 kHz: 2.8 / 34 (sparse spikes, two per firing) | M70 and slant-six 0.55-0.56 / 3.9-4.5 | 0.52 / 4.0 |
| same on the live drive (idle, part throttle, WOT, coast) | up to 1.8 / 40 | | 0.52-0.55 / 3.5-4.8 |
| 400-1000 Hz line structure, spectral flatness of 93 ms frames (white noise -2.5 dB) | V8 -9.3, V12 -9.9, I6 -11.3 dB | -3.6 to -9.0 dB | V8 -7.7, V12 -8.1, I6 -7.7 dB |
| formant ripple of the long-term spectrum of a rev (rms around its trend) | 3.8 dB | 2.0-2.9 dB | 3.1 dB (the pipe resonances were not too sharp) |
| V12 idle against the M70, 1/3-octave at 50-100 Hz and 3-8 kHz | | | within ±2 dB (its own resonances differ at 250-320 and 635 Hz, +6 to +10 dB) |

Left open: the idle top end of every engine is still 3 to 22 dB under the recordings (V8 3 dB, V12 8 dB, I6 22 dB; the
M70 and slant-six microphones are at the engine); the harmonic lines of the model at idle are still
a little cleaner than the closest-miked recordings; the within-cycle speed ripple of the crank is not in the sound (the
starter has it) and the idle rpm of a steady render does not hunt as a real engine's does (the lines of the recordings
wander by 1-2 %).

The pipe model itself is checked against theory and published figures, all in the test suite (`cargo test --release -p engine-core`):

| check | result |
|---|---|
| closed-open pipe modes (c/4L with end correction) | 70.6 / 211.8 / 353.0 / 494.2 Hz predicted, 70.6 / 211.8 / 353.1 / 494.4 simulated |
| open-open pipe modes with both end corrections | 166.5 / 333.0 / 499.5 / 666.0 predicted, 166.5 / 333.1 / 500.0 / 667.0 simulated |
| fundamental vs gas temperature | 293 K 83.9 Hz, 500 K 108.6 Hz (theory 108.5), 900 K 143.5 Hz (theory 143.4) |
| hot pipe in cold air | 127.4 Hz with hot surroundings, 120.1 Hz in 293 K air (the cold end correction lowers it) |
| area step reflection / transmission | R -0.598 / T 0.398 for A2/A1 = 4 (theory -0.600 / 0.400); R 0.597 / T 1.592 for 0.2 (0.600 / 1.600) |
| expansion-chamber transmission loss | 13.3 dB at the first peak (plane-wave formula 13.2), worst deviation 1.7 dB over the band |
| Helmholtz resonator, side-branch notch | formula 145.7 / 111.5 Hz, simulated 145.2 / 113.0 Hz (notch -37 dB) |
| Sod shock tube, mass and energy conservation, rest state in a cone | pass |
| numerical damping of travelling waves | 0.10 dB over 6 m at 400 Hz, 1.4 dB at 800 Hz (43 / 21 cells per wave) |
| time-domain solver vs an independent frequency-domain transfer-matrix solver | muffler transmission loss correlation 0.95 / 0.80 / 0.98 (stock / sport / straight-through); the strongest exhaust resonances agree to a few % (i4 356 vs 363 Hz, v8 349 / 355 vs 355 Hz), but the transfer matrix lists more small peaks than the engine excites |
| header tuning, `L[in] = 850 ED / rpm - 3` | primaries of 0.56 m (formula 8540 rpm) and 0.71 m (6946 rpm) show their overlap-pressure minimum at 8500 and 7500 rpm: within 0.5 % and 8 % of the formula; shorter pipes tune beyond the sweep |
| mufflers (mean transmission loss 100-1000 Hz) | stock 18.6 dB, sport 11.3, straight-through 10.6 (production 20-40, chambered 10-30, glass-pack 5-20 dB: stock sits a little low) |
| levels at 1.5 m (V8 6.2 L) | idle 74 dB(A), 6000 rpm WOT 94 dB(A) |
| gas temperatures (V8, 3500 rpm, soaked) | 790 C at the primaries, 700 C after the cat, 510 C at the tip |

Known limits: the open-pipe case (cat and muffler deleted) reads about 123 dB(A) at 1.5 m, probably 5-10 dB louder than
a real street car on straight pipes, and its top end, 40 dB above the stock car's, rests on assumed part losses; the pipes
are plane-wave (1-D), so above ~3 kHz, where the cross modes of a 65 mm pipe cut in, the top end is the valve-jet and
engine-noise layers rather than the gas solver; combustion noise has no cylinder-pressure model behind it (the cylinder
reservoirs have no heat release), only the engine-noise level law; the jet efficiency, the part losses and the engine
noise are calibrated against four recordings, not measured; coarse quality tiers lose some level between 1 and 2.8 kHz.

**Sound lab** (`engine-core/examples/soundlab.rs`) renders without the viewer and reports what came out:

```
soundlab steady     --engine v8 --rpm 3000 --throttle 0.8 [--tune "..."] [--quality ultra|high|balanced|fast|eco] [--shells] [--cells] [--bands] [--abs]
soundlab start      --engine i4turbo [--hold 2.4] [--cold]            # key turn: starter, hold-off, light-off, settle (WAV + spectrograms)
soundlab off        --engine v8 [--hot] [--trace]                      # what a stopped engine still says (must be nothing)
soundlab sweep      --engine v8 --from 900 --to 6400 --secs 12      # WAV + spectrogram of a rev
soundlab lift       --engine v12 --from 5500 --to 2200               # overrun burble
soundlab thermal    --engine v8 --rpm 3500                           # gas and wall temperature along the pipe
soundlab resonances --engine v8 --rpm 4000                           # transfer-matrix resonances, and the rpm where each meets a firing harmonic
soundlab compare    --engine v8 --rpm 3000 --ref recording.wav       # 1/3-octave, centroid and line comparison with a real recording
```

`steady --dump` also writes every source (`tail jet shell mech intake final`, plus the crank angle) as raw little-endian
f32 for the Python tools. `cargo test --release -p viewer render_drive_scenarios -- --ignored --nocapture` renders
scripted drives (start, idle, launch with auto-shift, lift-off, braking) through the live path, `render_model_swap` a
tier hand-over; `tools/sound/scan.py` looks for clicks, discontinuities and stray tones in the result.

`steady` prints the strongest spectral lines annotated with engine orders and firing multiples, 1/3-octave levels,
dB(A), per-source levels (tailpipe, jet, shells, machinery, intake) and where the solver time goes. `compare` is how
to check this model against an actual car: record it steady at a known rpm and point `--ref` at the file (each
spectrum is normalised to its own total power, since the gain of a recording is unknown).
`audio-samples/` holds renders: `idle_*`, `rev_*` (every engine) and `decel_*` (overrun burble) with their
spectrograms; the previous synth's renders are in `audio-samples/old-synth/` for A/B.

## Automation / screenshots

`--shot out.png --after 8 --preset N --gear G --throttle T --timescale S --yaw/--pitch/--dist --hideui
--engine ID --switch-engine ID --unplug CYL` runs a scripted start, captures a frame and exits
(mouse input is ignored in this mode, audio is off). To photograph the interface: `--ui engine,tuning,analysis,telemetry,
faults,learn,glossary,labels` opens those panels (without `--hideui`), `--tab cycle|pv|dyno|power`, `--pv log|pumping`,
`--cyl N`, `--scrub DEG` (turn the selected cylinder to that crank angle), `--lesson L:S` (start lesson L, 0-based, at
step S, running the earlier steps' set-up too), `--hover X,Y` (pretend the pointer is there) and `--welcome`.

`--tune "turbos=2,size=0.9,boost=1.0,filter=cold|open,runners=long|short|itb,header=shorty|long,cat=highflow|delete,muffler=sport|straight|delete,pipe=1.2,ic=none|large,pipemat=ti|ss304|ss409|steel|inconel,headermat=ti|ss304|ss409|steel|inconel|iron,wall=1.3,wrap=1,plen=0.8,pdia=1.1,mid=1.3,tail=1.5,cross=none|h|x"`
starts with those parts fitted (the same string works for `soundlab --tune`); `--tune-late "..."` swaps them in half way through the run, the way the
TUNING panel does.

## Not yet done

Bevy and the physics are both complete for single-engine use; ideas still open: cutaway toggles per
component, a real chassis/ground contact model, saving dyno runs, compound / sequential turbo staging,
and anti-lag / launch control on top of the new boost model.
