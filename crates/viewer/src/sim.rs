//! Bridges the headless engine-core sim into Bevy: steps it in fine crank-angle increments,
//! records per-angle combustion tables, and keeps a "visual" crank angle so the 3D rig can play
//! back at any speed without strobing.

use bevy::prelude::*;
use engine_core::*;
use std::sync::{Arc, Mutex};

/// What a rig rebuild depends on. Boost pressure, for instance, never changes how the car looks.
pub type VisualKey = (AirFilter, Runners, Header, Cat, Muffler, u32, Option<u8>, u32, Intercooler);

pub fn visual_key(t: &Tuning) -> VisualKey {
    (
        t.air_filter,
        t.runners,
        t.header,
        t.cat,
        t.muffler,
        (t.pipe_scale * 100.0).round() as u32,
        t.turbo_count,
        (t.turbo_size * 100.0).round() as u32,
        t.intercooler,
    )
}

/// Per-cylinder combustion state indexed by local crank angle (1 degree bins over 720).
#[derive(Clone)]
pub struct CycleTable {
    pub pressure: Vec<f32>,
    pub burn: Vec<f32>,
    /// Bit flags per bin: 1 = spark fired, 2 = misfire, 4 = backfire.
    pub flags: Vec<u8>,
}

impl CycleTable {
    fn new() -> Self {
        Self { pressure: vec![101_325.0; 720], burn: vec![0.0; 720], flags: vec![0; 720] }
    }

    pub fn pressure_at(&self, deg: f32) -> f32 {
        lerp_table(&self.pressure, deg)
    }

    pub fn burn_at(&self, deg: f32) -> f32 {
        lerp_table(&self.burn, deg)
    }
}

fn lerp_table(t: &[f32], deg: f32) -> f32 {
    let d = deg.rem_euclid(720.0);
    let i = d.floor() as usize % 720;
    let f = d - d.floor();
    t[i] * (1.0 - f) + t[(i + 1) % 720] * f
}

/// Numbers the sound worker publishes for the UI.
#[derive(Default, Clone)]
pub struct AudioDiag {
    /// Pipe-solver cells and nodes in the live exhaust + intake.
    pub cells: usize,
    pub nodes: usize,
    /// Fraction of real time the sound worker spends rendering (1.0 = it can barely keep up).
    pub load: f32,
    /// Ladder level of the solver: 1 = ultra, 2 = high, 3 = balanced, 4 = fast, 5 = eco.
    pub quality: u8,
    pub exhaust_path_m: f32,
    pub primary_m: f32,
    pub tail_gas_k: f32,
    pub header_gas_k: f32,
    pub resonator_hz: f32,
    /// Estimated quarter-wave drone frequency of the whole pipe at its present gas temperature, Hz.
    pub drone_hz: f32,
    pub underruns: u64,
}

/// Data shared with the sound worker thread: the engine state it needs, what it is built from, and what
/// it reports back.
#[derive(Default, Clone)]
pub struct AudioState {
    pub rpm: f32,
    pub throttle: f32,
    pub load: f32,
    pub running: bool,
    pub cranking: bool,
    pub lit: bool,
    pub boost: f32,
    pub spool: f32,
    pub master: f32,
    pub map_pa: f32,
    pub atm_pa: f32,
    pub air_k: f32,
    pub charge_k: f32,
    pub afr: f32,
    pub egt_k: f32,
    pub knock: f32,
    pub vehicle_speed: f32,
    pub gear_whine: f32,
    /// How completely each cylinder's last charge burned (0 = misfire), by label - 1.
    pub burn: Vec<f32>,
    /// How readily this engine burbles on a lift-off, and the latest backfire flash.
    pub burble: f32,
    pub backfire: f32,
    /// What the worker builds the acoustic model from.
    pub engine_id: String,
    pub tuning: Tuning,
    pub banks: Vec<u8>,
    /// 0 = automatic (the finest tier the machine keeps up with), 1..=5 = a fixed tier of the ladder.
    pub quality_pref: u8,
    /// Bumped on every sim frame so the synth can glide between snapshots instead of stepping.
    pub serial: u64,
    /// Burble pops the synth has fired so far, and the loudness of the latest (the exhaust visuals fire
    /// a gas puff for each, so what you see is what you hear). Written by the worker.
    pub pops_fired: u64,
    pub pop_gain: f32,
    pub diag: AudioDiag,
}

