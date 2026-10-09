//! Real-time engine sound.
//!
//! The sound itself is made in `engine_core::sound`: a compressible-flow model of the intake and exhaust
//! (see that module). This file only runs it: a worker thread owns the model, reads the engine state the
//! sim publishes every frame, renders audio ahead of time into a lock-free ring buffer, and Bevy's audio
//! callback drains that buffer. Keeping the solver off the audio callback means a heavy engine can never
//! glitch the mixer; if the worker cannot keep up it drops to a cheaper solver on its own.

use crate::sim::{AudioDiag, AudioState};
use bevy::audio::{AddAudioSource, AudioPlayer, ChannelCount, Decodable, PlaybackSettings, Sample, SampleRate, Source};
use bevy::prelude::*;
use engine_core::sound::{EngineSound, Quality, SoundInput, SAMPLE_RATE};
use engine_core::{engine_by_id, Tuning};
use std::num::NonZero;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Samples of look-ahead the worker tries to keep in the ring (about 90 ms).
const TARGET_FILL: usize = 4096;
const RING_LEN: usize = 1 << 15;
const CHUNK: usize = 256;

/// Single-producer single-consumer ring of f32 samples.
pub struct Ring {
    buf: Vec<AtomicU32>,
    head: AtomicUsize,
    tail: AtomicUsize,
    underruns: AtomicU64,
}

impl Ring {
    pub fn new() -> Arc<Ring> {
        Arc::new(Ring { buf: (0..RING_LEN).map(|_| AtomicU32::new(0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0), underruns: AtomicU64::new(0) })
    }

    pub fn len(&self) -> usize {
        self.tail.load(Ordering::Acquire).wrapping_sub(self.head.load(Ordering::Acquire))
    }

    /// Producer: appends as many samples as fit.
    pub fn push(&self, data: &[f32]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let free = RING_LEN - tail.wrapping_sub(self.head.load(Ordering::Acquire));
        let n = data.len().min(free);
        for (i, v) in data[..n].iter().enumerate() {
            self.buf[(tail + i) & (RING_LEN - 1)].store(v.to_bits(), Ordering::Relaxed);
        }
        self.tail.store(tail.wrapping_add(n), Ordering::Release);
        n
    }

    /// Consumer: next sample, or `None` (counted) when the producer has not kept up.
    #[inline]
    pub fn try_pop(&self) -> Option<f32> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            self.underruns.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let v = f32::from_bits(self.buf[head & (RING_LEN - 1)].load(Ordering::Relaxed));
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(v)
    }

    /// Consumer: next sample, or silence on underrun.
    #[inline]
    pub fn pop(&self) -> f32 {
        self.try_pop().unwrap_or(0.0)
    }
}

/// What the audio callback does when the ring runs dry. A hard cut to zero is a click on the way down and another on
/// the way back up; this lets the last sample die away over ~1.5 ms and, when samples return, cross-fades from where it
/// had got to over ~4 ms, so a starved buffer is a brief dip and not a pop.
#[derive(Clone, Copy, Debug)]
pub struct Declick {
    /// The last sample put out, the value held (and decaying) while starved, and the anchor the fresh samples fade in from.
    last: f32,
    hold: f32,
    /// 0 right after an underrun, rising to 1 as the fresh samples take over.
    mix: f32,
}

impl Declick {
    const DECAY: f32 = 0.985;
    const RECOVER: f32 = 1.0 / 192.0;

    pub fn new() -> Self {
        Declick { last: 0.0, hold: 0.0, mix: 1.0 }
    }

    #[inline]
    pub fn next(&mut self, fresh: Option<f32>) -> f32 {
        self.last = match fresh {
            Some(v) if self.mix >= 1.0 => v,
            Some(v) => {
                self.mix = (self.mix + Self::RECOVER).min(1.0);
                self.hold * (1.0 - self.mix) + v * self.mix
            }
            None => {
                self.hold = self.last * Self::DECAY;
                self.mix = 0.0;
                self.hold
            }
        };
        self.last
    }
}

#[derive(Asset, TypePath)]
pub struct EngineSynth {
    pub ring: Arc<Ring>,
}

impl Decodable for EngineSynth {
    type Decoder = SynthSource;

    fn decoder(&self) -> Self::Decoder {
        SynthSource { ring: self.ring.clone(), declick: Declick::new() }
    }
}

pub struct SynthSource {
    ring: Arc<Ring>,
    declick: Declick,
}

impl Iterator for SynthSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        Some(self.declick.next(self.ring.try_pop()))
    }
}

impl Source for SynthSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(1).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

pub struct EngineAudioPlugin {
    pub state: Arc<Mutex<AudioState>>,
}

impl Plugin for EngineAudioPlugin {
    fn build(&self, app: &mut App) {
        let state = self.state.clone();
        let ring = Ring::new();
        spawn_worker(Arc::downgrade(&self.state), ring.clone());
        app.add_audio_source::<EngineSynth>().add_systems(Startup, move |mut commands: Commands, mut assets: ResMut<Assets<EngineSynth>>| {
            let _ = &state;
            let handle = assets.add(EngineSynth { ring: ring.clone() });
            // ONCE, not LOOP: Bevy's loop wraps the source in rodio's `repeat_infinite`, which buffers 32768
            // samples (0.74 s) ahead of playback. This source never ends, so it needs no looping.
            commands.spawn((AudioPlayer(handle), PlaybackSettings::ONCE));
        });
    }
}

