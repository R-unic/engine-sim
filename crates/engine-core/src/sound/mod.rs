//! The engine's sound: the gas path (cylinders, valves, exhaust network) solved in the time domain at
//! audio rate, the sound radiated from the tailpipe mouths, pipe shells and the machinery, mixed to a
//! mono signal.
//!
//! Nothing here plays back a recording. A tailpipe's note is the volume flow the exhaust network
//! delivers to its mouth, differentiated (a monopole); the pipe's lengths, diameters, temperatures and
//! materials, the muffler's chambers and the engine's firing order all act on that flow inside the
//! solver, so changing a part changes the sound for the physical reason.

pub mod combustion;
pub mod flow_noise;
pub mod lowend;
pub mod mechanical;
pub mod radiation;
pub mod starter;
pub mod tier_eq;
pub mod turbo;

use crate::acoustics::{Env, GasPath, MouthSample, PipeMaterial};
use crate::config::{EngineConfig, IgnitionType};
use crate::constants::{FREE_REV_EXTRA_INERTIA_PER_L, STARTER_DISENGAGE_RPM, STARTER_MAX_RPM, STARTER_MIN_NM_PER_L};
use crate::tuning::Tuning;
use mechanical::{MechCtx, Mechanical};
use radiation::{monopole_pressure, FlowHist, JetNoise, Lowpass4, ShellBody};

pub const SAMPLE_RATE: u32 = 44_100;

/// Ambient pressure of the pipe networks, Pa.
const MODEL_P_AMB: f32 = 101_325.0;

/// The cylinders of the pipe model compress without blow-by past the rings or heat lost to the walls, so their
/// compression torque on the crank is larger than the engine model's; this brings the speed ripple while
/// cranking to what the engine model has (checked against it by the `cranking_speed_ripple` test).
const CRANKING_TORQUE_SCALE: f32 = 0.35;
/// A running engine's crank speed deviates from the engine model's rpm by the torque ripple and the cycle-to-cycle scatter of the
/// combustion: the mean torque is followed over `RUN_MEAN_TAU_S` (slower changes are the engine model's), the idle control and the
/// load pull the speed back at `RUN_GOVERNOR_RAD_S` (rad/s, a corner near 1.5 Hz), and the deviation is bounded.
const RUN_MEAN_TAU_S: f32 = 0.15;
const RUN_GOVERNOR_RAD_S: f32 = 9.0;
const RUN_RIPPLE_LIMIT: f32 = 0.12;
/// How hard a block rattles at the reference charge (petrol, idle) and how fast it grows with the rate of pressure rise.
const RATTLE_AT_IDLE: f32 = 0.72;
const RATTLE_EXPONENT: f32 = 0.7;
/// Peak pressure (Pa at the listener, before the insertion loss of the cat and the muffler) of the crack of a pop per
/// (strength - 1)^1.3.
const CRACK_PA: f32 = 20.0;
/// Extra output gain at light-off (the engine catching and flaring above idle) and how long it takes to relax, s.
const ROAR_GAIN: f32 = 1.3;
const ROAR_TAU_S: f32 = 1.6;

/// Upper edge of what the pipe solver radiates, Hz. Its cells are centimetres long, so nothing above a few kHz
/// is resolved wave motion; what lies above that is the grid's own spikes at the steepened fronts of the pulses,
/// and the top end comes from the flow-noise layer instead (`flow_noise`).
const RADIATION_BAND_HZ: f32 = flow_noise::BAND_HZ;

/// Speed / accuracy trade of the pipe solver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// Cell size, m.
    pub dx: f32,
    /// The pipe solver runs on its own clock: a step is as long as the CFL limit allows, but at most `div`
    /// audio samples; the outputs are interpolated to the audio samples.
    pub div: u32,
    /// Scale on the junction solver's convergence tolerances (smaller = cleaner weak signals, slower).
    pub tol: f32,
}

/// How far apart the solver's outputs may be, in audio samples. The solver's own CFL step is what sets it (in hot
/// exhaust a step is 1 to 5 samples long, shorter on finer cells); the cap only stops a quiet pipe from taking
/// steps so long that the valve events are blurred.
const MAX_STEP_SAMPLES: u32 = 8;

impl Quality {
    /// 2 cm cells: a 2 kHz wave in hot exhaust is still 14 cells long, and the blow-down fronts are
    /// a few centimetres thick.
    pub const ULTRA: Quality = Quality { dx: 0.020, div: MAX_STEP_SAMPLES, tol: 1.0 };
    /// 2.8 cm cells.
    pub const HIGH: Quality = Quality { dx: 0.028, div: MAX_STEP_SAMPLES, tol: 1.0 };
    /// 4 cm cells: a 1 kHz wave in hot exhaust is fourteen cells long.
    pub const BALANCED: Quality = Quality { dx: 0.040, div: MAX_STEP_SAMPLES, tol: 1.0 };
    /// 5.5 cm cells.
    pub const FAST: Quality = Quality { dx: 0.055, div: MAX_STEP_SAMPLES, tol: 1.0 };
    /// 7 cm cells.
    pub const ECO: Quality = Quality { dx: 0.070, div: MAX_STEP_SAMPLES, tol: 1.0 };