/// Cold-start roar: seconds of throttle blip after light-off.
const FLARE_RISE_S: f32 = 0.2;
const FLARE_DECAY_S: f32 = 0.8;
const FLARE_PEAK_THROTTLE: f32 = 1.0;
/// Starter cranks this long before the engine lights.
const IGNITION_HOLDOFF_S: f64 = 1.1;

/// How much each engine burbles on a lift-off. Big multi-cylinder and sporty engines pop on the
/// overrun; a diesel never does.
fn burble_amount(id: &str) -> f32 {
    match engine_core::layout_of(id) {
        "v12" => 1.0,
        "v16" => 0.9,
        "w16" | "w24" => 0.8,
        "v8" => 0.6,
        "i6" => 0.5,
        "v6" => 0.4,
        "vtwin" => 0.3,
        "i4" => 0.2,
        _ => 0.0,
    }
}

#[derive(Resource)]
pub struct Sim {
    pub engine: CombustionEngine,
    pub tel: EngineTelemetry,
    pub engines: Vec<EngineConfig>,
    pub transmissions: Vec<TransmissionConfig>,
    pub brakes: Vec<BrakeConfig>,
    pub fuels: Vec<FuelConfig>,
    pub engine_idx: usize,
    pub trans_idx: usize,
    pub brake_idx: usize,
    pub fuel_idx: usize,
    pub gear_idx: usize,
    /// Bolt-on parts. Edit through [`Sim::set_tuning`] so the engine and the dyno follow.
    pub tuning: Tuning,
    /// Bumped whenever `tuning` or the engine/fuel changes, so the background dyno knows to re-run.
    pub tune_serial: u32,
    /// The parts the 3D rig was last built with (see `rig_system`).
    pub built_key: Option<VisualKey>,

    pub throttle_input: f32,
    pub brake_input: f32,
    pub time_scale: f32,
    pub paused: bool,
    pub starter_held: bool,
    pub starter_held_by_ui: bool,
    pub starter_script: bool,
    pub throttle_slider_active: bool,
    pub dyno_load: f32,
    /// Hold the starter until the engine catches (the quick-start button), then let go by itself.
    pub auto_start: bool,
    /// The crank is being turned by hand from the cycle timeline (paused), so the glow and spark
    /// effects follow the angle exactly instead of persisting for a few frames.
    pub scrub: bool,
    /// The cylinder the analysis panel is following; its sleeve glows in the colour of its stroke.
    pub focus_cyl: Option<usize>,

    /// Per-cylinder tables in physical-label order minus one (index = label - 1).
    pub tables: Vec<CycleTable>,
    /// Sim phase offset (deg) per label-1.
    pub offsets: Vec<f32>,
    /// Telemetry index per label-1.
    pub tel_idx: Vec<usize>,
    last_bin: Vec<usize>,

    // Visual playback
    pub visual_cap_rpm: f32, // 0 = real-time
    pub dilation: f32,
    pub alpha_v: f64,        // visual crank angle, deg, unwrapped
    pub alpha_v_prev: f64,
    pub wheel_angle: f32,    // visual wheel rotation, rad
    pub shaft_angle: f32,    // output shaft rotation, rad
    pub input_angle: f32,    // gearbox input shaft rotation, rad
    pub turbo_angle: f32,
    pub floor_offset: f32,
    pub vis_dt: f32,
    /// Time-averaged crank speed over the last frame (rpm) - immune to per-firing ripple.
    pub rpm_avg: f32,
    pub rebuild: bool,
    pub hide_ui: bool,
    pub master_volume: f32,
    pub auto_shift: bool,
    shift_cooldown: f32,
    pub muted: bool,
    /// Speedometer unit: false = mph (default), true = km/h.
    pub speed_kph: bool,
    /// Seconds since the engine lit (None = not in the startup roar).
    flare_t: Option<f32>,
    was_unlit: bool,
    /// Exhaust bank per label-1.
    banks: Vec<u8>,