/// The state of the engine as the sound model wants it.
pub fn sound_input(a: &AudioState) -> SoundInput {
    SoundInput {
        rpm: a.rpm,
        throttle: a.throttle,
        load: a.load,
        map_pa: a.map_pa,
        atm_pa: a.atm_pa,
        air_k: a.air_k,
        charge_k: a.charge_k,
        afr: a.afr,
        egt_k: a.egt_k,
        boost: a.boost,
        spool: a.spool,
        running: a.running,
        lit: a.lit,
        cranking: a.cranking,
        knock: a.knock,
        vehicle_speed: a.vehicle_speed,
        gear_whine: a.gear_whine,
        burn: a.burn.clone(),
        master: a.master,
        serial: a.serial,
        turbo_speed: a.spool,
        burble: a.burble,
        backfire: a.backfire,
    }
}

fn quality_of(level: u8) -> Quality {
    Quality::level(level)
}

/// Picks the solver tier. A change of tier is audible - the finer solvers carry more of the 300 Hz-3 kHz body, and a
/// swap in the middle of a pull is a step in the sound however carefully it is faded - so the policy is to choose
/// *once*, from a short benchmark of the engine (`pick`), and to change afterwards only when it must: a sustained
/// overload steps down at once, a machine that has been idle for a long time steps up once in a while. The worker
/// feeds it the smoothed fraction of real time it spends rendering; it answers with a new ladder level (1 = finest)
/// when one is due. Finer tiers cost about `STEP_COST` times the next coarser one, which lets it jump several levels.
#[derive(Clone, Debug)]
pub struct AutoQuality {
    level: u8,
    /// The finest level worth trying: raised when a finer one proved too heavy, forgotten after a long while.
    floor: u8,
    floor_at: f32,
    changed_at: f32,
    over_since: Option<f32>,
    calm_since: Option<f32>,
}

impl AutoQuality {
    /// Load above which the solver is too heavy, for how long, and the load a coarser tier is aimed at.
    pub const OVERLOAD: f32 = 0.80;
    const OVERLOAD_FOR: f32 = 1.2;
    const AIM_DOWN: f32 = 0.5;
    /// Load below which a finer tier is worth trying, for how long (the benchmark is taken at a hard working point,
    /// so a calm machine is calm at every other one too).
    const CALM: f32 = 0.20;
    const CALM_FOR: f32 = 20.0;
    /// Quiet time after any change before the next one (up) is considered.
    const COOLDOWN_UP: f32 = 45.0;
    /// After a change the load estimate needs time to settle.
    const SETTLE: f32 = 3.0;
    /// What one tier finer costs, relative to the one above it.
    pub const STEP_COST: f32 = 1.45;
    const FLOOR_MEMORY: f32 = 300.0;
    /// The cost the benchmark aims at: well inside one core, so the model that is built beside the current one for a
    /// hand-over (which doubles the work for a moment) and a hard pull do not starve the ring. (The benchmark is a hard
    /// working point and the live load of the same tier is about 0.6 of it.)
    pub const AIM_COST: f32 = 0.50;
    pub const START_LEVEL: u8 = 3;

    pub fn new() -> Self {
        AutoQuality { level: Self::START_LEVEL, floor: 1, floor_at: 0.0, changed_at: 0.0, over_since: None, calm_since: None }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// The finest tier whose predicted cost stays under `AIM_COST`, given the measured cost (fraction of real time)
    /// of the middle tier (3) at a hard working point.
    pub fn pick(cost_mid: f32) -> u8 {
        let mut level = 5u8;
        for l in (1..=5u8).rev() {
            let cost = cost_mid * Self::STEP_COST.powi(3 - l as i32);
            if cost <= Self::AIM_COST {
                level = l;
            } else {
                break;
            }
        }
        level
    }

    /// A different engine: start from the level its benchmark gave.
    pub fn reset(&mut self, t: f32, level: u8) {
        *self = AutoQuality { level: level.clamp(1, 5), changed_at: t, ..AutoQuality::new() };
    }

    /// Feeds the smoothed load at time `t` (seconds); returns the new level when it changes.
    pub fn update(&mut self, load: f32, t: f32) -> Option<u8> {
        if t - self.floor_at > Self::FLOOR_MEMORY {
            self.floor = 1;
        }
        if t - self.changed_at < Self::SETTLE {
            return None;
        }
        if load > Self::OVERLOAD {
            self.calm_since = None;
            let since = *self.over_since.get_or_insert(t);
            if t - since >= Self::OVERLOAD_FOR && self.level < 5 {
                let k = ((load / Self::AIM_DOWN).ln() / Self::STEP_COST.ln()).ceil().max(1.0) as u8;
                self.level = (self.level + k).min(5);
                // the finer tier just left is too heavy for this engine here
                self.floor = self.level;
                self.floor_at = t;
                return Some(self.changed(t));
            }
            return None;
        }
        self.over_since = None;
        if load < Self::CALM && self.level > self.floor && t - self.changed_at > Self::COOLDOWN_UP {
            let since = *self.calm_since.get_or_insert(t);
            if t - since >= Self::CALM_FOR {
                // one tier at a time: a step up is the part of the policy a listener can notice
                self.level = (self.level - 1).max(self.floor).max(1);
                return Some(self.changed(t));
            }
        } else {
            self.calm_since = None;
        }
        None
    }

    fn changed(&mut self, t: f32) -> u8 {
        self.changed_at = t;
        self.over_since = None;
        self.calm_since = None;
        self.level
    }
}

/// The model's cost at a hard working point (0.7 of the redline's revs, nearly wide open, which is where the pipes
/// are busiest) on the middle tier, as a fraction of real time on this thread. A throw-away model renders a third of
/// a second of audio after a short warm-up.
fn benchmark_cost(engine: &str, tuning: &Tuning, banks: &[u8]) -> Option<f32> {
    let cfg = engine_by_id(engine)?;
    let mut m = EngineSound::new(&cfg, tuning, banks, Quality::BALANCED);
    m.preheat(950.0);
    let rpm = (cfg.redline_rpm as f32 * 0.7).clamp(2500.0, 5000.0);
    m.set_input(&SoundInput {
        rpm,
        throttle: 0.9,
        load: 0.95,
        map_pa: 92_000.0,
        atm_pa: 101_325.0,
        air_k: 293.0,
        charge_k: 305.0,
        afr: 12.8,
        egt_k: 950.0,
        running: true,
        lit: true,
        burn: vec![1.0; cfg.cylinder_count],
        master: 1.0,
        serial: 1,
        ..Default::default()
    });
    let mut buf = vec![0.0f32; CHUNK];
    for _ in 0..(SAMPLE_RATE as usize / 8 / CHUNK) {
        m.render(&mut buf);
    }
    // two runs, the better one: the first meets cold caches, a processor that has not yet clocked up and the rest of
    // the program starting at the same moment, and says the machine is slower than it is
    let n = SAMPLE_RATE as usize / 5 / CHUNK;
    let mut best = f32::MAX;
    for _ in 0..2 {
        let t0 = Instant::now();
        for _ in 0..n {
            m.render(&mut buf);
        }
        best = best.min(t0.elapsed().as_secs_f32() / (n * CHUNK) as f32 * SAMPLE_RATE as f32);
    }
    Some(best)
}

/// Short ramps around a rebuild of the model, so the new one never starts (or the old one stops) with a jump.
struct Fade {
    gain: f32,
    /// +1 rising / steady, -1 falling.
    dir: f32,
}

impl Fade {
    const LEN: f32 = 441.0;