    /// The tiers from finest to coarsest; level 1 is `ULTRA`.
    pub const LADDER: [Quality; 5] = [Quality::ULTRA, Quality::HIGH, Quality::BALANCED, Quality::FAST, Quality::ECO];
    pub const NAMES: [&'static str; 5] = ["ultra", "high", "balanced", "fast", "eco"];

    /// The tier at ladder position `level` (1 = finest), clamped to the ladder.
    pub fn level(level: u8) -> Quality {
        Quality::LADDER[(level.clamp(1, 5) - 1) as usize]
    }
}

/// What the engine model tells the sound model; the viewer refreshes it every frame.
#[derive(Clone, Debug, Default)]
pub struct SoundInput {
    pub rpm: f32,
    pub throttle: f32,
    /// Normalised brake load, 0..1.2.
    pub load: f32,
    pub map_pa: f32,
    pub atm_pa: f32,
    pub air_k: f32,
    pub charge_k: f32,
    pub afr: f32,
    pub egt_k: f32,
    pub boost: f32,
    pub spool: f32,
    pub running: bool,
    pub lit: bool,
    pub cranking: bool,
    pub knock: f32,
    pub vehicle_speed: f32,
    pub gear_whine: f32,
    /// Per cylinder (label - 1): how completely the last charge burned, 0 (misfire) .. 1.
    pub burn: Vec<f32>,
    /// Output gain, 0 when muted.
    pub master: f32,
    /// Bumped on every update so the rpm can glide between snapshots.
    pub serial: u64,
    /// Shaft speed of the turbo, 0..1.
    pub turbo_speed: f32,
    /// How readily this engine burbles on a lift-off (0 never .. 1 a V12 on the overrun); the viewer scales it
    /// up when the catalyst is deleted.
    pub burble: f32,
    /// Rising edge: an ignition fault just lit unburnt charge in the exhaust.
    pub backfire: f32,
}

/// Crank angle after the firing top dead centre at which the pressure rise of a combustion hits the block, and the
/// half-width of the triangular scatter of that moment (about 1.5 degrees rms).
const COMB_ANGLE_DEG: f32 = 366.0;
const COMB_JITTER_DEG: f32 = 3.7;

/// Default bank assignment when the caller has no layout: the same one the viewer's 3-D layout uses. V engines
/// alternate between their two banks, a W16 is two VR8s side by side (four banks of four, cylinders 1-4 on the
/// first) and a W24 three banks of eight with the labels counted from the other side.
pub fn default_banks(cfg: &EngineConfig) -> Vec<u8> {
    let nb = crate::config::bank_tilts_deg(&cfg.id).len().max(1);
    let n = cfg.cylinder_count;
    match crate::config::layout_of(&cfg.id) {
        "w16" => (0..n).map(|l| (l * 4 / n) as u8).collect(),
        "w24" => (0..n).map(|l| (2 - l * 3 / n) as u8).collect(),
        _ => (0..n).map(|l| (l % nb) as u8).collect(),
    }
}

#[inline(always)]
fn tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    0
}

pub struct EngineSound {
    pub gas: GasPath,
    pub cfg: EngineConfig,
    tuning: Tuning,
    fs: f32,
    pub input: SoundInput,
    shells: Vec<ShellBody>,
    jets: Vec<JetNoise>,
    mech: Mechanical,
    /// Listener distance from the tailpipes, m.
    pub listener_r: f32,
    // smoothing
    rpm: f32,
    rpm_from: f32,
    ramp: f32,
    ramp_step: f32,
    since_update: u32,
    last_serial: u64,
    throttle: f32,
    load: f32,
    boost: f32,
    master: f32,
    prev_local: Vec<f32>,
    /// Crank angle (degrees, 0..720 of the cylinder) at which each cylinder's next combustion thud reaches the block:
    /// redrawn every cycle, because combustions are never timed to the degree.
    comb_angle: Vec<f32>,
    // solver sub-rate: outputs of the last two solver steps, interpolated per audio sample
    div: u32,
    /// Length of the latest solver step, s, and the audio time since the start of that step.
    step_len: f32,
    step_clock: f32,
    mouth_prev: Vec<MouthSample>,
    mouth_cur: Vec<MouthSample>,
    imouth_prev: Vec<MouthSample>,
    imouth_cur: Vec<MouthSample>,
    /// Smooth history of each mouth's volume flow (exhaust, intake), differentiated for the radiation.
    flow_hist: Vec<FlowHist>,
    iflow_hist: Vec<FlowHist>,
    ijets: Vec<JetNoise>,
    /// Intake induction noise reaching the listener this sample (analysis).
    pub p_intake: f32,
    body_prev: Vec<f32>,
    body_cur: Vec<f32>,
    /// Tailpipe pressure at the listener for the current sample, Pa (analysis).
    pub p_tail: f32,
    /// Whether the radiation of a coarse tier is brought back to the body of the finest one (`tier_eq`); off only to
    /// measure the tiers as they are.
    pub tier_correction: bool,
    /// Pressure from the jets, the pipe shells and the machinery this sample (analysis).
    pub p_jet: f32,
    pub p_shell: f32,
    pub p_mech: f32,
    /// Radiated pressure of each shell body this sample (analysis).
    pub shell_out: Vec<f32>,
    // overrun burble state
    thr_slow: f32,
    rpm_slow: f32,
    lift_env: f32,
    prev_backfire: f32,
    pop_arm: Vec<f32>,
    // the burble comes in bursts (a rich pocket here, a lean one there): a two-state gate, and the cracks of the pops in flight
    burst_on: bool,
    burst_left: f32,
    pops_seen: u64,
    cracks: [(f32, f32); 12],
    crack_env: f32,
    crack_tau: f32,
    crack_hp: f32,
    crack_lp: f32,
    crack_lp2: f32,
    rng: u32,
    /// Pops fired and the loudness of the latest, for the tailpipe visuals.
    pub pops_fired: u64,
    pub pop_gain: f32,
    // cranking: the crank's speed ripple under the compression strokes, and what drives it
    /// Gas torque on the crank from the cylinders (Nm), its slow mean, and the speed deviation (rad/s) the
    /// starter's torque curve and the crank's inertia leave of it.
    tau_gas: f32,
    tau_mean: f32,
    crank_dw: f32,
    /// Inertia the starter turns (kg m^2), the slope of its torque-speed curve (Nm per rad/s) and its stall torque (Nm).
    crank_j: f32,
    starter_k: f32,
    starter_t0: f32,
    /// How hard the starter is working against compression, 0..1 (smoothed), for the sound of the motor.
    strain: f32,
    // the cylinders' pressure through the cycle: the torque ripple of a running engine and the strength of the rattle
    combustion: combustion::Combustion,
    charge: combustion::Charge,
    /// Cycle-to-cycle gain of each cylinder's burn (redrawn as it passes the firing top dead centre) and the angle it was at.
    cyl_gain: Vec<f32>,
    cyl_prev: Vec<f32>,
    /// Peak rate of pressure rise of the reference charge (a petrol engine at idle): what the rattle level was set at.
    rise_ref: f32,
    /// Rattle strength of the current charge, relative to the reference (see `Mechanical`).
    rattle: f32,
    // engine activity: 1 while the crank is driven, falling away once it stops, so a stopped engine is silent
    activity: f32,
    // the pipe solver resolves no more than a few kHz; the monopole radiation is band-limited to match
    tail_lp: Lowpass4,
    intake_lp: Lowpass4,
    // puts back what a coarse solver tier smooths away (`tier_eq`)
    tail_eq: tier_eq::TierEq,
    intake_eq: tier_eq::TierEq,
    // valve-jet noise above that band
    flow: flow_noise::FlowLayer,
    // the first firings after light-off are uneven
    catch: f32,
    /// The roar of the engine lighting and flaring: 1 at light-off, relaxing over a couple of seconds
    roar: f32,
    /// `roar` as the output gain sees it (eased in over about a tenth of a second, so the light-off is a swell, not a step)
    roar_gain: f32,
    was_unlit: bool,
    // the low orders of the crank, where the pipe model leaves them out
    lowend: lowend::LowEnd,
    /// What the low-end layer added this sample (analysis).
    pub p_low: f32,
    // loudness compressor
    env_rms: f32,
    // master chain
    dc_in: f32,
    dc_out: f32,
    limiter: f32,
    /// Pa at the listener that maps to digital full scale.
    pub full_scale_pa: f32,
    pub sample_index: u64,
    /// Cycle counters for the stages of a sample (diagnostics): gas path, events, radiation, mechanical.
    pub prof: [u64; 5],
}

