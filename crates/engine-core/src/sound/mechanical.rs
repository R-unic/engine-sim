//! The sounds that are not gas dynamics: starter motor, valve train, combustion knock transmitted through
//! the block, detonation ping, turbo compressor whistle and blow-off, gear whine.

use super::flow_noise::Shaper;
use super::starter::{Starter, StarterCtx};

/// Pressure (rms, Pa, at the listener) of the broadband engine noise at 1000 rpm and full load: calibrated against
/// recordings of idling and revving cars (`README`, Audio).
const ENGINE_NOISE_PA: f32 = 0.024;

#[derive(Clone, Debug)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Noise(seed | 1)
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Slowly wandering unit-variance noise (one-pole low-passed white noise, rescaled): the random drift of a tone's
/// frequency or the flutter of its level.
#[derive(Clone, Copy, Debug)]
pub struct Wander {
    y: f32,
    k: f32,
    g: f32,
}

impl Wander {
    pub fn new(hz: f32, fs: f32) -> Wander {
        let k = 1.0 - (-std::f32::consts::TAU * hz / fs).exp();
        // white noise in [-1, 1] has variance 1/3; a one-pole low-pass keeps k / (2 - k) of it
        Wander { y: 0.0, k, g: (3.0 * (2.0 - k) / k).sqrt() }
    }
    #[inline]
    pub fn step(&mut self, n: f32) -> f32 {
        self.y += (n - self.y) * self.k;
        self.y * self.g
    }
}

/// A two-pole resonator excited by impulses: one structural mode of the block or head.
#[derive(Clone, Copy, Debug, Default)]
pub struct Res {
    y1: f32,
    y2: f32,
    a1: f32,
    a2: f32,
}