    fn apply(&mut self, buf: &mut [f32]) {
        let step = self.dir / Self::LEN;
        for v in buf.iter_mut() {
            self.gain = (self.gain + step).clamp(0.0, 1.0);
            *v *= self.gain;
        }
    }
}

/// A replacement model running beside the current one: it renders in step with it (its output discarded) until its
/// pipes have settled, then is cross-faded in.
struct Incoming {
    m: EngineSound,
    key: BuildKey,
    primed: usize,
    mixed: usize,
}

/// Samples the replacement runs before it is heard (0.25 s: its pipes need a few cycles to establish the pulses),
/// and the length of the cross-fade (0.2 s: the tiers differ in how much body they carry, and a step of a few dB is
/// a morph over a fifth of a second and a click over 40 ms).
const PRIME: usize = 11_025;
const XFADE: usize = 8_820;

impl Incoming {
    /// Renders the replacement's next `chunk.len()` samples and mixes them into `chunk` (the current model's) once it
    /// is primed; true when the hand-over is complete. The two are correlated (same pulses at the same crank angle),
    /// so the fade is linear.
    fn mix(&mut self, chunk: &mut [f32], scratch: &mut [f32]) -> bool {
        let n = chunk.len();
        self.m.render(&mut scratch[..n]);
        self.primed += n;
        if self.primed > PRIME {
            for (i, (c, s)) in chunk.iter_mut().zip(scratch.iter()).enumerate() {
                let g = ((self.mixed + i) as f32 / XFADE as f32).min(1.0);
                *c += (*s - *c) * g;
            }
            self.mixed += n;
        }
        self.mixed >= XFADE
    }
}

/// Everything a built model depends on; a change rebuilds it.
#[derive(Clone, PartialEq)]
struct BuildKey {
    engine: String,
    tuning: Tuning,
    banks: Vec<u8>,
    quality: u8,
}

fn spawn_worker(state: std::sync::Weak<Mutex<AudioState>>, ring: Arc<Ring>) {
    std::thread::Builder::new()
        .name("engine-sound".into())
        .spawn(move || {
            let mut model: Option<(EngineSound, BuildKey)> = None;
            let mut auto = AutoQuality::new();
            let mut fade = Fade { gain: 0.0, dir: 1.0 };
            let mut last_engine = String::new();
            let clock = Instant::now();
            let mut last_build = Instant::now() - Duration::from_secs(10);
            let mut last_serial = u64::MAX;
            let mut load_avg = 0.0f32;
            let mut chunk = vec![0.0f32; CHUNK];
            let mut scratch = vec![0.0f32; CHUNK];
            let mut incoming: Option<Incoming> = None;
            let mut diag_tick = Instant::now();
            let mut log_tick = Instant::now();
            let log = std::env::var_os("SOUND_LOG").is_some();
            loop {
                let Some(shared) = state.upgrade() else { return };
                // the sim's snapshot, taken without holding the lock while rendering
                let snap = match shared.try_lock() {
                    Ok(g) => Some(g.clone()),
                    Err(_) => None,
                };
                let Some(snap) = snap else {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                };
                if snap.engine_id.is_empty() {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                if snap.engine_id != last_engine {
                    last_engine = snap.engine_id.clone();
                    // the first tier is chosen from a benchmark of this engine, not guessed and climbed to
                    let level = benchmark_cost(&snap.engine_id, &snap.tuning, &snap.banks).map_or(AutoQuality::START_LEVEL, |c| {
                        if log {
                            eprintln!("SOUND benchmark {}: {:.0}% of a core on the middle tier", snap.engine_id, c * 100.0);
                        }
                        AutoQuality::pick(c)
                    });
                    auto.reset(clock.elapsed().as_secs_f32(), level);
                    load_avg = 0.0;
                }
                let pref = if snap.quality_pref == 0 { auto.level() } else { snap.quality_pref.clamp(1, 5) };
                let want = BuildKey { engine: snap.engine_id.clone(), tuning: snap.tuning.clone(), banks: snap.banks.clone(), quality: pref };
                let stale = model.as_ref().map_or(true, |(_, k)| *k != want);
                let handing_over = incoming.as_ref().map_or(false, |i| i.key == want);
                let same_engine = model.as_ref().map_or(false, |(_, k)| k.engine == want.engine);
                if stale && !handing_over && model.is_some() && same_engine {
                    // A finer solver tier or a changed part: the replacement is built beside the current model,
                    // started from its crank angle and compressor state, runs in step with it until its pipes have
                    // settled and is then cross-faded in. (A cold model dropped in place was a 40 ms dropout and a
                    // +4 dB surge.) A slider drag changes the parts every frame: at most one replacement per 0.4 s.
                    let due = last_build.elapsed() > if incoming.is_some() { Duration::from_millis(400) } else { Duration::from_millis(120) };
                    if due {
                        if let (Some(cfg), Some((cur, _))) = (engine_by_id(&want.engine), model.as_ref()) {
                            let mut m = EngineSound::new(&cfg, &want.tuning, &want.banks, quality_of(pref));
                            if cur.input.running || snap.running {
                                m.preheat((snap.egt_k * 0.85).max(500.0));
                            }
                            m.align_with(cur);
                            m.set_input(&sound_input(&snap));
                            incoming = Some(Incoming { m, key: want, primed: 0, mixed: 0 });
                            last_build = Instant::now();
                        }
                    }
                } else if stale && !handing_over && (model.is_none() || last_build.elapsed() > Duration::from_millis(120)) {
                    // A different engine (or the very first model): ramp the old one down and the new one up.
                    incoming = None;
                    if model.is_some() && fade.dir > 0.0 {
                        fade.dir = -1.0;
                    }
                    if model.is_none() || fade.gain <= 0.0 {
                        if let Some(cfg) = engine_by_id(&want.engine) {
                            let warm = model.as_ref().map_or(false, |(m, _)| m.input.running);
                            let mut m = EngineSound::new(&cfg, &want.tuning, &want.banks, quality_of(pref));
                            if warm || snap.running {
                                m.preheat((snap.egt_k * 0.85).max(500.0));
                            }
                            model = Some((m, want));
                            last_build = Instant::now();
                            last_serial = u64::MAX;
                            fade.dir = 1.0;
                        }
                    }
                }
                let Some((m, _)) = model.as_mut() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                if snap.serial != last_serial {
                    last_serial = snap.serial;
                    let inp = sound_input(&snap);
                    m.set_input(&inp);
                    if let Some(inc) = incoming.as_mut() {
                        inc.m.set_input(&inp);
                    }
                }
                let mut rendered = 0usize;
                let t0 = Instant::now();
                let mut handed_over = false;
                while ring.len() < TARGET_FILL {
                    m.render(&mut chunk);
                    fade.apply(&mut chunk);
                    if let Some(inc) = incoming.as_mut() {
                        handed_over = inc.mix(&mut chunk, &mut scratch);
                    }
                    ring.push(&chunk);
                    rendered += CHUNK;
                    // the sim may have published a newer frame while we were rendering
                    if handed_over || rendered >= 4 * CHUNK {
                        break;
                    }
                }
                if rendered > 0 {
                    let spent = t0.elapsed().as_secs_f32();
                    let audio = rendered as f32 / SAMPLE_RATE as f32;
                    // while a replacement renders beside the model the work is doubled for a moment: that is no
                    // measure of the tier
                    if incoming.is_none() {
                        load_avg += (spent / audio - load_avg) * 0.05;
                    }
                    // the finest solver the machine keeps up with
                    if snap.quality_pref == 0 {
                        if let Some(new) = auto.update(load_avg, clock.elapsed().as_secs_f32()) {
                            // the cost per second of audio changes by about the tier ratio
                            let ratio = AutoQuality::STEP_COST.powi(pref as i32 - new as i32);
                            load_avg = (load_avg * ratio).clamp(0.05, 1.5);
                        }
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(2));
                }
                if diag_tick.elapsed() > Duration::from_millis(200) {
                    diag_tick = Instant::now();
                    if let Ok(mut g) = shared.try_lock() {
                        g.pops_fired = m.pops_fired;
                        g.pop_gain = m.pop_gain;
                        g.diag = diagnostics(m, load_avg, pref, ring.underruns.load(Ordering::Relaxed));
                        if log && log_tick.elapsed() > Duration::from_secs(2) {
                            log_tick = Instant::now();
                            let d = &g.diag;
                            eprintln!("SOUND rpm {:.0} thr {:.2} | {} cells {} nodes | load {:.0}% quality {} underruns {} | drone {:.0} Hz, tail gas {:.0} C, pops {}", snap.rpm, snap.throttle, d.cells, d.nodes, d.load * 100.0, d.quality, d.underruns, d.drone_hz, d.tail_gas_k - 273.15, g.pops_fired);
                        }
                    }
                }
                if handed_over {
                    if let Some(inc) = incoming.take() {
                        model = Some((inc.m, inc.key));
                    }
                }
            }
        })
        .expect("spawn the engine sound thread");
}

fn diagnostics(m: &EngineSound, load: f32, quality: u8, underruns: u64) -> AudioDiag {
    let r = &m.gas.exhaust.report;
    let grid = &m.gas.exhaust.net.grid;
    // gas temperature at the first and last duct of the longest route
    let header_k = if grid.ducts.is_empty() { 0.0 } else { grid.mean_temperature(0) };
    let tail_k = m.gas.exhaust.tails.first().map_or(0.0, |t| grid.mean_temperature(t.duct));
    let t_mean = 0.5 * (header_k + tail_k);
    let c = engine_core::acoustics::Gas::EXHAUST.sound_speed(t_mean.max(280.0));
    AudioDiag {
        cells: r.cells + m.gas.intake.report.cells,
        nodes: r.nodes + m.gas.intake.report.nodes,
        load,
        quality,
        exhaust_path_m: r.path_length,
        primary_m: r.primary_length,
        tail_gas_k: tail_k,
        header_gas_k: header_k,
        resonator_hz: r.resonator_hz.unwrap_or(0.0),
        drone_hz: c / (4.0 * (r.path_length + 0.02)),
        underruns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use engine_core::acoustics::analysis::{peaks, rms, spectrum};

    const FRAME: f32 = 1.0 / 60.0;

    /// Drives the real sim frame by frame and renders the real sound model from the snapshots, exactly as the
    /// worker does.
    fn run(engine: &str, secs: f32, quality: Quality, script: impl Fn(f32, &mut Sim)) -> (Vec<f32>, Vec<(f32, f32)>, u64) {
        let shared = Arc::new(Mutex::new(AudioState::default()));
        let mut sim = Sim::new(shared.clone(), engine);
        let cfg = engine_by_id(engine).unwrap();
        let mut snd = EngineSound::new(&cfg, &Tuning::default(), &engine_core::sound::default_banks(&cfg), quality);
        let per_frame = (SAMPLE_RATE as f32 * FRAME) as usize;
        let (mut out, mut log) = (Vec::new(), Vec::new());
        let mut buf = vec![0.0f32; per_frame];
        for f in 0..(secs / FRAME) as usize {
            let t = f as f32 * FRAME;
            script(t, &mut sim);
            sim.step(FRAME);
            let a = shared.lock().unwrap().clone();
            snd.set_input(&sound_input(&a));
            snd.render(&mut buf);
            out.extend_from_slice(&buf);
            log.push((t, sim.tel.rpm as f32));
        }
        (out, log, snd.pops_fired)
    }

    fn start_script(t: f32, sim: &mut Sim) {
        sim.starter_held = t < 2.0 || (sim.tel.rpm < 500.0 && t < 4.0);
    }

    fn slice(x: &[f32], from: f32, to: f32) -> &[f32] {
        let end = ((to * SAMPLE_RATE as f32) as usize).min(x.len());
        &x[(from * SAMPLE_RATE as f32) as usize..end]
    }

    #[test]
    fn ring_buffer_round_trips_and_underruns_to_silence() {
        let r = Ring::new();
        assert_eq!(r.pop(), 0.0);
        let data: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        assert_eq!(r.push(&data), 1000);
        for i in 0..1000 {
            assert_eq!(r.pop(), i as f32);
        }
        assert_eq!(r.len(), 0);
        // wrap-around
        for round in 0..50 {
            r.push(&vec![round as f32; 3000]);
            for _ in 0..3000 {
                assert_eq!(r.pop(), round as f32);
            }
        }
    }

    #[test]
    fn every_engine_starts_and_makes_finite_sound() {
        for e in engine_core::engines() {
            let (x, log, _) = run(&e.id, 5.0, Quality::FAST, start_script);
            assert!(x.iter().all(|v| v.is_finite()), "{}: non-finite audio", e.id);
            let crank = rms(slice(&x, 0.4, 1.0));
            let idle = rms(slice(&x, 4.0, 5.0));
            println!("{:>9}: crank rms {crank:.4}, idle rms {idle:.4}, rpm {:.0}", e.id, log.last().unwrap().1);
            assert!(crank > 1e-4, "{}: the starter is inaudible", e.id);
            assert!(idle > crank * 0.5, "{}: idle is far weaker than cranking ({idle} vs {crank})", e.id);
            assert!(x.iter().all(|v| v.abs() <= 1.0));
        }
    }

    #[test]
    fn idle_note_sits_on_the_firing_frequency() {
        for id in ["v8", "i4", "v12", "i6"] {
            let (x, log, _) = run(id, 7.0, Quality::BALANCED, start_script);
            let rpm = log.last().unwrap().1;
            let cfg = engine_by_id(id).unwrap();
            let fire = rpm / 60.0 * cfg.cylinder_count as f32 / 2.0;
            let psd = spectrum(slice(&x, 4.5, 7.0), 16384);
            let pk = peaks(&psd, SAMPLE_RATE as f64, 20.0, 400.0, 6.0);
            println!("{id}: idle rpm {rpm:.0}, firing {fire:.1} Hz, strongest lines {:?}", pk.iter().take(4).map(|p| p.0.round()).collect::<Vec<_>>());
            // the firing frequency, or a sub-harmonic of it (V8 half orders), must be among the strongest lines
            let near = |f: f64| pk.iter().take(5).any(|p| (p.0 / f - 1.0).abs() < 0.04);
            assert!(near(fire as f64) || near(fire as f64 / 2.0) || near(fire as f64 * 2.0), "{id}: no line near {fire} Hz in {:?}", pk.iter().take(5).collect::<Vec<_>>());
        }
    }

    #[test]
    fn revving_raises_the_pitch_and_the_level() {
        let script = |t: f32, sim: &mut Sim| {
            start_script(t, sim);
            if sim.gear_idx == 1 && t > 3.5 {
                sim.set_gear_index(2);
            }
            sim.auto_shift = false;
            sim.throttle_input = if t > 4.5 { 1.0 } else { 0.0 };
        };
        let (x, log, _) = run("v8", 9.0, Quality::FAST, script);
        // windows where the engine is passing through 2500 and 5000 rpm, clear of idle and of the rev limiter
        let when = |rpm: f32| log.iter().find(|l| l.0 > 4.6 && l.1 > rpm).map(|l| l.0).unwrap();
        let (t_lo, t_hi) = (when(2500.0), when(5000.0));
        let (l1, l2) = (rms(slice(&x, 3.9, 4.4)), rms(slice(&x, t_hi, t_hi + 0.3)));
        println!("rev: idle rms {l1:.4} -> {l2:.4} at 5000 rpm (t = {t_hi:.2} s)");
        assert!(l2 > l1, "full throttle at high rpm must be louder than idle");
        // the spectral centroid below 3 kHz climbs with the revs
        let centroid = |t0: f32| {
            let psd = spectrum(slice(&x, t0, t0 + 0.3), 8192);
            engine_core::acoustics::analysis::centroid(&psd, SAMPLE_RATE as f64, 30.0, 3000.0)
        };
        let (c1, c2) = (centroid(t_lo), centroid(t_hi));
        println!("spectral centroid {c1:.0} Hz at 2500 rpm -> {c2:.0} Hz at 5000 rpm");
        assert!(c2 > c1 * 1.25, "pitch must rise with rpm: {c1} -> {c2}");
    }

    #[test]
    fn lift_off_burbles_where_it_suits_the_engine() {
        let script = |t: f32, sim: &mut Sim| {
            start_script(t, sim);
            if sim.gear_idx == 1 && t > 3.5 {
                sim.set_gear_index(2);
            }
            sim.throttle_input = if (4.5..9.0).contains(&t) { 1.0 } else { 0.0 };
        };
        let mut pops = Vec::new();
        for id in ["v12", "v8", "dieseli6"] {
            let (x, _, p) = run(id, 14.0, Quality::FAST, script);
            assert!(x.iter().all(|v| v.is_finite()));
            println!("BURBLE {id}: {p} pops");
            pops.push(p);
        }
        assert!(pops[0] >= 3, "a V12 should burble on lift-off: {}", pops[0]);
        assert_eq!(pops[2], 0, "a diesel never pops");
    }

    /// Feeds `load` to the policy every 0.1 s from `t0` for `secs`; returns the time and level of the first change.
    fn feed(a: &mut AutoQuality, load: f32, t0: f32, secs: f32) -> Option<(f32, u8)> {
        let mut t = t0;
        while t < t0 + secs {
            t += 0.1;
            if let Some(l) = a.update(load, t) {
                return Some((t, l));
            }
        }
        None
    }

    #[test]
    fn the_benchmark_picks_the_finest_tier_that_leaves_room() {
        assert_eq!(AutoQuality::pick(0.05), 1, "a light engine gets the finest tier");
        assert_eq!(AutoQuality::pick(0.20), 1);
        assert_eq!(AutoQuality::pick(0.30), 2);
        assert_eq!(AutoQuality::pick(0.45), 3);
        assert_eq!(AutoQuality::pick(0.60), 4);
        assert_eq!(AutoQuality::pick(2.00), 5, "an engine too heavy for any tier still gets the coarsest");
        // a finer tier costs more, so the pick never gets finer as the engine gets heavier
        let mut last = 1;
        for c in (1..400).map(|i| i as f32 * 0.01) {
            let l = AutoQuality::pick(c);
            assert!(l >= last);
            last = l;
        }
    }

    #[test]
    fn auto_quality_steps_down_at_once_when_overloaded_and_up_only_after_a_long_calm() {
        // overloaded at level 3: it drops, several tiers at once when the overload is large
        let mut b = AutoQuality::new();
        let (t, l) = feed(&mut b, 1.2, 0.0, 30.0).expect("an overloaded solver must coarsen");
        assert_eq!(l, 5);
        assert!(t < 6.0, "overload took {t} s to answer");
        let mut c = AutoQuality::new();
        let (_, l) = feed(&mut c, 0.85, 0.0, 30.0).expect("a slightly overloaded solver must coarsen");
        assert_eq!(l, 5, "0.85 of a core needs two tiers to get inside half a core");

        // a calm machine steps up one tier, and only after a long quiet time: a change is a change in the sound
        let mut a = AutoQuality::new();
        let (t, l) = feed(&mut a, 0.10, 0.0, 300.0).expect("a calm machine must refine, eventually");
        assert_eq!(l, 2, "one tier at a time");
        assert!(t > 40.0, "refined at {t} s");

        // between the thresholds nothing ever changes
        let mut d = AutoQuality::new();
        assert!(feed(&mut d, 0.5, 0.0, 600.0).is_none());
    }

    #[test]
    fn auto_quality_does_not_flap_and_forgets_after_a_while() {
        let mut a = AutoQuality::new();
        let (t, l) = feed(&mut a, 1.0, 0.0, 30.0).unwrap();
        assert!(l >= 4);
        // the machine now looks calm, but the finer tier just proved too heavy: it stays put for minutes
        assert!(feed(&mut a, 0.10, t, 250.0).is_none(), "refined into the tier that had just overloaded");
        // much later it may try again
        let again = feed(&mut a, 0.10, t + 250.0, 300.0);
        assert!(again.is_some(), "it never tries a finer tier again");
        // a different engine starts over from the level its benchmark gave
        a.reset(500.0, 2);
        assert_eq!(a.level(), 2);
    }

    /// Renders scripted drives through the live path (sim -> `sound_input` -> model) and writes them as WAVs for
    /// the spectral tools: `cargo test --release -p viewer render_drive_scenarios -- --ignored --nocapture`.
    /// `DRIVE_ENGINES=v8,i4` picks the engines, `DRIVE_OUT` the folder, `DRIVE_QUALITY` the ladder level.
    #[test]
    #[ignore]
    fn render_drive_scenarios() {
        let out_dir = std::env::var("DRIVE_OUT").unwrap_or_else(|_| "../../target/soundlab".into());
        std::fs::create_dir_all(&out_dir).unwrap();
        let level: u8 = std::env::var("DRIVE_QUALITY").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
        let ids = std::env::var("DRIVE_ENGINES").unwrap_or_else(|_| "v8,i4,v12,i6".into());
        for id in ids.split(',') {
            let script = |t: f32, sim: &mut Sim| {
                start_script(t, sim);
                sim.auto_shift = true;
                if sim.gear_idx == 1 && t > 5.0 && t < 5.1 {
                    sim.set_gear_index(2);
                }
                sim.throttle_input = match t {
                    t if t < 5.0 => 0.0,
                    t if t < 8.0 => 0.45,
                    t if t < 14.0 => 1.0,
                    t if t < 18.0 => 0.0,
                    t if t < 20.0 => 0.35,
                    _ => 0.0,
                };
                sim.brake_input = if (20.0..23.0).contains(&t) { 0.6 } else { 0.0 };
            };
            let (x, log, pops) = run(id, 26.0, quality_of(level), script);
            let base = format!("{out_dir}/drive_{id}");
            engine_core::acoustics::analysis::write_wav(&format!("{base}.wav"), &x, SAMPLE_RATE).unwrap();
            let csv: String = log.iter().step_by(6).map(|(t, r)| format!("{t:.2},{r:.0}\n")).collect();
            std::fs::write(format!("{base}_rpm.csv"), csv).unwrap();
            println!("DRIVE {id}: {} s written to {base}.wav, {pops} pops, peak {:.3}", x.len() as f32 / SAMPLE_RATE as f32, x.iter().fold(0.0f32, |a, v| a.max(v.abs())));
        }
    }

    /// Named scenarios for the problems that only show up in the live path: `highgear` (wide open in the top gear,
    /// the revs climbing to the limiter), `shutoff` (revved in neutral, then the key goes off), `coast` (in gear at
    /// speed, then the key goes off and the car rolls on). `DRIVE_SCENARIOS`, `DRIVE_ENGINES`, `DRIVE_OUT`, `DRIVE_QUALITY`.
    #[test]
    #[ignore]
    fn render_problem_scenarios() {
        let out_dir = std::env::var("DRIVE_OUT").unwrap_or_else(|_| "../../target/soundlab".into());
        std::fs::create_dir_all(&out_dir).unwrap();
        let level: u8 = std::env::var("DRIVE_QUALITY").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
        let ids = std::env::var("DRIVE_ENGINES").unwrap_or_else(|_| "v8,i4,v6".into());
        let scen = std::env::var("DRIVE_SCENARIOS").unwrap_or_else(|_| "highgear,shutoff,coast".into());
        for id in ids.split(',') {
            for sc in scen.split(',') {
                let script = |t: f32, sim: &mut Sim| {
                    start_script(t, sim);
                    match sc {
                        "highgear" => {
                            sim.auto_shift = true;
                            if sim.gear_idx == 1 && t > 4.0 && t < 4.1 {
                                sim.set_gear_index(2);
                            }
                            sim.throttle_input = if t < 4.5 { 0.0 } else { 1.0 };
                        }
                        "shutoff" => {
                            sim.auto_shift = false;
                            sim.throttle_input = if (5.0..9.0).contains(&t) { 0.7 } else { 0.0 };
                            if t > 9.0 && sim.tel.is_running {
                                sim.engine.shut_off();
                            }
                        }
                        _ => {
                            sim.auto_shift = true;
                            if sim.gear_idx == 1 && t > 4.0 && t < 4.1 {
                                sim.set_gear_index(2);
                            }
                            sim.throttle_input = if (4.5..16.0).contains(&t) { 0.6 } else { 0.0 };
                            if t > 16.0 && sim.tel.is_running {
                                sim.engine.shut_off();
                            }
                        }
                    }
                };
                let secs = if sc == "highgear" { 45.0 } else { 30.0 };
                let (x, log, _) = run(id, secs, quality_of(level), script);
                let base = format!("{out_dir}/{sc}_{id}");
                engine_core::acoustics::analysis::write_wav(&format!("{base}.wav"), &x, SAMPLE_RATE).unwrap();
                let csv: String = log.iter().step_by(6).map(|(t, r)| format!("{t:.2},{r:.0}\n")).collect();
                std::fs::write(format!("{base}_rpm.csv"), csv).unwrap();
                println!("SCENARIO {sc} {id}: {} s written to {base}.wav", x.len() as f32 / SAMPLE_RATE as f32);
            }
        }
    }

    /// Runs the sim and the live model, replacing the model by a finer-tier one halfway the way the worker does
    /// (`Incoming`: built beside the old one, aligned to it, primed, cross-faded). Returns the audio and the time the
    /// hand-over was started.
    fn run_with_handover(id: &str, secs: f32, at: f32, to: Quality) -> (Vec<f32>, f32) {
        let shared = Arc::new(Mutex::new(AudioState::default()));
        let mut sim = Sim::new(shared.clone(), id);
        let cfg = engine_by_id(id).unwrap();
        let banks = engine_core::sound::default_banks(&cfg);
        sim.pre_roll(2.0);
        let mut snd = EngineSound::new(&cfg, &Tuning::default(), &banks, Quality::BALANCED);
        let per_frame = (SAMPLE_RATE as f32 * FRAME) as usize;
        let (mut out, mut buf, mut scratch) = (Vec::new(), vec![0.0f32; per_frame], vec![0.0f32; per_frame]);
        let mut incoming: Option<Incoming> = None;
        let mut started = false;
        for f in 0..(secs / FRAME) as usize {
            let t = f as f32 * FRAME;
            sim.throttle_input = if t > 1.0 { 0.3 } else { 0.0 };
            sim.step(FRAME);
            let a = shared.lock().unwrap().clone();
            let inp = sound_input(&a);
            if !started && t >= at {
                started = true;
                let mut m = EngineSound::new(&cfg, &Tuning::default(), &banks, to);
                m.preheat((a.egt_k * 0.85).max(500.0));
                m.align_with(&snd);
                m.set_input(&inp);
                incoming = Some(Incoming { m, key: BuildKey { engine: id.into(), tuning: Tuning::default(), banks: banks.clone(), quality: 2 }, primed: 0, mixed: 0 });
            }
            snd.set_input(&inp);
            if let Some(inc) = incoming.as_mut() {
                inc.m.set_input(&inp);
            }
            snd.render(&mut buf);
            let done = incoming.as_mut().map_or(false, |inc| inc.mix(&mut buf, &mut scratch));
            if done {
                snd = incoming.take().unwrap().m;
            }
            out.extend_from_slice(&buf);
        }
        (out, at)
    }

    #[test]
    fn a_model_handover_is_not_a_jump_in_level_or_waveform() {
        let (x, at) = run_with_handover("i4", 5.0, 1.5, Quality::HIGH);
        let win = (0.02 * SAMPLE_RATE as f32) as usize;
        let db = |a: usize| 20.0 * rms(&x[a..a + win]).max(1e-6).log10();
        let steady: Vec<f32> = (0..20).map(|k| db(((at - 0.6) * SAMPLE_RATE as f32) as usize + k * win)).collect();
        let mean = steady.iter().sum::<f32>() / steady.len() as f32;
        let (lo, hi) = (steady.iter().cloned().fold(f32::MAX, f32::min), steady.iter().cloned().fold(f32::MIN, f32::max));
        // the 20 ms windows from the start of the hand-over to well after it
        let worst = (0..40).map(|k| db(((at + 0.1) * SAMPLE_RATE as f32) as usize + k * win)).map(|d| (d - mean).abs()).fold(0.0f32, f32::max);
        println!("hand-over: steady {mean:.1} dB (spread {lo:.1}..{hi:.1}), worst window after {worst:.1} dB from it");
        assert!(worst < (hi - lo).max(1.0) + 2.0, "level jumped by {worst} dB across the hand-over (steady spread {})", hi - lo);
        let step = |r: std::ops::Range<usize>| x[r].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        let (a0, a1) = (((at - 0.6) * SAMPLE_RATE as f32) as usize, (at * SAMPLE_RATE as f32) as usize);
        let (b0, b1) = ((at * SAMPLE_RATE as f32) as usize, ((at + 0.6) * SAMPLE_RATE as f32) as usize);
        assert!(step(b0..b1) < 2.5 * step(a0..a1), "waveform step {} vs {}", step(b0..b1), step(a0..a1));
    }

    #[test]
    fn a_starved_ring_is_a_dip_not_a_click() {
        // a 200 Hz sine at a few percent of full scale, 30 samples of it missing, then back
        let mut d = Declick::new();
        let s = |i: usize| 0.3 * (std::f32::consts::TAU * 200.0 * i as f32 / 44_100.0).sin();
        let mut out = Vec::new();
        for i in 0..2000 {
            let starved = (1000..1030).contains(&i);
            out.push(d.next(if starved { None } else { Some(s(i)) }));
        }
        let natural = out[..900].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        let worst = out[900..1300].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        println!("dropout: natural step {natural:.4}, worst step around the dropout {worst:.4}");
        assert!(worst <= 1.6 * natural, "dropout makes a step of {worst} against the signal's own {natural}");
        // and after the recovery it is the signal again
        assert!((out[1500] - s(1500)).abs() < 1e-6);
    }

    /// Renders a swap through the live hand-over for the scanner: `DRIVE_OUT` is the folder.
    #[test]
    #[ignore]
    fn render_model_swap() {
        let out_dir = std::env::var("DRIVE_OUT").unwrap_or_else(|_| "../../target/soundlab".into());
        std::fs::create_dir_all(&out_dir).unwrap();
        for id in std::env::var("DRIVE_ENGINES").unwrap_or_else(|_| "v8".into()).split(',') {
            let (x, _) = run_with_handover(id, 8.0, 3.0, Quality::ULTRA);
            let base = format!("{out_dir}/swap_{id}");
            engine_core::acoustics::analysis::write_wav(&format!("{base}.wav"), &x, SAMPLE_RATE).unwrap();
            println!("SWAP {id}: written to {base}.wav");
        }
    }

    /// Which tier the benchmark picks for every engine on this machine: `cargo test --release -p viewer print_auto_tiers -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_auto_tiers() {
        for e in engine_core::engines() {
            let cfg = engine_by_id(&e.id).unwrap();
            let banks = engine_core::sound::default_banks(&cfg);
            let t = Tuning::default();
            let c = benchmark_cost(&e.id, &t, &banks).unwrap();
            println!("TIER {:>9}: benchmark {:.0}% of a core on the middle tier -> level {}", e.id, c * 100.0, AutoQuality::pick(c));
        }
    }

    #[test]
    fn the_fade_never_jumps() {
        let mut f = Fade { gain: 1.0, dir: -1.0 };
        let mut buf = vec![1.0f32; 1000];
        f.apply(&mut buf);
        let worst = buf.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(worst < 0.005, "fade step {worst}");
        assert_eq!(f.gain, 0.0);
        f.dir = 1.0;
        let mut buf = vec![1.0f32; 1000];
        f.apply(&mut buf);
        assert!(buf[0] < 0.01 && (buf[999] - 1.0).abs() < 1e-6);
    }
}