    pub audio: Arc<Mutex<AudioState>>,
    /// Backfire visual timers per label-1.
    pub backfire_flash: Vec<f32>,
    pub misfire_flash: Vec<f32>,
    pub spark_flash: Vec<f32>,
    pub sim_time: f64,
    /// True when a synth is running and reporting its pops; otherwise `step` generates them itself.
    pub audio_live: bool,
    /// Exhaust pops this frame (count and the loudest strength 0..1.3), consumed by `puffs`.
    pub pops_this_frame: u32,
    pub pop_strength: f32,
    last_pops_fired: u64,
    thr_slow_v: f32,
    lift_env_v: f32,
    pop_rng: u32,
}

impl Sim {
    pub fn new(audio: Arc<Mutex<AudioState>>, engine_id: &str) -> Self {
        let engines = engines();
        let engine_idx = engines.iter().position(|e| e.id == engine_id).unwrap_or(0);
        let transmissions = transmissions();
        let brakes = brakes();
        let fuels = fuels();
        let cfg = engines[engine_idx].clone();
        let trans_idx = 1;
        let fuel_idx = fuels.iter().position(|f| f.ignition_type == cfg.ignition_type).unwrap();
        let mut engine = CombustionEngine::new(cfg.clone(), &transmissions[trans_idx], &brakes[0], fuels[fuel_idx].clone());
        engine.set_ignition_holdoff(IGNITION_HOLDOFF_S);
        let tel = engine.telemetry();
        let mut s = Self {
            engine,
            tel,
            engines,
            transmissions,
            brakes,
            fuels,
            engine_idx,
            trans_idx,
            brake_idx: 0,
            fuel_idx,
            gear_idx: 1,
            tuning: Tuning::default(),
            tune_serial: 1,
            built_key: None,
            throttle_input: 0.0,
            brake_input: 0.0,
            time_scale: 1.0,
            paused: false,
            starter_held: false,
            starter_held_by_ui: false,
            starter_script: false,
            throttle_slider_active: false,
            dyno_load: 0.0,
            auto_start: false,
            scrub: false,
            focus_cyl: None,
            tables: vec![],
            offsets: vec![],
            tel_idx: vec![],
            last_bin: vec![],
            visual_cap_rpm: 420.0,
            dilation: 1.0,
            alpha_v: 0.0,
            alpha_v_prev: 0.0,
            wheel_angle: 0.0,
            shaft_angle: 0.0,
            input_angle: 0.0,
            turbo_angle: 0.0,
            floor_offset: 0.0,
            vis_dt: 0.0,
            rpm_avg: 0.0,
            rebuild: true,
            hide_ui: false,
            master_volume: 0.8,
            auto_shift: true,
            shift_cooldown: 0.0,
            muted: false,
            speed_kph: false,
            flare_t: None,
            was_unlit: false,
            banks: vec![],
            audio,
            backfire_flash: vec![],
            misfire_flash: vec![],
            spark_flash: vec![],
            sim_time: 0.0,
            audio_live: false,
            pops_this_frame: 0,
            pop_strength: 0.0,
            last_pops_fired: 0,
            thr_slow_v: 0.0,
            lift_env_v: 0.0,
            pop_rng: 0x2545_F491,
        };
        s.reset_tables();
        s
    }

    pub fn cfg(&self) -> &EngineConfig {
        &self.engines[self.engine_idx]
    }

    fn reset_tables(&mut self) {
        let cfg = self.engines[self.engine_idx].clone();
        let n = cfg.cylinder_count;
        self.tables = vec![CycleTable::new(); n];
        self.offsets = vec![0.0; n];
        self.tel_idx = vec![0; n];
        let mut cum = 0.0;
        for i in 0..n {
            let label = cfg.firing_order[i] as usize;
            self.offsets[label - 1] = cum as f32;
            self.tel_idx[label - 1] = i;
            cum += cfg.firing_intervals_deg[i];
        }
        self.last_bin = vec![0; n];
        let lay = crate::layout::Layout::plan(&cfg);
        self.banks = (0..n).map(|l| lay.cyls[l].bank as u8).collect();
        self.flare_t = None;
        self.was_unlit = false;
        self.backfire_flash = vec![0.0; n];
        self.misfire_flash = vec![0.0; n];
        self.spark_flash = vec![0.0; n];
    }