impl EngineSound {
    pub fn new(cfg: &EngineConfig, tuning: &Tuning, banks: &[u8], quality: Quality) -> EngineSound {
        let mut env = Env::new(MODEL_P_AMB, 293.0);
        env.tol = quality.tol;
        let gas = GasPath::new(cfg, tuning, banks, quality.dx, env);
        let fs = SAMPLE_RATE as f32;
        let listener_r = 1.5;
        let shells: Vec<ShellBody> = gas
            .exhaust
            .bodies
            .iter()
            .map(|b| ShellBody::new(b, &b.wall.material.props(), listener_r, fs))
            .collect();
        let jets = gas.exhaust.tails.iter().enumerate().map(|(i, t)| JetNoise::new(t.diameter, 0x9E37 + i as u32 * 7919)).collect();
        let n = cfg.cylinder_count;
        // the same starter curve the engine model uses: stall torque, falling linearly to its free speed
        let starter_t0 = cfg.starter_torque_nm.max(STARTER_MIN_NM_PER_L * cfg.displacement_l() * cfg.compression_ratio / 10.5) as f32;
        let rise_ref = {
            let mut c = combustion::Combustion::new(cfg, gas.valves.exhaust.open);
            c.set_charge(&combustion::Charge { map_pa: 35_000.0, charge_k: 310.0, air_fuel_ratio: 14.7, ambient_pa: MODEL_P_AMB });
            c.peak_rise_raw()
        };
        EngineSound {
            cfg: cfg.clone(),
            tuning: tuning.clone(),
            fs,
            input: SoundInput { atm_pa: 101_325.0, air_k: 293.0, charge_k: 300.0, afr: 14.7, burn: vec![1.0; n], master: 1.0, ..Default::default() },
            shells,
            jets,
            mech: Mechanical::new(fs, cfg.displacement_l() as f32),
            listener_r,
            rpm: 0.0,
            rpm_from: 0.0,
            ramp: 1.0,
            ramp_step: 1.0 / 735.0,
            since_update: 0,
            last_serial: 0,
            throttle: 0.0,
            load: 0.0,
            boost: 0.0,
            master: 0.0,
            prev_local: vec![0.0; n],
            comb_angle: vec![COMB_ANGLE_DEG; n],
            div: quality.div.max(1),
            step_len: 0.0,
            step_clock: 0.0,
            mouth_prev: vec![MouthSample::default(); gas.mouths.len()],
            mouth_cur: vec![MouthSample::default(); gas.mouths.len()],
            imouth_prev: vec![MouthSample::default(); gas.intake_mouths.len()],
            imouth_cur: vec![MouthSample::default(); gas.intake_mouths.len()],
            flow_hist: vec![FlowHist::default(); gas.mouths.len()],
            iflow_hist: vec![FlowHist::default(); gas.intake_mouths.len()],
            ijets: gas.intake.mouths.iter().enumerate().map(|(i, m)| JetNoise::new(m.1, 0x4F31 + i as u32 * 6151)).collect(),
            p_intake: 0.0,
            body_prev: vec![101_325.0; gas.body_p.len()],
            body_cur: vec![101_325.0; gas.body_p.len()],
            p_tail: 0.0,
            tier_correction: true,
            p_jet: 0.0,
            p_shell: 0.0,
            p_mech: 0.0,
            shell_out: vec![0.0; gas.exhaust.bodies.len()],
            thr_slow: 0.0,
            rpm_slow: 0.0,
            lift_env: 0.0,
            prev_backfire: 0.0,
            pop_arm: vec![0.0; n],
            burst_on: false,
            burst_left: 0.0,
            pops_seen: 0,
            cracks: [(f32::MAX, 0.0); 12],
            crack_env: 0.0,
            crack_tau: 0.002,
            crack_hp: 0.0,
            crack_lp: 0.0,
            crack_lp2: 0.0,
            rng: 0x2545_F491,
            pops_fired: 0,
            pop_gain: 0.0,
            tau_gas: 0.0,
            tau_mean: 0.0,
            crank_dw: 0.0,
            crank_j: (cfg.rotating_inertia_kg_m2 + FREE_REV_EXTRA_INERTIA_PER_L * cfg.displacement_l()) as f32,
            starter_t0: starter_t0,
            starter_k: starter_t0 / (STARTER_MAX_RPM as f32 * std::f32::consts::TAU / 60.0),
            strain: 0.0,
            combustion: {
                let mut c = combustion::Combustion::new(cfg, gas.valves.exhaust.open);
                c.set_charge(&combustion::Charge { map_pa: 35_000.0, charge_k: 310.0, air_fuel_ratio: 14.7, ambient_pa: MODEL_P_AMB });
                c
            },
            charge: combustion::Charge { map_pa: 0.0, charge_k: 300.0, air_fuel_ratio: 14.7, ambient_pa: MODEL_P_AMB },
            cyl_gain: vec![1.0; n],
            cyl_prev: vec![0.0; n],
            rise_ref,
            rattle: RATTLE_AT_IDLE,
            activity: 0.0,
            tail_lp: Lowpass4::new(RADIATION_BAND_HZ, fs),
            intake_lp: Lowpass4::new(RADIATION_BAND_HZ, fs),
            tail_eq: tier_eq::TierEq::new(quality.dx, fs),
            intake_eq: tier_eq::TierEq::new(quality.dx, fs),
            flow: flow_noise::FlowLayer::new(fs, tuning, gas.exhaust.report.path_length, 470.0, !gas.exhaust.turbines.is_empty()),
            catch: 0.0,
            roar: 0.0,
            roar_gain: 0.0,
            was_unlit: false,
            lowend: lowend::LowEnd::for_cylinders(fs, n),
            p_low: 0.0,
            env_rms: 0.05,
            dc_in: 0.0,
            dc_out: 0.0,
            limiter: 0.0,
            full_scale_pa: 8.0,
            gas,
            sample_index: 0,
            prof: [0; 5],
        }
    }