impl Res {
    pub fn new(f: f32, zeta: f32, fs: f32) -> Res {
        let w = std::f32::consts::TAU * f / fs;
        let r = (-zeta * w).exp();
        Res { y1: 0.0, y2: 0.0, a1: 2.0 * r * w.cos(), a2: -r * r }
    }
    #[inline]
    pub fn step(&mut self, x: f32) -> f32 {
        let y = x + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Everything the mechanical layer needs to know each sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct MechCtx {
    pub rpm: f32,
    pub throttle: f32,
    pub load: f32,
    pub spool: f32,
    pub boost: f32,
    /// The starter is driving the engine: the pinion is in and the motor is turning.
    pub starter_on: bool,
    /// Instantaneous crank speed while cranking (with the surge and sag of the compression strokes), rpm.
    pub crank_rpm: f32,
    /// 0..1: how hard the starter works against a compression stroke right now.
    pub starter_strain: f32,
    pub combusting: bool,
    pub knock: f32,
    pub gear_whine: f32,
    pub vehicle_speed: f32,
    /// Combustion impulses this sample (sum over cylinders, 0..), valve events this sample (count).
    pub combustion: f32,
    pub valve_events: f32,
    pub diesel: bool,
    pub lift_off_pop: f32,
}

#[derive(Clone, Debug)]
pub struct Mechanical {
    fs: f32,
    rng: Noise,
    block: [Res; 3],
    head: Res,
    ping: Res,
    starter: Starter,
    /// The combustion pressure rise is not a spike: it reaches the block as a pulse about a third of a
    /// millisecond long, which is what keeps the rattle a thud rather than a click.
    comb_lp: f32,
    valve_lp: f32,
    // turbo
    spool_s: f32,
    turbo_phase: f32,
    turbo_phase2: f32,
    turbo_lp: f32,
    bov_env: f32,
    bov_lp: f32,
    bov_armed: bool,
    boost_hold: f32,
    thr_slow: f32,
    throttle: f32,
    gear_phase: f32,
    gear_shaft: f32,
    knock_env: f32,
    tick_env: f32,
    tick_gain: f32,
    /// Broadband engine noise (combustion, bearings, gears, drive, injectors): stationary, as recordings of real engines show.
    engine_noise: Shaper,
    // the tonal parts are not perfectly steady: drift and flutter of the whistle and the gear mesh
    whistle_fm: Wander,
    whistle_am: Wander,
    gear_fm: Wander,
    gear_am: Wander,
    rng2: Noise,
}

impl Mechanical {
    pub fn new(fs: f32, displacement_l: f32) -> Self {
        Mechanical {
            fs,
            rng: Noise::new(0x51ED_270B),
            block: [Res::new(640.0, 0.03, fs), Res::new(1350.0, 0.04, fs), Res::new(2600.0, 0.05, fs)],
            head: Res::new(2400.0, 0.06, fs),
            ping: Res::new(5800.0, 0.02, fs),
            starter: Starter::new(fs, displacement_l),
            comb_lp: 0.0,
            valve_lp: 0.0,
            spool_s: 0.0,
            turbo_phase: 0.0,
            turbo_phase2: 0.0,
            turbo_lp: 0.0,
            bov_env: 0.0,
            bov_lp: 0.0,
            bov_armed: true,
            boost_hold: 0.0,
            thr_slow: 0.0,
            throttle: 0.0,
            gear_phase: 0.0,
            gear_shaft: 0.0,
            knock_env: 0.0,
            tick_env: 0.0,
            tick_gain: 1.0,
            engine_noise: {
                let mut s = Shaper::new(0x2F6B_91A7, fs, 450.0, 1_800.0, 6_500.0);
                s.normalise();
                s
            },
            whistle_fm: Wander::new(5.0, fs),
            whistle_am: Wander::new(14.0, fs),
            gear_fm: Wander::new(3.0, fs),
            gear_am: Wander::new(9.0, fs),
            rng2: Noise::new(0x7A31_C5E3),
        }
    }

    /// One sample of mechanical sound, in pascals at the listener.
    pub fn step(&mut self, c: &MechCtx) -> f32 {
        let dt = 1.0 / self.fs;
        let tau = std::f32::consts::TAU;
        let n = self.rng.next();

        // ---- combustion pressure rise rattles the block (diesel clatter is the same, much sharper)
        let strength = c.combustion * if c.diesel { 7.0 } else { 1.0 };
        let imp = strength * (0.6 + 0.8 * c.load.clamp(0.0, 1.2));
        // the pulse is smoothed (a diesel's much less: its pressure rise is nearly instant)
        let k_comb = if c.diesel { 0.5 } else { 1.0 - (-tau * 700.0 * dt).exp() };
        self.comb_lp += (imp - self.comb_lp) * k_comb;
        let mut s = 0.0;
        let k = [1.0, 0.7, 0.45];
        for (i, r) in self.block.iter_mut().enumerate() {
            s += r.step(self.comb_lp * k[i]) * (1.0 - 0.15 * i as f32);
        }
        // a petrol engine's combustion thud stands out at idle, where nothing masks it, and is lost in the
        // exhaust and the wind of a hard drive: it is a little quieter at low revs
        let revs = 0.55 + 0.45 * (c.rpm / 3000.0).clamp(0.0, 1.0);
        let mut out = s * if c.diesel { 0.020 } else { 0.030 * revs };

        // ---- valve train: a soft tick per valve event through the head's resonance, every one a little
        // different (lash, the lifter, which valve), and rolled off - the ear picks out sparse sharp ticks
        // at idle, where they are not masked by anything
        if c.valve_events > 0.0 {
            self.tick_gain = 0.45 + 0.55 * (0.5 + 0.5 * n);
        }
        self.tick_env = self.tick_env.max(c.valve_events.min(1.0) * self.tick_gain * (0.2 + 0.8 * (c.rpm / 6000.0).min(1.0)));
        self.tick_env *= (-dt / 0.0008).exp();
        let tick = self.head.step(n * self.tick_env * 0.05);
        self.valve_lp += (tick - self.valve_lp) * (1.0 - (-tau * 2600.0 * dt).exp());
        out += self.valve_lp * 0.03;

        // ---- engine noise: combustion, bearings, gears, chain and belts, injectors and the rest add up to a broadband
        // noise (recordings: stationary, envelope rms/mean ~0.55, with only a few percent of modulation at the firing
        // rate) whose level climbs with engine speed (about rpm^1.1 in amplitude) and a little with load
        let speed = (c.rpm / 1000.0).max(0.0);
        let load = if c.combusting { 0.7 + 0.3 * c.load.clamp(0.0, 1.2) } else { 0.45 };
        out += self.engine_noise.step(ENGINE_NOISE_PA * speed.powf(1.1) * load * if c.diesel { 1.6 } else { 1.0 });

        // ---- knock (detonation ping)
        if c.knock > 0.05 {
            self.knock_env = self.knock_env.max(0.25 * c.knock);
        }
        out += self.ping.step(n * self.knock_env) * 0.05;
        self.knock_env *= (-dt / 0.004).exp();

        // ---- starter motor, pinion and ring gear
        out += self.starter.step(&StarterCtx { on: c.starter_on, rpm: c.crank_rpm, strain: c.starter_strain });

        // ---- turbo: compressor whistle that rises with shaft speed, flutter on a lift
        self.spool_s += (c.spool - self.spool_s) * (1.0 - (-dt / 0.12).exp());
        self.throttle += (c.throttle - self.throttle) * (1.0 - (-dt / 0.08).exp());
        self.thr_slow += (self.throttle - self.thr_slow) * (1.0 - (-dt / 0.4).exp());
        if self.spool_s > 0.03 && c.combusting {
            let sp = self.spool_s;
            // a compressor wheel's whistle drifts about with the flow (about 1 %) and flutters in level: a perfectly
            // steady sine reads as a synthesiser
            let n2 = self.rng2.next();
            let f1 = (700.0 + 3300.0 * sp.powf(1.2)) * (1.0 + 0.010 * self.whistle_fm.step(n2));
            let flutter = (1.0 + 0.3 * self.whistle_am.step(self.rng2.next())).max(0.25);
            self.turbo_phase = (self.turbo_phase + f1 * dt).fract();
            self.turbo_phase2 = (self.turbo_phase2 + f1 * 0.617 * dt).fract();
            let tone = (self.turbo_phase * tau).sin() + 0.4 * (self.turbo_phase2 * tau).sin() + 0.2 * (self.turbo_phase * 2.0 * tau).sin();
            let drive = (0.25 + 0.75 * self.thr_slow.min(1.0)) * (0.3 + 0.7 * c.boost.clamp(0.0, 1.2));
            out += tone * flutter * 0.016 * sp.powf(1.5) * drive;
            self.turbo_lp += (n - self.turbo_lp) * (0.04 + 0.08 * sp);
            out += self.turbo_lp * 0.008 * sp * (0.3 + c.boost.min(1.2));
        }
        self.boost_hold = (self.boost_hold * (-dt / 0.6).exp()).max(c.boost);
        if self.throttle < 0.4 && self.bov_armed && self.thr_slow > 0.55 && self.boost_hold > 0.3 && c.spool > 0.2 && !c.diesel {
            self.bov_env = self.boost_hold.clamp(0.0, 1.2);
            self.bov_armed = false;
        }
        if self.throttle > 0.25 {
            self.bov_armed = true;
        }
        self.bov_lp += (n - self.bov_lp) * 0.35;
        out += (n - self.bov_lp) * self.bov_env * 0.2;
        self.bov_env *= (-dt / 0.2).exp();

        // ---- gearbox whine
        if c.gear_whine > 0.01 {
            // the mesh tone wanders a little and its level is modulated at the shaft's turn (eccentricity): sidebands
            // either side of the line, as in a real gearbox, instead of one pure sine
            let f = (180.0 + 28.0 * c.vehicle_speed) * (1.0 + 0.004 * self.gear_fm.step(self.rng2.next()));
            self.gear_phase = (self.gear_phase + f * dt).fract();
            self.gear_shaft = (self.gear_shaft + f / 23.0 * dt).fract();
            let am = (1.0 + 0.35 * (self.gear_shaft * tau).sin() + 0.2 * self.gear_am.step(self.rng2.next())).max(0.2);
            out += ((self.gear_phase * tau).sin() + 0.35 * (self.gear_phase * 3.0 * tau).sin()) * am * c.gear_whine * 0.003;
        }
        out
    }
}