    /// Swap in a different engine, keeping the current gearbox/brakes.
    pub fn select_engine(&mut self, idx: usize) {
        self.engine_idx = idx;
        let cfg = self.engines[idx].clone();
        if self.fuels[self.fuel_idx].ignition_type != cfg.ignition_type {
            self.fuel_idx = self.fuels.iter().position(|f| f.ignition_type == cfg.ignition_type).unwrap();
        }
        self.engine = CombustionEngine::new(
            cfg,
            &self.transmissions[self.trans_idx],
            &self.brakes[self.brake_idx],
            self.fuels[self.fuel_idx].clone(),
        );
        self.engine.set_ignition_holdoff(IGNITION_HOLDOFF_S);
        self.gear_idx = 1;
        self.tuning = Tuning::default();
        self.tune_serial += 1;
        self.reset_tables();
        self.alpha_v = 0.0;
        self.tel = self.engine.telemetry();
        self.rebuild = true;
    }

    pub fn select_transmission(&mut self, idx: usize) {
        self.trans_idx = idx;
        self.engine.set_transmission(&self.transmissions[idx]);
        self.gear_idx = 1;
        self.rebuild = true;
    }

    pub fn select_brake(&mut self, idx: usize) {
        self.brake_idx = idx;
        self.engine.set_brake_system(&self.brakes[idx]);
    }

    pub fn select_fuel(&mut self, idx: usize) {
        if self.fuels[idx].ignition_type != self.cfg().ignition_type {
            return;
        }
        self.fuel_idx = idx;
        self.engine.set_fuel(self.fuels[idx].clone());
        self.tune_serial += 1;
    }

    /// Bolts a new set of parts on. The engine and the dyno update at once; the 3D rig is only
    /// rebuilt when `rebuild_rig` is set (continuous sliders do that on release, not every frame).
    pub fn set_tuning(&mut self, t: Tuning, rebuild_rig: bool) {
        self.engine.set_tuning(t.clone());
        self.tuning = t;
        self.tune_serial += 1;
        if rebuild_rig && self.built_key != Some(visual_key(&self.tuning)) {
            self.rebuild = true;
        }
    }

    /// Starts the engine and runs it for `idle_s` more seconds without drawing a frame, so a lesson
    /// can begin on a warm, idling engine whose per-angle cycle tables are already full.
    pub fn pre_roll(&mut self, idle_s: f64) {
        self.auto_start = false;
        self.engine.warm_up();
        self.engine.set_throttle(0.0);
        self.engine.engage_starter();
        let (mut t, mut lit_at) = (0.0f64, None::<f64>);
        while t < 12.0 {
            let w_deg = (self.engine.rpm() * 6.0).max(1.0);
            let h = (1.2 / w_deg).clamp(1.0 / 20_000.0, 1.0 / 240.0);
            self.engine.update(h);
            self.record_samples();
            t += h;
            match lit_at {
                None if self.engine.rpm() > 550.0 && !self.engine.is_cranking_unlit() => {
                    self.engine.release_starter();
                    lit_at = Some(t);
                }
                Some(l) if t > l + idle_s => break,
                _ => {}
            }
        }
        self.tel = self.engine.telemetry();
        self.rpm_avg = self.tel.rpm as f32;
        self.was_unlit = false;
        self.flare_t = None;
    }

    /// Plugs everything back in and clears every injected fault.
    pub fn reset_faults(&mut self) {
        for l in 1..=self.cfg().cylinder_count as u32 {
            if let Some(ch) = self.engine.cylinder_ignition_mut(l) {
                *ch = Default::default();
            }
        }
        self.engine.set_catalyst_clog(0.0);
        self.engine.exhaust.o2.connected = true;
        self.engine.set_muffler_restriction(1.0);
        self.engine.evap = Default::default();
    }

    /// Turns the crank by hand so cylinder `ci` sits at `local_deg` (0..720) of its cycle, and pauses.
    pub fn scrub_to(&mut self, ci: usize, local_deg: f32) {
        let base = (self.alpha_v / 720.0).floor() * 720.0;
        self.alpha_v = base + (self.offsets[ci] + local_deg.rem_euclid(720.0)) as f64;
        self.alpha_v_prev = self.alpha_v;
        self.paused = true;
        self.scrub = true;
    }