    /// Makes this freshly built model carry on where `other` is: the same crank angle, speed glide, loudness
    /// compressor and limiter, cranking state. A model that replaces another (a finer solver tier, a part fitted)
    /// can then be cross-faded in without a jump in phase or in level - a cold one starts its compressor at a gain
    /// of its own and its crank at zero, which is a surge and a flam.
    pub fn align_with(&mut self, other: &EngineSound) {
        self.rpm = other.rpm;
        self.rpm_from = other.rpm_from;
        self.ramp = other.ramp;
        self.ramp_step = other.ramp_step;
        self.since_update = other.since_update;
        self.last_serial = other.last_serial;
        self.throttle = other.throttle;
        self.load = other.load;
        self.boost = other.boost;
        self.master = other.master;
        self.activity = other.activity;
        self.env_rms = other.env_rms;
        self.limiter = other.limiter;
        self.dc_in = other.dc_in;
        self.dc_out = other.dc_out;
        self.catch = other.catch;
        self.roar = other.roar;
        self.roar_gain = other.roar_gain;
        self.was_unlit = other.was_unlit;
        self.tau_gas = other.tau_gas;
        self.tau_mean = other.tau_mean;
        self.crank_dw = other.crank_dw;
        self.strain = other.strain;
        self.cyl_gain.clone_from(&other.cyl_gain);
        self.lowend = other.lowend.clone();
        self.gas.crank = other.gas.crank;
        self.gas.copy_state_from(&other.gas);
        // the same noise, sample for sample: the two models' hiss and rasp then cross-fade as one signal instead of
        // dipping 3 dB at the middle of the fade as two uncorrelated ones do
        self.mech.copy_noise_from(&other.mech);
        self.flow.copy_noise_from(&other.flow);
        self.jets.clone_from(&other.jets);
        self.ijets.clone_from(&other.ijets);
        self.input = other.input.clone();
    }

    /// Names and mode frequencies of the radiating shells (reports).
    pub fn shell_report(&self) -> Vec<(String, Vec<f32>)> {
        self.shells.iter().map(|s| (s.name.clone(), s.frequencies())).collect()
    }

    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    /// Starts the pipes at running temperature (tests and a restart of a warm engine).
    pub fn preheat(&mut self, egt_k: f32) {
        self.gas.exhaust.preheat(egt_k);
    }

    /// Runs the engine at its current input for `seconds` of audio time with the pipe walls heating
    /// `accel` times faster than real time, so a cold exhaust reaches its working temperature quickly.
    pub fn soak(&mut self, seconds: f32, accel: f32) {
        self.gas.exhaust.net.grid.thermal_accel = accel;
        let n = (seconds * self.fs) as usize;
        for _ in 0..n {
            self.next_sample();
        }
        self.gas.exhaust.net.grid.thermal_accel = 1.0;
    }

    /// Temperature of cylinder `i`'s gas when its exhaust valve opens, from the engine state. Burned-gas
    /// temperature climbs with load; a cylinder that did not fire only holds the compressed charge. (The
    /// pressure is not set: it follows from the mass the intake delivered, so a throttled engine's cylinder
    /// ends up *below* atmospheric at opening and the exhaust flows back in at idle, as in life.)
    pub fn evo_temperature(&self, i: usize, inp: &SoundInput) -> f32 {
        self.evo_temperature_for(inp.burn.get(i).copied().unwrap_or(1.0), inp)
    }

    fn evo_temperature_for(&self, burn: f32, inp: &SoundInput) -> f32 {
        let burn = burn.clamp(0.0, 1.0);
        let diesel = self.cfg.ignition_type == IgnitionType::Compression;
        let load = inp.load.clamp(0.0, 1.2);
        let t_fired = if diesel { 800.0 + 420.0 * load } else { 1020.0 + 430.0 * load.powf(0.8) };
        let t_unburnt = inp.charge_k.max(250.0) * 1.35;
        t_unburnt + (t_fired - t_unburnt) * burn
    }

