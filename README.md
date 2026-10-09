# engine-sim

A real-time piston-engine + drivetrain simulator in Rust, with a procedural 3D visualization of the
whole powertrain: crankshaft, pistons, rods, valvetrain, cams, ignition, intake, exhaust, turbo,
clutch, gearbox, driveshaft, differential, wheels and brakes. Ported from the roblox-ts prototype
(`../../roblox/typescript/engine`), then pushed far beyond what Roblox can do: fine crank-angle
stepping, per-angle combustion recording, synthesized engine audio and a PBR render.

```
cargo run --release -p viewer                     # V8, default
cargo run --release -p viewer -- --engine v8_50  # vtwin i4_14 i4_16 i4 i4_25 v6_30 v6_35 v6 i6_32 i6 i6_40 dieseli6 v8_50 v8 v10_50 v10 v10_80 v12 v16 w16 w24
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
validated against it: the 6.2 L V8 lands within 2% of a real LS3's 430 hp,
all 16 engines idle and rev (`crates/engine-core/tests`).

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

No engine ships with a turbo: every preset is naturally aspirated (several sizes per layout, e.g. the flat-plane 5.0 L V8 in
the style of the Jesko's, or the 6.2 L cross-plane one) and you bolt turbos on here; a diesel gets a sensible default turbo the
same way. An untouched `Tuning` reproduces the reference dyno figures exactly (`stock_tuning_is_exactly_the_stock_engine`).
Presets: Stock / Street / Sport / Turbo kit / Race. More boost needs more octane: the HUD flags KNOCK, so
swap to E85 or Race 100 in the ENGINE SIM panel.

Header, mid-pipe and tailpipe lengths, every diameter, the metal and wall gauge are not cosmetic: they are
the geometry and boundary conditions of the compressible-flow solver that makes the exhaust sound (below), and the
engine model's backpressure and scavenging follow the same numbers.

The DYNO chart in ANALYSIS is measured, not scraped from your driving: a worker thread re-runs a headless
sweep (`dyno_curve`) a moment after any change and overlays it on the stock curve, with peak figures.

Turbos are laid out in two mirrored columns at the front of the engine (compressors face outward so each
has room for its own filter), stacked in tiers for three or four, and fed from the collector that
actually sits next to them. Per cylinder, parcels of gas follow the real runner -> collector -> turbine
-> downpipe -> cat -> muffler path of whichever parts are fitted (see "Visible processes").

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

Visible processes: a spark arc at each plug, port-injector fuel spray (diesel: in-chamber plume), a turbulent
flame front that spreads from the plug across each chamber, the gas moving through the intake and the exhaust, and
everything that leaves the tailpipe: haze, steam, soot and the fire of a backfire. They are not sprites:

- *Tailpipe plume* (`volume.rs`, `plume.wgsl`). A few hundred Gaussian *blobs* carry the physics: thrown out of the
  pipe a puff per exhaust-valve opening (more of them the faster it flows, so they overlap into a jet), slowed by drag
  against the air and the car's own airflow, lifted by their heat, stirred by a divergence-free swirl, grown by
  entrainment, cooled by mixing, thinned as the steam re-evaporates. Every frame they are splatted (mass-conserving
  Gaussian kernels, stretched along their velocity) into a 3D texture of haze, soot and flame emission, and a ray
  marcher integrates it along each camera ray: the haze scatters the key and fill lights and shadows itself, soot
  absorbs, flames emit with the colour of a black body at their temperature (1850-2350 K at birth, cooling as they
  burn out) and light the smoke around them; fine turbulent structure is added from moving noise; the volume writes
  its own depth so depth of field does not blur it as if it were the wall behind it. What shows follows the engine:
  a trace of haze when warm, steam when the exhaust is cold, black smoke when rich or a diesel under load; the
  car's speed streaks the plume out.
- *Backfire and pops*. The charge lights at the mouth and the flame front runs out along the plume, so the fire is seen
  to leave the pipe: a fireball plus 16-26 tongues of flame (each lit a few ms after the one before it, each a
  streak along its own velocity, solid while young, breaking into sheets as it burns out), the smoke the burn leaves,
  burning droplets that streak across the frame as a shutter would see them (`puffs.rs`), a flash of tail light and a
  flash of the manifold glowing through its pipes. Diesels do not pop.
- *Flame in the chamber* (`chamber.rs`, `chamber_flame.wgsl`). The chamber volume is ray-marched too: a spark kernel
  becomes a wrinkled flame front (a thin white-yellow sheet with a blue skin) with the burned gas glowing behind it
  as a black body, pushed about by turbulence that moves with time, growing with the burned fraction the engine
  model recorded for that crank angle.
- *Gas in the pipes* (`gas.rs`, `gas.wgsl`). Parcels of gas follow each runner and pipe centre line - fresh charge, pale
  blue, in through every intake valve while it is open; burned gas out of every exhaust valve at its opening, glowing
  while it is hot and fading to grey as the pipe cools it - as soft Gaussian blobs (a camera-facing quad with a
  Gaussian falloff is the exact line integral of a spherical Gaussian volume), stretched along the flow and stirred
  sideways by swirl that grows downstream. All of them are one dynamic mesh and one draw call.

Everything ages in time-scaled time, so slow-mo stretches it. Developer hooks for judging it in screenshots:
`FX_TEST=backfire|pop` (with `FX_TEST_AT=seconds`) fires one on a timer, `FX_SLOW=0.04` runs only the fx clock slowly,
`NO_DOF=1` closes the aperture, `NO_PLUME=1`, `NO_GAS=1`, `NO_PUFF=1` and `NO_CHAMBER=1` switch one effect off, `PLUME_LOG=1` and `RIG_LOG=1` print the plume state and the cylinder positions.

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
- *Combustion rattle.* Every combustion thud rings a bank of six well damped block modes (520 Hz - 2.9 kHz): a block has
  hundreds of closely spaced modes, so what reaches the ear is a dull thud and not a tone (three sharp modes made a nasal
  640 Hz line that sat in every engine at every speed). The thuds scatter in strength (+-40 %) and in time (1.5 degrees rms)
  so the firing harmonics near a mode do not stand as clean tones. A diesel uses the same bank with a quicker rise, mostly the modes
  below 1.8 kHz and a bigger scatter (strength 0.5-1.5, timing 2.4 degrees rms: no two injections ignite alike); its first version fed three
  sharp modes with a regular pulse and was a nasal 640 Hz buzz at the firing rate with a tick train out to 5 kHz.
- *Many cylinders are never perfectly even.* From eight to sixteen cylinders (`gas_exchange::irregularity`) each cylinder's
  blow-down strength scatters up to 2.5x as much and carries a fixed timing error of up to 0.8 degrees (valve lash,
  cam tolerance, ignition): a perfectly symmetric sixteen is a bare firing line and its overtones, a whine. A W16 sends
  its first two banks (one VR8) down one side of the exhaust and the other two down the other.

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

**The tiers sound alike.** A tier change used to be a different engine (V8, 3500 rpm, tailpipe monopole against the finest
tier, mean over engines and speeds, `examples/tierlab.rs`): 40 mm cells -0.5 dB at 250 Hz, -4.7 at 1 kHz, -11 at 1.6 kHz;
55 mm cells -4 dB of *low end* (100-250 Hz), -8 at 500 Hz and -13 at 1 kHz. The heavy engines, which the auto policy
puts on the coarse tiers, therefore lost their low end and the body of the note, and every change of tier was a step in
the sound. A small bank of peaking filters per tier (`sound/tier_eq.rs`, gains fitted to those means and interpolated by
cell size) puts back what the grid smooths away: after it every tier is within about 1 dB of the finest below 1 kHz,
whatever the engine (the spread between engines is 1-3 dB; the test `the_tiers_of_the_solver_sound_alike` pins
it), and what the coarse grids cannot give above 1.5 kHz (up to -14 dB on the coarsest) is where the valve-jet layer
takes over anyway.

*Auto* (`AutoQuality` in `viewer/src/audio.rs`) chooses once and then keeps still. When an engine is selected a
throw-away model renders a third of a second at a hard working point (0.7 of the redline, nearly wide open) and the
tier is the finest whose predicted cost (a tier costs about 1.45x the next coarser one) stays under 50 % of a core (the benchmark is a hard working point; live, the same tier takes about 0.6 of it):
room for a hard pull and for the second model that runs beside the first during a hand-over. After that it changes only
when it must: a load above 80 % for 1.2 s steps down at once (several tiers if need be, to the one that would bring it to half a core);
a load under 20 % for 20 s, at least 45 s after the last change, steps up by one tier; never back into a tier that
has just overloaded for five minutes. A switch (and a part fitted in the TUNING panel) does not drop the sound out and bring a cold model in: the
replacement is built beside the current one and takes over its crank angle, speed glide, compressor and limiter state,
its cylinders' own offsets and *its thermal state, duct by duct and cell by cell* (a model that starts at its own idea of
the pipe temperatures has other resonances: the old swap of an i6 to the finest tier dropped the 30-200 Hz band by 7 dB),
and its noise generators, so the hiss and the rasp of both are the same sample for sample and cross-fade as one
signal instead of dipping 3 dB in the middle as two uncorrelated ones do; it runs in step for 0.25 s while its pipes
settle and is cross-faded in over 0.2 s. If the audio device ever outruns the worker the output does not cut to zero (two
clicks) but holds the last sample and fades back to the signal over 4 ms. A fixed tier can be
pinned with the buttons. Single-core speed at 3500 rpm WOT (`soundlab steady --rpm 3500 --throttle 0.8 --quality ultra`, times real time):

| | ultra | high | balanced | fast | eco |
|---|---|---|---|---|---|
| i4 | 2.5 | 3.9 | 5.2 | 6.9 | 10 |
| v8 | 1.2 | 1.6 | 2.4 | 3.7 | 4.5 |
| v12 | 1.0 | 1.6 | 2.2 | 2.9 | 3.8 |
| w24 | 0.6 | 0.9 | 1.3 | 1.7 | 2.6 |

**Starting and stopping.** The starter (`sound/starter.rs`) is built from its parts instead of a tone. A key turn heard from
outside is a rough, buzzing whirr that chugs, in the 100 Hz - 2 kHz octaves. (The first version was three thin tones that wobbled
with the compression strokes, an "alien" whine; the second matched the flat octave bands of two recordings, 800 Hz to 6 kHz, with a
wide noise floor, and read as gas rushing out: an airy hiss. `the_starter_is_a_low_mid_whirr_and_not_hiss` now pins the shape: the
100 Hz - 1.6 kHz octaves within 5 dB of each other, 3.2-6.4 kHz at least 6 dB and everything above at least 14 dB down.) The whirr is
the *armature*: the motor turns about thirteen times per crank turn and each turn is a pulse (commutator, brushes, poles) of its own
strength, ringing housing modes at 310 Hz - 1.5 kHz; over it a click for every ring-gear tooth that passes the pinion (teeth x crank
speed, with large tooth-to-tooth scatter in spacing and strength, through damped modes at 1-4.6 kHz), a light brush and fan hash
(pink noise from 160 Hz to 2.4 kHz) and a barely audible whine. All of it is louder against a compression stroke and quieter between
(`the_starter_chugs_with_the_compression_strokes`), plus the snap of the solenoid and the pinion grind when it throws in and the
clunk and free spin-down when it lets go. Its pitch is the crank's *instantaneous* speed: the sound model integrates the gas torque
of its own cylinders (compression resists, expansion pushes) against the engine model's starter torque curve and
crank inertia, so the whirr sags and surges with every compression stroke (peak-to-peak 10 % on an i4, 3 % on a V8, as in
the engine model; `cranking_sags_and_surges_like_the_engine_model` checks it). After light-off the first firings are
uneven for about a second. With the engine off (`activity` fades the radiation out 0.2 s after the crank stops) the output
is digital silence: the solver's round-off noise, which the loudness compressor would otherwise lift to -65 dBFS, no
longer reaches the speakers. The intake and the exhaust also share one ambient pressure now (the engine model's
atmosphere of 105 kPa at 33 C against the pipe model's 101 kPa had blown a 3 m/s draught through the stopped engine).

A switched-off engine that is still *turning* (the key off in a car that rolls on in gear, a stall on a hill) is only a
crank being dragged round: most engine noise is combustion, so the broadband engine noise falls to 10 % of its running
level within a tenth of a second of the last firing (`Mechanical::fire`), and nothing else (gas path, jets, intake) runs. It used to hiss
for as long as the wheels kept the crank turning, 25 dB under the running engine and lifted by the loudness compressor;
now it is 40 dB under it and a coast of 12 s decays to inaudible (`an_engine_that_is_switched_off_but_still_turning_is_nearly_silent`).

**Gas level.** The valve jets and the engine noise sit well below the level the recordings gave (`JET_GAIN` 0.12, `ENGINE_NOISE_PA` 0.011, both
shaped to 4.5 kHz instead of 6, the valve ticks 3.5 dB down; engines with more than eight cylinders get another 1.5-4.4 dB off both, because the
balanced exhaust of a W16 makes the quietest note under the same hash). Through the loudness compressor the engine as a whole is as loud as
before, but in a drive through the rev range the 4-10 kHz share is 6-9 dB down on the recording-matched level and 1.5-4 kHz 3-5 dB down: the airy hiss
that read as gas moving is gone and the note stands out of it. The pipe radiation has two 4th-order low-passes at 2.8 kHz (48 dB per octave).

**Hiss at high rpm.** The valve-jet and engine-noise levels were calibrated on a car revving in neutral; held for half a
minute at 5500 rpm in a high gear they sat at the loud edge of what the recordings allow and stood out of the note. Past 2500 rpm the
jets' power falls by up to 6 dB (exhaust) and 3 dB (intake) toward 5500 rpm and the engine noise by 3 dB. At full
throttle at 5000 rpm the 1.5-4 kHz band is now 3.5 dB lower and 4-10 kHz 4.5 dB lower than before, the tone unchanged.

**Low end** (`sound/lowend.rs`). An engine of perfectly matched cylinders has only firing harmonics, and for a fast six or twelve
(fundamental 250 Hz and more) that is one bright line and its overtones with nothing under it - the V6 at 5000 rpm had its
30-200 Hz band 19-25 dB under its body. Real engines have the low orders of the crank (0.5, 1, 1.5, 2 ... times per
revolution) from cylinder-to-cylinder differences, runner lengths, the shaking of the block on its mounts. The layer measures the
band under 150 Hz and the band 260-1500 Hz of what the engine already makes and adds just what is missing to hold the first at
-3 dB of the second at idle, -5 dB at 2000 rpm and -6.5 dB from 3000: orders phase-locked to the crank angle, each wandering in
level and phase (a rough rumble, not a hum). A cross-plane V8, a four or a diesel, which have their low end, get nothing; a
V12 at idle goes from -8 to 0 dB, the V6 at speed from -19..-25 to -10..-14 and the I6 from -14..-12 to -9..-7. Engines of 8 to 16
cylinders get a floor up to 7.5 dB deeper (3 dB at idle, all of it from 2000 rpm): sixteen near-identical firings are the furthest from
the matched idealisation, and a W16 had its 80-200 Hz band 6 dB under the W24's (whose unbalanced sides give it low orders for free) and
its note a hollow buzz on a 250-300 Hz header resonance. Its low band now sits within about 2 dB of the W24's and, against the real Veyron recordings (low end re body -2..+3 dB at idle and on the rev), at -2.5..+0.7 dB.

**Combustion, crank speed and rattle** (`sound/combustion.rs`). The pipe model's cylinders know nothing of the burn, so the sound model
now evaluates the engine model's own cylinder (polytropic compression and expansion, the Wiebe heat release, the same geometry, heat of
combustion and efficiencies) into a table over the 720 degrees of the cycle, once per snapshot of the engine state. Two things use it.
(1) *Torque.* A running engine's crank speed is the engine model's rpm plus what the combustion torque does to the crank (inertia, the idle
control and the load pulling it back at about 1.5 Hz): the within-cycle ripple (about 3 % peak to peak on a four at idle, 1 % on a V8, 0.5 % on a
W16) and, because every cylinder's burn scatters from cycle to cycle (more at idle), a slow wander of a few rpm that a perfectly steady idle does
not have. The crank angle the whole sound model runs on carries it. (2) *Rattle.* How hard a combustion hits the block follows the peak rate of
pressure rise the burn adds (bar per degree: 0.9 on a V8 at idle, 2.6 wide open, a diesel more), where it used to follow a fixed function of load;
the old level is kept (petrol idle = 0.72) and the dependence on charge, spark advance and a misfire is now physical.

**Turbo** (`sound/turbo.rs`). The compressor's shaft is its own state: it spools up in a tenth of a second on top of the engine model's lag and
*runs down over a second or two* (it keeps its momentum), so the whistle (a tone from 0.9 to 6.7 kHz that follows shaft speed,
roughened by the flow pulses at the firing rate) glides down instead of vanishing. An engine without a turbo reports no spool and makes no whistle (`an_engine_without_a_turbo_never_whistles`). What the charge pressure does when the throttle closes is a
tuning choice (`bov=stock|vent|none`, the "Blow-off valve" combo): a recirculating diverter gives a short soft hiss, a vented valve the loud "pssh" (about
-10 dB under the engine), and with no valve the compressor surges: a train of chirps, each a tone sweeping up by a factor of 2.5 and down again over
0.11 s, 3 to 10 a second while the trapped pressure bleeds away, with a thump each (the structure of the Celica recordings). Sound only: the dyno does not move.

**Burble and pops.** The pops are real blow-downs: a cylinder holding unburnt charge lights at its exhaust valve and the pipe model radiates it,
so a burble is low and round. (A version with bursts, a heavy-tailed strength and a separate high-frequency "crack" layer, scaled to the rally
recordings, was tried and sounded like a train of clicks; it was removed. `burble_is_low_and_round_not_a_click` keeps the high-passed envelope of an
overrun under 18 dB over its median, where the rally recordings reach 25-37.)

**Coarse solver tiers.** The two coarsest tiers carry 1.5-2.5 dB more correction at 320-1300 Hz than the per-tier means ask for: in the finished mix a W24
and a V8 on the 70 mm tier were still 2-5 dB under the 20 mm one at 200-1500 Hz, evenly over the speed range; they are now within 1-3 dB.


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
noise are calibrated against four recordings, not measured; the coarse quality tiers still lose level above 1.5 kHz (the per-tier correction is capped at +10 dB), where the valve-jet layer carries the sound; the low-end layer is a model of what real engines do, not of any one engine.

**Sound lab** (`engine-core/examples/soundlab.rs`) renders without the viewer and reports what came out:

```
soundlab steady     --engine v8 --rpm 3000 --throttle 0.8 [--tune "..."] [--banks 0,1,0,1,...] [--quality ultra|high|balanced|fast|eco] [--shells] [--cells] [--bands] [--abs]
soundlab start      --engine i4      [--hold 2.4] [--cold]            # key turn: starter, hold-off, light-off, settle (WAV + spectrograms)
soundlab off        --engine v8 [--hot] [--trace]                      # what a stopped engine still says (must be nothing)
soundlab sweep      --engine v8 --from 900 --to 6400 --secs 12      # WAV + spectrogram of a rev
soundlab lift       --engine v12 --from 5500 --to 2200 [--tune "cat=delete,bov=none"]  # overrun: burble, blow-off, flutter
soundlab ripple     --engine i4 [--rpm 800 --throttle 0]              # crank speed of a running engine: engine model against the sound model
soundlab thermal    --engine v8 --rpm 3500                           # gas and wall temperature along the pipe
soundlab resonances --engine v8 --rpm 4000                           # transfer-matrix resonances, and the rpm where each meets a firing harmonic
soundlab compare    --engine v8 --rpm 3000 --ref recording.wav       # 1/3-octave, centroid and line comparison with a real recording
tierlab --engines v8,i4,i6 --rpms 1200,3000,6000 [--corrected]              # how each tier differs from the finest, band by band
```

`steady --dump` also writes every source (`tail jet shell mech intake final`, plus the crank angle) as raw little-endian
f32 for the Python tools. `cargo test --release -p viewer render_drive_scenarios -- --ignored --nocapture` renders
scripted drives (start, idle, launch with auto-shift, lift-off, braking) through the live path, `render_model_swap` a
tier hand-over, `render_problem_scenarios` the top gear at full throttle, the key switched off in neutral and in gear;
`tools/sound/scan.py` looks for clicks, discontinuities and stray tones in the result, `ltas.py` gives the band balance by
engine and rpm, `timeline.py` level and bands over time.

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

`--tune "turbos=2,size=0.9,boost=1.0,filter=cold|open,runners=long|short|itb,header=shorty|long,cat=highflow|delete,muffler=sport|straight|delete,pipe=1.2,ic=none|large,bov=stock|vent|none,pipemat=ti|ss304|ss409|steel|inconel,headermat=ti|ss304|ss409|steel|inconel|iron,wall=1.3,wrap=1,plen=0.8,pdia=1.1,mid=1.3,tail=1.5,cross=none|h|x"`
starts with those parts fitted (the same string works for `soundlab --tune`); `--tune-late "..."` swaps them in half way through the run, the way the
TUNING panel does.

## Not yet done

Bevy and the physics are both complete for single-engine use; ideas still open: cutaway toggles per
component, a real chassis/ground contact model, saving dyno runs, compound / sequential turbo staging,
and anti-lag / launch control on top of the new boost model.