    pub fn set_gear_index(&mut self, idx: usize) {
        let gears = &self.transmissions[self.trans_idx].gears;
        let idx = idx.min(gears.len() - 1);
        self.gear_idx = idx;
        let g = &gears[idx];
        self.engine.set_gear(g.label, g.ratio);
    }

    pub fn shift_up(&mut self) {
        let n = self.transmissions[self.trans_idx].gears.len();
        if self.gear_idx + 1 < n {
            self.set_gear_index(self.gear_idx + 1);
        }
    }

    pub fn shift_down(&mut self) {
        if self.gear_idx > 0 {
            self.set_gear_index(self.gear_idx - 1);
        }
    }

    /// Advance by one real frame.
    pub fn step(&mut self, real_dt: f32) {
        let dt_real = real_dt.clamp(0.0, 1.0 / 20.0) as f64;
        // Cold-start roar: a quick throttle blip as the engine catches, only while out of gear.
        let mut flare = 0.0;
        if let Some(t) = self.flare_t {
            let env = if t < FLARE_RISE_S { t / FLARE_RISE_S } else { (-(t - FLARE_RISE_S) / FLARE_DECAY_S).exp() };
            flare = FLARE_PEAK_THROTTLE * env;
            if !self.paused {
                self.flare_t = Some(t + dt_real as f32 * self.time_scale);
            }
            if t > FLARE_RISE_S + 5.0 * FLARE_DECAY_S {
                self.flare_t = None;
            }
        }
        if self.tel.total_gear_ratio.abs() > 1e-3 {
            flare = 0.0;
        }
        self.engine.set_throttle((self.throttle_input).max(flare) as f64);
        self.engine.set_brake(self.brake_input as f64);
        self.engine.set_dyno_load(self.dyno_load as f64);

        if self.starter_held {
            self.engine.engage_starter();
        } else {
            self.engine.release_starter();
        }

        // Auto-shift (drive-by-wire stand-in): only once actually in a forward gear and moving.
        self.shift_cooldown = (self.shift_cooldown - dt_real as f32).max(0.0);
        if self.auto_shift && self.gear_idx >= 2 && self.shift_cooldown <= 0.0 && !self.tel.is_shifting && self.tel.is_running {
            let redline = self.tel.redline_rpm as f32;
            let rpm = self.tel.rpm as f32;
            let top = self.transmissions[self.trans_idx].gears.len() - 1;
            if rpm > redline * 0.9 && self.gear_idx < top && self.throttle_input > 0.3 {
                self.shift_up();
                self.shift_cooldown = 0.7;
            } else if rpm < redline * 0.33 && self.gear_idx > 2 && self.tel.vehicle_speed_mps > 1.0 {
                self.shift_down();
                self.shift_cooldown = 0.7;
            }
        }

        let mut sim_dt = if self.paused { 0.0 } else { dt_real * self.time_scale as f64 };
        let mut micro_steps = 0;
        let (mut rpm_acc, mut t_acc) = (0.0f64, 0.0f64);
        while sim_dt > 1e-9 && micro_steps < 6000 {
            let w_deg = (self.engine.rpm() * 6.0).max(1.0);
            let h = (1.2 / w_deg).clamp(1.0 / 20_000.0, 1.0 / 240.0).min(sim_dt);
            self.engine.update(h);
            rpm_acc += self.engine.rpm() * h;
            t_acc += h;
            sim_dt -= h;
            micro_steps += 1;
            self.record_samples();
        }
        if t_acc > 0.0 {
            self.rpm_avg = (rpm_acc / t_acc) as f32;
        } else if !self.paused {
            self.rpm_avg = self.engine.rpm() as f32;
        }
        self.sim_time += dt_real * self.time_scale as f64;
        self.tel = self.engine.telemetry();
        self.update_pops(dt_real as f32);
        let unlit = self.engine.is_cranking_unlit();
        if self.was_unlit && !unlit && self.tel.is_running {
            self.flare_t = Some(0.0);
        }
        self.was_unlit = unlit;
        // the starter only engages in neutral, so a gear selected before it caught cancels the request
        if self.auto_start && ((self.tel.is_running && !unlit && self.tel.rpm > 600.0) || self.tel.total_gear_ratio.abs() > 1e-3) {
            self.auto_start = false;
        }
        if !self.paused {
            self.scrub = false;
        }

        // ----- visual playback -----
        let rpm = self.tel.rpm as f32;
        self.dilation = if self.visual_cap_rpm > 0.0 && rpm > self.visual_cap_rpm { self.visual_cap_rpm / rpm } else { 1.0 };
        let vis_dt = if self.paused { 0.0 } else { dt_real * self.time_scale as f64 * self.dilation as f64 };
        self.vis_dt = vis_dt as f32;
        self.alpha_v_prev = self.alpha_v;
        self.alpha_v += self.tel.rpm * 6.0 * vis_dt; // deg
        let wheel_w = self.tel.vehicle_speed_mps as f32 / engine_core::constants::WHEEL_RADIUS_M as f32;
        self.wheel_angle += wheel_w * vis_dt as f32;
        self.floor_offset += self.tel.vehicle_speed_mps as f32 * vis_dt as f32;
        let total = self.tel.total_gear_ratio as f32;
        let final_drive = self.transmissions[self.trans_idx].final_drive_ratio as f32;
        let out_w = wheel_w * final_drive;
        self.shaft_angle += out_w * vis_dt as f32;
        let crank_w = self.tel.rpm as f32 * std::f32::consts::TAU / 60.0;
        let in_w = if total.abs() < 1e-3 {
            crank_w
        } else {
            let e = self.tel.clutch_engagement as f32;
            crank_w * e + out_w * self.tel.gear_ratio as f32 * (1.0 - e)
        };
        self.input_angle += in_w * vis_dt as f32;
        let turbo_w = self.tel.turbo_spool_fraction as f32 * 2.0 * std::f32::consts::PI * 2000.0 + (rpm * 0.1).min(40.0);
        self.turbo_angle += turbo_w * vis_dt as f32;

        for v in [&mut self.backfire_flash, &mut self.misfire_flash, &mut self.spark_flash] {
            for t in v.iter_mut() {
                *t = (*t - dt_real as f32 * 6.0).max(0.0);
            }
        }

        // ----- audio snapshot -----
        let cfg = self.cfg().clone();
        if let Ok(mut a) = self.audio.lock() {
            // sound follows playback speed: slow-mo drops the pitch, pause silences it
            let speed = if self.paused { 0.0 } else { self.time_scale };
            a.rpm = self.rpm_avg * speed;
            a.throttle = self.tel.throttle as f32;
            // smoothed brake torque normalised per engine (instantaneous torque spikes every firing)
            let tq_ref = (75.0 * cfg.displacement_l()).max(60.0);
            a.load = (self.tel.display_torque_nm / tq_ref).clamp(0.0, 1.3) as f32;
            a.running = self.tel.is_running && !self.paused;
            a.cranking = self.tel.is_cranking;
            a.lit = !unlit;
            a.boost = (self.tel.boost_pressure_pa / 100_000.0) as f32;
            a.spool = self.tel.turbo_spool_fraction as f32;
            a.map_pa = self.tel.manifold_pressure_pa as f32;
            a.atm_pa = self.tel.atmospheric_pressure_pa as f32;
            a.air_k = self.tel.air_temperature_k as f32;
            a.charge_k = self.tel.charge_temp_k as f32;
            a.afr = self.tel.air_fuel_ratio as f32;
            a.egt_k = self.tel.exhaust_gas_temp_k as f32;
            a.knock = self.tel.knock_intensity as f32;
            a.vehicle_speed = self.tel.vehicle_speed_mps as f32;
            a.gear_whine = (self.tel.clutch_engagement as f32) * (self.tel.vehicle_speed_mps as f32 / 40.0).min(1.0);
            a.burn.clear();
            for l in 0..cfg.cylinder_count {
                // fully burned charge shows as 1 at the end of the power stroke; a misfire or an ignition cut as 0
                a.burn.push(self.tables[l].burn_at(525.0));
            }
            a.burble = burble_amount(&cfg.id) * if self.tuning.is_cat_deleted() { 1.4 } else { 1.0 };
            a.backfire = self.backfire_flash.iter().cloned().fold(0.0, f32::max);
            if a.engine_id != cfg.id {
                a.engine_id = cfg.id.clone();
            }
            if a.tuning != self.tuning {
                a.tuning = self.tuning.clone();
            }
            if a.banks != self.banks {
                a.banks = self.banks.clone();
            }
            a.master = if self.muted { 0.0 } else { self.master_volume };
            a.serial = a.serial.wrapping_add(1);
        }
    }