    /// Takes a new snapshot of the engine state.
    pub fn set_input(&mut self, inp: &SoundInput) {
        // an engine catching after the starter has turned it over does not fire evenly at first: a cylinder
        // now and then burns a fraction of its charge, and the idle settles over about a second
        if self.was_unlit && inp.lit && inp.running {
            self.catch = 1.0;
            self.roar = 1.0;
        }
        self.was_unlit = inp.cranking && !inp.lit;
        for i in 0..self.cfg.cylinder_count {
            let mut burn = inp.burn.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0);
            if self.catch > 0.02 && self.rand() < 0.3 * self.catch {
                burn *= 0.35 + 0.5 * self.rand();
            }
            self.gas.t_evo[i] = self.evo_temperature_for(burn, inp);
            self.gas.fuel_frac[i] = burn / inp.afr.max(5.0);
        }
        self.input = inp.clone();
        // the pressure the cylinders reach follows the charge they were given
        let charge = combustion::Charge {
            map_pa: inp.map_pa.max(15_000.0),
            charge_k: inp.charge_k.max(250.0),
            air_fuel_ratio: inp.afr.max(5.0),
            ambient_pa: if inp.atm_pa > 5.0e4 { inp.atm_pa } else { MODEL_P_AMB },
        };
        let moved = |a: f32, b: f32| (a - b).abs() > 0.01 * b.abs().max(1.0);
        if moved(charge.map_pa, self.charge.map_pa) || moved(charge.air_fuel_ratio, self.charge.air_fuel_ratio) || moved(charge.charge_k, self.charge.charge_k) {
            self.combustion.set_charge(&charge);
            self.charge = charge;
            self.rattle = RATTLE_AT_IDLE * (self.combustion.peak_rise_bar_per_deg() / self.rise_ref).max(0.05).powf(RATTLE_EXPONENT);
        }
        // combustion scatters most at idle and settles as the load comes on
        self.gas.cyc_cov = 0.02 + 0.05 * (1.0 - inp.load.clamp(0.0, 1.0));
        // Both pipe networks sit in the same ambient (`MODEL_P_AMB`): the engine model's atmosphere (a function of
        // its air temperature) only enters as the manifold pressure *ratio*. Giving the intake a different ambient
        // than the exhaust blew a steady draught through any cylinder resting in valve overlap, and the solver
        // noise riding on it was an airy hiss with the engine off.
        let p_ratio = if inp.atm_pa > 5.0e4 { MODEL_P_AMB / inp.atm_pa } else { 1.0 };
        self.gas.map_target = (inp.map_pa * p_ratio).min(MODEL_P_AMB * 0.99);
        // the turbine extracts work in step with the shaft
        for &n in &self.gas.exhaust.turbines {
            self.gas.exhaust.net.nodes[n].extract = 0.16 * inp.spool.clamp(0.0, 1.0) * inp.throttle.clamp(0.0, 1.0);
        }
        if inp.air_k > 200.0 {
            self.gas.exhaust.net.grid.t_amb = inp.air_k;
        }
        // convective cooling of the pipes follows road speed
        self.gas.exhaust.net.grid.h_conv = 10.0 + 6.0 * inp.vehicle_speed.max(0.0).powf(0.8);
    }

    /// Roughly Gaussian (sum of four uniforms), unit variance.
    fn rand_gauss(&mut self) -> f32 {
        (self.rand() + self.rand() + self.rand() + self.rand() - 2.0) * 1.732
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / 16_777_216.0
    }

    /// Overrun burble and backfire: on a lift-off or a rev-matched downshift, a cylinder now and then holds unburnt charge
    /// that lights at the exhaust valve. The pop is armed here and fired by the gas path at that cylinder's next
    /// exhaust-valve opening, so its bang is a real blow-down.
    ///
    /// Real burble is not a steady patter. The unburnt charge comes in pockets (the mixture in the manifold is never even),
    /// so the pops arrive in bursts with quiet between; and their strength is anything but even: most are a soft thump,
    /// a few are a sharp crack that stands 20 dB and more out of the rest (recordings of rally cars on the overrun:
    /// median +16 dB over the surrounding noise, the loudest +25 to +30, a third of a second between bursts).
    fn arm_pops(&mut self, combusting: bool, dt: f32) {
        let inp = &self.input;
        self.thr_slow += (self.throttle - self.thr_slow) * (1.0 - (-dt / 0.4).exp());
        self.rpm_slow += (self.rpm - self.rpm_slow) * (1.0 - (-dt / 0.35).exp());
        if self.throttle < 0.08 {
            if self.thr_slow > 0.3 {
                self.lift_env = self.lift_env.max(self.thr_slow.min(1.0));
            }
            if self.rpm - self.rpm_slow > 250.0 {
                self.lift_env = self.lift_env.max(0.9);
            }
            self.lift_env *= (-dt / 3.0).exp();
        } else {
            self.lift_env *= (-dt / 0.15).exp();
        }
        let n = self.cfg.cylinder_count;
        let burble = inp.burble;
        let gate = if combusting { burble * self.lift_env * ((self.rpm - 1700.0) / 3000.0).clamp(0.0, 1.0) } else { 0.0 };
        let fire_hz = (self.rpm / 60.0 * n as f32 * 0.5).max(1.0);
        let backfire_edge = self.input.backfire > self.prev_backfire + 0.3;
        self.prev_backfire = self.input.backfire;
        // the bursts: on for a few tenths of a second, then a gap
        self.burst_left -= dt;
        if self.burst_left <= 0.0 {
            self.burst_on = !self.burst_on;
            let (u, v) = (self.rand(), self.rand());
            self.burst_left = if self.burst_on { 0.06 + 0.5 * u * v } else { 0.08 + 0.5 * u * u.sqrt() };
        }
        // pops per second on average (the bursts concentrate them); each cylinder's chance per cycle follows from it
        let rate = 40.0 * gate;
        let p_avg = rate / fire_hz;
        let p = if self.burst_on { (p_avg * 1.9).min(0.85) } else { p_avg * 0.1 };
        for i in 0..n {
            let local = self.gas.local(i);
            let prev = self.pop_arm[i];
            self.pop_arm[i] = local;
            // arm once per cycle as the cylinder passes 300 deg
            let crossed = if prev <= local { prev < 300.0 && local >= 300.0 } else { local >= 300.0 };
            if !crossed || self.gas.pop[i].is_some() || self.rpm < 300.0 {
                continue;
            }
            if backfire_edge && self.rand() < 0.5 {
                self.gas.pop[i] = Some((3.0 + 3.0 * self.rand(), 1900.0));
                self.pop_gain = 1.3;
                continue;
            }
            if p > 0.0 && self.rand() < p {
                // log-normal strength: the soft ones are common, a few are many times the median
                let z = self.rand_gauss();
                let s = (0.62 * z).exp();
                let mult = (1.35 + 1.45 * s * s).min(9.0);
                let t = (1450.0 + 90.0 * mult).min(2150.0);
                self.gas.pop[i] = Some((mult, t));
                self.pop_gain = ((mult - 1.35) / 6.0).clamp(0.2, 1.3);
            }
        }
        // report to the viewer
        self.pops_fired = self.gas.pops_fired;
    }

    /// Renders `out.len()` samples.
    pub fn render(&mut self, out: &mut [f32]) {
        for o in out.iter_mut() {
            *o = self.next_sample();
        }
    }

    pub fn next_sample(&mut self) -> f32 {
        let dt = 1.0 / self.fs;
        self.sample_index += 1;
        // glide the rpm between snapshots so a fast rev is a smooth pitch slide, not a staircase
        self.since_update += 1;
        if self.input.serial != self.last_serial {
            self.last_serial = self.input.serial;
            self.ramp_step = 1.0 / (self.since_update as f32).clamp(200.0, 4000.0);
            self.since_update = 0;
            self.rpm_from = self.rpm;
            self.ramp = 0.0;
        }
        self.ramp = (self.ramp + self.ramp_step).min(1.0);
        self.rpm = self.rpm_from + (self.input.rpm - self.rpm_from) * self.ramp;
        let k = 1.0 - (-dt / 0.08).exp();
        self.throttle += (self.input.throttle - self.throttle) * k;
        self.load += (self.input.load - self.load) * k;
        self.boost += (self.input.boost - self.boost) * k;
        self.master += (self.input.master - self.master) * k;

        let (in_running, in_lit, in_cranking) = (self.input.running, self.input.lit, self.input.cranking);
        let (in_spool, in_knock, in_gear, in_speed) = (self.input.spool, self.input.knock, self.input.gear_whine, self.input.vehicle_speed);
        let running = in_running && self.rpm > 20.0;
        let combusting = running && in_lit;
        // the pinion lets go as the engine runs away from the starter (the engine model cuts the starter at 500)
        let starter_on = in_cranking && self.rpm < STARTER_DISENGAGE_RPM as f32 - 20.0;

        // ---- cranking: every compression stroke slows the crank and the one after it lets it surge back, in
        // step with how the starter's torque curve and the crank's inertia let it
        let w_bar = self.rpm * (std::f32::consts::TAU / 60.0);
        if starter_on && w_bar > 5.0 {
            self.tau_mean += (self.tau_gas - self.tau_mean) * (1.0 - (-dt / 0.6).exp());
            let tau_ac = self.tau_gas - self.tau_mean;
            self.crank_dw += dt * (tau_ac - self.starter_k * self.crank_dw) / self.crank_j;
            let lim = 0.5 * w_bar;
            self.crank_dw = self.crank_dw.clamp(-lim, lim);
            let target = (-tau_ac / (0.9 * self.starter_t0)).clamp(0.0, 1.0);
            self.strain += (target - self.strain) * (1.0 - (-dt / 0.02).exp());
        } else if combusting && w_bar > 5.0 {
            // a running engine: the same crank, driven by the combustion torque, held to its speed by the idle control and
            // the load (a leak with a corner near 1.5 Hz). What changes slower than that is the engine model's own rpm.
            self.tau_mean += (self.tau_gas - self.tau_mean) * (1.0 - (-dt / RUN_MEAN_TAU_S).exp());
            let tau_ac = self.tau_gas - self.tau_mean;
            self.crank_dw += dt * (tau_ac - self.crank_j * RUN_GOVERNOR_RAD_S * self.crank_dw) / self.crank_j;
            let lim = RUN_RIPPLE_LIMIT * w_bar;
            self.crank_dw = self.crank_dw.clamp(-lim, lim);
            self.strain *= (-dt / 0.05).exp();
        } else {
            let k = (-dt / 0.05).exp();
            self.crank_dw *= k;
            self.strain *= k;
            self.tau_mean *= k;
        }
        let crank_rpm = if w_bar > 5.0 { self.rpm * (1.0 + self.crank_dw / w_bar) } else { self.rpm };

        // ---- the gas path: the solver steps at its CFL limit (at most `div` samples), the outputs are
        // interpolated to the audio sample
        let t0 = tsc();
        let driven = running || in_cranking;
        let rpm_eff = if driven { crank_rpm } else { 0.0 };
        self.catch *= (-dt / 0.9).exp();
        self.roar *= (-dt / ROAR_TAU_S).exp();
        self.roar_gain += (self.roar - self.roar_gain) * (1.0 - (-dt / 0.12).exp());
        // radiation follows the engine turning: a stopped engine is silent, however the solver's round-off
        // noise would otherwise sound once the compressor below lifts it
        let k_act = 1.0 - (-dt / if rpm_eff > 25.0 || in_cranking { 0.03 } else { 0.2 }).exp();
        self.activity += ((rpm_eff > 25.0 || in_cranking) as u8 as f32 - self.activity) * k_act;
        let n = self.cfg.cylinder_count;
        let (mut combustion, mut valve_events) = (0.0f32, 0.0f32);
        self.step_clock += dt;
        while self.step_clock > self.step_len {
            self.step_clock -= self.step_len;
            let rate = self.gas.exhaust.net.grid.max_rate.max(self.gas.intake.net.grid.max_rate).max(1.0);
            let h = (0.88 / rate).min(self.div as f32 * dt).max(0.25 * dt);
            self.gas.step(h, rpm_eff);
            self.step_len = h;
            let (jet_x, jet_i) = self.gas.jet_powers();
            self.flow.set_jets(jet_x, jet_i);
            for k in 0..self.mouth_cur.len() {
                self.mouth_prev[k] = self.mouth_cur[k];
                self.mouth_cur[k] = self.gas.mouths[k];
                self.flow_hist[k].push(self.mouth_cur[k].q, h);
            }
            for k in 0..self.imouth_cur.len() {
                self.imouth_prev[k] = self.imouth_cur[k];
                self.imouth_cur[k] = self.gas.intake_mouths[k];
                self.iflow_hist[k].push(self.imouth_cur[k].q, h);
            }
            self.body_prev.copy_from_slice(&self.body_cur);
            self.body_cur.copy_from_slice(&self.gas.body_p);
            // torque the gas in the cylinders puts on the crank (a compression stroke resists, an expansion pushes)
            if starter_on {
                let mut tg = 0.0;
                for i in 0..n {
                    tg += (self.gas.res[i].p() - MODEL_P_AMB) * self.gas.geom.dv_dtheta(self.gas.local(i));
                }
                self.tau_gas = tg * CRANKING_TORQUE_SCALE;
            } else if combusting {
                // the burning cylinders' torque: the pulses that sag and surge the crank within every cycle, and, as
                // their strength scatters from cycle to cycle, make it wander
                let mut tg = 0.0;
                for i in 0..n {
                    let local = self.gas.local(i);
                    let prev = self.cyl_prev[i];
                    // a new cycle's burn begins as the cylinder passes its firing top dead centre
                    if (prev < 360.0) != (local < 360.0) && local >= 360.0 {
                        self.cyl_gain[i] = 1.0 + self.gas.cyc_cov * self.rand_gauss() + self.gas.cylinder_bias(i);
                    }
                    self.cyl_prev[i] = local;
                    tg += self.combustion.torque(local, self.input.burn.get(i).copied().unwrap_or(1.0) * self.cyl_gain[i]);
                }
                self.tau_gas = tg;
            }
            // engine events for the mechanical layer (firing, valve motion)
            if rpm_eff > 15.0 {
                let vt = self.gas.valves;
                let diesel = self.cfg.ignition_type == IgnitionType::Compression;
                for i in 0..n {
                    let local = self.gas.local(i);
                    let prev = self.prev_local[i];
                    let crossed = |a: f32| if prev <= local { prev < a && local >= a } else { prev < a || local >= a };
                    if combusting && crossed(self.comb_angle[i]) {
                        // no two combustions are alike: how hard the block is hit and when (cylinder to cylinder
                        // and cycle to cycle) scatter, so the thuds are a rough rattle and not a pulse train whose
                        // every harmonic stands as a clean tone
                        // (a diesel's clatter is the sharper, and varies the more: the ignition delay of every injection differs,
                        // and a clatter that ticks over at exactly the firing rate is a buzz at the block's mode)
                        let (hard, shift) = if diesel {
                            (0.5 + 1.0 * self.rand(), 1.6 * COMB_JITTER_DEG * (self.rand() + self.rand() - 1.0))
                        } else {
                            (0.6 + 0.8 * self.rand(), COMB_JITTER_DEG * (self.rand() + self.rand() - 1.0))
                        };
                        combustion += self.input.burn.get(i).copied().unwrap_or(1.0) * hard;
                        self.comb_angle[i] = COMB_ANGLE_DEG + shift;
                    }
                    for a in [vt.exhaust.open, vt.exhaust.close(), vt.intake.open, vt.intake.close()] {
                        if crossed(a) {
                            valve_events += 0.35;
                        }
                    }
                    self.prev_local[i] = local;
                }
            }
            self.arm_pops(combusting, h);
        }
        let frac = (self.step_clock / self.step_len).clamp(0.0, 1.0);
        let t1 = tsc();

        let t2 = tsc();
        // ---- radiation
        let r = self.listener_r;
        let act = self.activity;
        let mut p = 0.0f32;
        let mut p_tail = 0.0f32;
        let mut p_jet = 0.0f32;
        for k in 0..self.mouth_cur.len() {
            let (a, c) = (self.mouth_prev[k], self.mouth_cur[k]);
            p_tail += monopole_pressure(self.flow_hist[k].dq_dt(frac), r);
            p_jet += self.jets[k].step(a.u + (c.u - a.u) * frac, r, self.fs);
        }
        let p_tail_eq = self.tail_eq.step(p_tail);
        p_tail = self.tail_lp.step(if self.tier_correction { p_tail_eq } else { p_tail }) * act;
        let (flow_x, flow_i) = self.flow.step(r, self.listener_r * 1.2, self.gas.map_target / (0.97 * MODEL_P_AMB), self.rpm);
        let crack = self.crack_step(dt, n) * act;
        p_jet = (p_jet + flow_x) * act;
        // induction noise from the intake mouths; a turbo's compressor and charge piping muffle it
        let turbo_mute = 1.0 - 0.85 * in_spool.clamp(0.0, 1.0);
        let ri = self.listener_r * 1.2;
        let (mut p_in, mut p_in_jet) = (0.0f32, 0.0f32);
        for k in 0..self.imouth_cur.len() {
            let (a, c) = (self.imouth_prev[k], self.imouth_cur[k]);
            p_in += monopole_pressure(self.iflow_hist[k].dq_dt(frac), ri);
            p_in_jet += self.ijets[k].step(a.u + (c.u - a.u) * frac, ri, self.fs);
        }
        let p_in_eq = self.intake_eq.step(p_in);
        p_in = (self.intake_lp.step(if self.tier_correction { p_in_eq } else { p_in }) + p_in_jet + flow_i) * turbo_mute * act;
        self.p_intake = p_in;
        p += p_in;
        p += p_tail + p_jet;
        self.p_tail = p_tail;
        self.p_jet = p_jet;
        let mut p_shell = 0.0f32;
        for (b, sh) in self.shells.iter_mut().enumerate() {
            let pb = self.body_prev[b] + (self.body_cur[b] - self.body_prev[b]) * frac;
            let o = sh.step(pb, dt) * act;
            self.shell_out[b] = o;
            p_shell += o;
        }
        p += p_shell;
        self.p_shell = p_shell;
        let t3 = tsc();
        let p_mech = self.mech.step(&MechCtx {
            rpm: self.rpm,
            throttle: self.throttle,
            load: self.load,
            spool: in_spool,
            boost: self.boost,
            starter_on,
            crank_rpm,
            starter_strain: self.strain,
            combusting,
            knock: in_knock,
            gear_whine: in_gear,
            vehicle_speed: in_speed,
            combustion,
            valve_events,
            diesel: self.cfg.ignition_type == IgnitionType::Compression,
            rattle: self.rattle,
            rpm_frac: self.rpm / self.cfg.redline_rpm as f32,
            fire_phase: (self.gas.crank * n as f32 / 720.0).fract(),
            blow_off: self.tuning.blow_off,
            lift_off_pop: 0.0,
        });
        p += p_mech;
        self.p_mech = p_mech;
        // what an engine of perfectly matched cylinders has no orders for
        self.p_low = self.lowend.step(p, self.gas.crank, self.rpm, act);
        p += self.p_low;

        let t4 = tsc();
        self.prof[0] += t1 - t0;
        self.prof[1] += t2 - t1;
        self.prof[2] += t3 - t2;
        self.prof[3] += t4 - t3;
        // ---- loudness: a slow compressor keeps idle audible and full throttle bearable (about a third of
        // the real dynamic range is given up), then DC block and soft limiter
        self.env_rms += (p * p - self.env_rms) * (1.0 - (-dt / 0.35).exp());
        let comp = (self.env_rms.max(1.0e-4).sqrt() / 1.0).powf(-0.30).clamp(0.4, 6.0);
        // the light-off roar is not leveled away: the compressor needs a third of a second to follow it
        let x = p * comp * (1.0 + ROAR_GAIN * self.roar_gain) / self.full_scale_pa;
        let hp = x - self.dc_in + 0.9985 * self.dc_out;
        self.dc_in = x;
        self.dc_out = hp;
        let env = hp.abs();
        let rel = if env > self.limiter { 0.02 } else { 0.0003 };
        self.limiter += (env - self.limiter) * rel;
        let g = if self.limiter > 0.5 { 0.5 / self.limiter } else { 1.0 };
        // a pop's crack does not go through the loudness compressor and the limiter: it is a transient and has to stay one
        let bang = crack / self.full_scale_pa;
        ((hp * g + bang) * 1.4).tanh() * 0.8 * self.master
    }

    /// Turbo shaft speed, 0..1 (analysis).
    pub fn turbo_shaft(&self) -> f32 {
        self.mech.turbo_shaft()
    }

    /// How hard the combustion hits the block now, relative to a petrol engine at idle (analysis).
    pub fn rattle_level(&self) -> f32 {
        self.rattle
    }

    /// The sharp top of the pops. The pipe solver radiates up to 2.8 kHz; a combustion bang is a shock that steepens in the
    /// pipe and leaves it as a crack with energy to 8 kHz and more, a few milliseconds long, delayed by the path to the mouth
    /// and reduced by what is fitted: through a stock cat and can almost none of it gets out (the thump does), with the
    /// cat and the muffler gone it is the sharp crackle of a deleted car.
    fn crack_step(&mut self, dt: f32, n_cyl: usize) -> f32 {
        let _ = n_cyl;
        if self.gas.pops_fired != self.pops_seen {
            let new = self.gas.pops_fired.saturating_sub(self.pops_seen).min(4);
            self.pops_seen = self.gas.pops_fired;
            let mult = self.gas.last_pop_mult;
            for _ in 0..new {
                if let Some(slot) = self.cracks.iter_mut().find(|c| c.0 == f32::MAX) {
                    // louder ones are longer and carry more: the amplitude grows faster than the energy of the pop
                    *slot = (self.flow.exhaust_delay_samples() as f32, CRACK_PA * (mult - 1.0).max(0.0).powf(1.3) * self.flow.exhaust_transmission());
                }
            }
        }
        let mut fired = 0.0f32;
        for c in self.cracks.iter_mut() {
            if c.0 != f32::MAX {
                c.0 -= 1.0;
                if c.0 <= 0.0 {
                    fired = fired.max(c.1);
                    *c = (f32::MAX, 0.0);
                }
            }
        }
        if fired > 0.0 {
            // a new bang: the envelope jumps (never above the loudest in flight), its length grows with its strength
            self.crack_env = self.crack_env.max(fired);
            self.crack_tau = 0.0012 + 0.0028 * (fired / (CRACK_PA * 6.0)).clamp(0.0, 1.0);
        }
        self.crack_env *= (-dt / self.crack_tau).exp();
        if self.crack_env < 1.0e-6 {
            self.crack_env = 0.0;
            return 0.0;
        }
        let w = self.rand_gauss() * 0.7;
        let tau = std::f32::consts::TAU;
        self.crack_hp += (w - self.crack_hp) * (1.0 - (-tau * 1100.0 * dt).exp());
        let x = w - self.crack_hp;
        self.crack_lp += (x - self.crack_lp) * (1.0 - (-tau * 9000.0 * dt).exp());
        self.crack_lp2 += (self.crack_lp - self.crack_lp2) * (1.0 - (-tau * 9000.0 * dt).exp());
        self.crack_lp2 * self.crack_env
    }

    /// Fractional deviation of the crank's instantaneous speed from the mean while cranking (analysis).
    pub fn crank_speed_deviation(&self) -> f32 {
        let w = self.rpm * (std::f32::consts::TAU / 60.0);
        if w > 5.0 {
            self.crank_dw / w
        } else {
            0.0
        }
    }

    /// Pressure at the listener from the tailpipes alone, Pa (for analysis).
    pub fn tail_pressure(&self) -> f32 {
        self.p_tail
    }

    pub fn pipe_material(&self) -> PipeMaterial {
        self.tuning.pipe_material
    }
}