    /// Mirrors the synth's burble gate (lift-off or rev-matched downshift on an engine that suits it)
    /// so the tailpipe can spit gas and flame in step with the pops. With a live synth the real pop
    /// events are used instead.
    fn update_pops(&mut self, dt: f32) {
        self.pops_this_frame = 0;
        self.pop_strength = 0.0;
        if self.paused {
            return;
        }
        let id = self.cfg().id.clone();
        let burble = burble_amount(&id) * if self.tuning.is_cat_deleted() { 1.4 } else { 1.0 };
        let diesel = self.cfg().ignition_type == IgnitionType::Compression;
        let tel = &self.tel;
        // same lift-off envelope as the synth
        let thr = tel.throttle as f32;
        self.thr_slow_v += (thr - self.thr_slow_v) * (1.0 - (-dt / 0.4).exp());
        if thr < 0.08 {
            if self.thr_slow_v > 0.3 {
                self.lift_env_v = self.lift_env_v.max(self.thr_slow_v.min(1.0));
            }
            self.lift_env_v *= (-dt / 3.0).exp();
        } else {
            self.lift_env_v *= (-dt / 0.15).exp();
        }
        if self.audio_live {
            if let Ok(a) = self.audio.lock() {
                let n = a.pops_fired.saturating_sub(self.last_pops_fired);
                self.last_pops_fired = a.pops_fired;
                if n > 0 && n < 64 {
                    self.pops_this_frame = n as u32;
                    self.pop_strength = a.pop_gain.clamp(0.2, 1.3);
                }
            }
            return;
        }
        if !tel.is_running || diesel {
            return;
        }
        let gate = burble * self.lift_env_v * ((tel.rpm as f32 - 1700.0) / 3000.0).clamp(0.0, 1.0);
        // ~20 pops a second at full gate, like the synth
        let p = (20.0 * gate * dt).min(0.8);
        let r = |s: &mut u32| {
            *s ^= *s << 13;
            *s ^= *s >> 17;
            *s ^= *s << 5;
            (*s >> 8) as f32 / 16_777_216.0
        };
        if p > 0.0 && r(&mut self.pop_rng) < p {
            self.pops_this_frame = 1;
            let g = r(&mut self.pop_rng);
            self.pop_strength = ((0.35 + 0.9 * g * g) * gate.sqrt() * 0.9).clamp(0.2, 1.3);
        }
    }

    fn record_samples(&mut self) {
        let n = self.engine.cylinder_count();
        for i in 0..n {
            let s = self.engine.cylinder_sample(i);
            let l = (s.label - 1) as usize;
            let bin = (s.local_angle_deg as usize) % 720;
            let t = &mut self.tables[l];
            if bin != self.last_bin[l] {
                // A micro-step covers about 1.2 degrees, so a bin is skipped now and then and would keep a stale
                // (or, on a fresh table, default) value: fill what was jumped over by interpolation.
                let prev = self.last_bin[l];
                let gap = (bin + 720 - prev) % 720;
                if gap > 1 && gap <= 8 {
                    let (p0, b0) = (t.pressure[prev], t.burn[prev]);
                    let (p1, b1) = (s.pressure_pa as f32, s.burn_fraction as f32);
                    for k in 1..gap {
                        let (f, at) = (k as f32 / gap as f32, (prev + k) % 720);
                        t.pressure[at] = p0 + (p1 - p0) * f;
                        t.burn[at] = b0 + (b1 - b0) * f;
                        t.flags[at] = 0;
                    }
                }
                t.flags[bin] = 0;
                self.last_bin[l] = bin;
            }
            t.pressure[bin] = s.pressure_pa as f32;
            t.burn[bin] = s.burn_fraction as f32;
            if s.ignited {
                t.flags[bin] |= 1;
                self.spark_flash[l] = 1.0;
            }
            if s.misfired {
                t.flags[bin] |= 2;
                self.misfire_flash[l] = 1.0;
            }
            if s.backfired {
                t.flags[bin] |= 4;
                self.backfire_flash[l] = 1.0;
            }
        }
    }
}
