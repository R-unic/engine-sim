//! What burns, sprays and smokes outside the combustion chambers.
//!
//! The tailpipe plume - exhaust haze, steam, soot and the flame of a backfire - is a participating medium, simulated
//! as blobs and ray-marched (`volume.rs`); this file decides what comes out of the pipe and when (per exhaust-valve
//! opening, per pop, per backfire) and feeds it to that medium. The two things that are *particles* are handled
//! here: embers and burning droplets flung out of a backfire (streaked by their own speed, like a shutter would),
//! and the fuel spray of the injectors.
//!
//! Everything is aged in *fx time* (real time x the sim's time scale), so slowing the sim down stretches every
//! flame and puff out to be inspected.

use crate::fx::{hash01, FlowPaths};
use crate::rig::anim::{local_angle, valve_open_amount, RigGeom};
use crate::rig::engine_build::bank_to_engine;
use crate::rig::Ctx;
use crate::scene::MainCamera;
use crate::sim::Sim;
use crate::volume::{Blob, Plume};
use bevy::asset::RenderAssetUsages;
use bevy::light::PointLight;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use engine_core::{IgnitionType, Muffler};
use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ember = 0,
    Mist = 1,
}

const POOL: [usize; 2] = [110, 160];

#[derive(Resource, Clone)]
pub struct PuffTex {
    pub soft: Handle<Image>,
}

#[derive(Component)]
pub struct Puff {
    kind: Kind,
    mat: Handle<StandardMaterial>,
    alive: bool,
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    size0: f32,
    size1: f32,
    hot: f32,
    tint: Vec3,
    grav: f32,
    drag: f32,
}

#[derive(Resource, Default)]
pub struct PuffPool {
    rings: [Vec<Entity>; 2],
    next: [usize; 2],
    pub tips: Vec<Vec3>,
    pub axis: Vec3,
    pub radius: f32,
}

impl PuffPool {
    fn tip(&self, i: usize) -> Vec3 {
        self.tips.get(i % self.tips.len().max(1)).copied().unwrap_or(Vec3::ZERO)
    }
}

#[derive(Component)]
pub struct TailLight;

fn soft_tex(images: &mut Assets<Image>) -> Handle<Image> {
    let n = 64u32;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            // a hot core and a Gaussian skirt: what a point of light looks like through a lens
            let a = ((-5.0 * r * r).exp() * 0.75 + (1.0 - r).max(0.0).powi(3) * 0.25).clamp(0.0, 1.0);
            data.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    images.add(Image::new(
        Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

pub fn setup_puff_tex(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(PuffTex { soft: soft_tex(&mut images) });
}

/// Spawns the particle pool under the rig root (called from `fx::build_fx`).
pub fn build_pool(cx: &mut Ctx, root: Entity, tex: &PuffTex, tips: &[Vec3], axis: Vec3, radius: f32) {
    let quad = cx.meshes.add(Rectangle::new(1.0, 1.0));
    let mut pool = PuffPool { tips: tips.to_vec(), axis, radius, ..default() };
    for (k, kind) in [Kind::Ember, Kind::Mist].into_iter().enumerate() {
        for _ in 0..POOL[k] {
            let mat = cx.material(StandardMaterial {
                base_color: Color::NONE,
                base_color_texture: Some(tex.soft.clone()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                cull_mode: None,
                double_sided: true,
                fog_enabled: false,
                ..default()
            });
            let e = cx
                .cmd
                .spawn((
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_scale(Vec3::ZERO),
                    Visibility::Hidden,
                    Puff { kind, mat, alive: false, pos: Vec3::ZERO, vel: Vec3::ZERO, age: 0.0, life: 1.0, size0: 0.0, size1: 0.0, hot: 1.0, tint: Vec3::ONE, grav: 0.0, drag: 0.0 },
                    ChildOf(root),
                ))
                .id();
            pool.rings[k].push(e);
        }
    }
    cx.cmd.insert_resource(pool);
}

// ---------------------------------------------------------------------------------- emission

struct Rng(u32);

impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 16_777_216.0
    }
    fn r(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    fn unit(&mut self) -> Vec3 {
        let z = self.r(-1.0, 1.0);
        let a = self.r(0.0, TAU);
        let s = (1.0 - z * z).sqrt();
        Vec3::new(s * a.cos(), z, s * a.sin())
    }
    /// `axis` jittered by up to `spread` (radians-ish), unit length.
    fn cone(&mut self, axis: Vec3, spread: f32) -> Vec3 {
        (axis + self.unit() * spread).normalize_or_zero()
    }
}

/// Optical mass of a Gaussian blob of radius `r` whose centre extinction is `sigma` (1/m).
fn haze_mass(sigma: f32, r: f32) -> f32 {
    sigma * 15.75 * (0.5 * r).powi(3)
}

/// Emission mass of a Gaussian blob of radius `r` whose centre emission is `e` (per metre).
fn heat_mass(e: f32, r: f32) -> f32 {
    e * 15.75 * (0.5 * r).powi(3)
}

/// A flame leaving the pipe: a fireball, flame tongues streaming out and the smoke it leaves behind, plus embers.
/// `k` scales it (1 = a full backfire, ~0.4 = a small crackle). The charge lights at the mouth and the flame front
/// runs out along the plume (each blob lights `along / FRONT` seconds after the first), so the fire is seen to leave
/// the pipe rather than to appear.
fn backfire(plume: &mut Plume, embers: &mut Vec<Ember>, tip: Vec3, axis: Vec3, r_pipe: f32, k: f32, small: bool) {
    const FRONT: f32 = 55.0;
    let sc = if small { 0.5 } else { 1.0 };
    let kk = k.max(0.5);
    // the fireball: the unburnt charge that reached the open air, lighting all at once
    let mut b = Blob::new(tip + axis * 0.07, axis * 9.0, (0.058 + 0.04 * k) * sc);
    b.r_max = 0.3;
    b.heat = heat_mass(150.0 * kk, b.r);
    b.t_flame = 2250.0;
    b.temp = 1500.0;
    b.lift = 0.10;
    b.tau_burn = 0.12;
    b.tau_flame_t = 0.4;
    b.life = 0.9;
    b.soot = haze_mass(4.0, b.r);
    b.tau_haze = 3.0;
    plume.spawn(b);
    // tongues of flame thrown out along the pipe's axis, hottest and fastest at the front
    let n = if small { 7 } else { 26 };
    for _ in 0..n {
        let along = plume.range(0.0, 0.5 * sc);
        let speed = plume.range(12.0, 40.0) * (0.55 + 0.45 * k) * (0.7 + 0.3 * sc);
        let d = plume.unit();
        let dir = (axis + d * 0.22).normalize_or_zero();
        let mut b = Blob::new(tip + axis * along + d * 0.01, dir * speed, plume.range(0.028, 0.055) * sc);
        b.r_max = 0.2;
        b.heat = heat_mass(plume.range(100.0, 190.0) * kk, b.r);
        b.t_flame = plume.range(1900.0, 2350.0);
        b.temp = 1300.0;
        b.lift = 0.10;
        b.tau_burn = plume.range(0.07, 0.14);
        b.tau_flame_t = plume.range(0.25, 0.5);
        b.ignite = along / FRONT + plume.range(0.0, 0.012);
        b.life = 0.8;
        b.soot = haze_mass(plume.range(1.0, 4.0), b.r);
        b.tau_haze = 2.5;
        plume.spawn(b);
    }
    // smoke the burn leaves, thrown out with it
    if !small {
        for _ in 0..8 {
            let speed = plume.range(2.0, 9.0);
            let d = plume.unit();
            let mut b = Blob::new(tip + axis * plume.range(0.0, 0.25), (axis + d * 0.5).normalize_or_zero() * speed, plume.range(0.05, 0.09));
            b.r_max = 0.26;
            b.soot = haze_mass(plume.range(5.0, 11.0), b.r);
            b.haze = haze_mass(plume.range(2.0, 5.0), b.r);
            b.temp = 650.0;
            b.tau_haze = 3.2;
            b.life = 2.6;
            plume.spawn(b);
        }
    }
    // burning droplets and sparks
    let count = if small { 3 } else { 10 };
    for _ in 0..count {
        let d = plume.unit();
        let v = (axis + d * 0.6).normalize_or_zero() * plume.range(4.0, 18.0);
        embers.push(Ember { pos: tip + axis * 0.04, vel: v, life: plume.range(0.3, 0.9), hot: plume.range(9.0, 15.0) });
    }
    let _ = r_pipe;
}

struct Ember {
    pos: Vec3,
    vel: Vec3,
    life: f32,
    hot: f32,
}

/// One burble / overrun pop at the tailpipe: a jet of exhaust gas shoved out of the pipe (always),
/// lit by a flame when the pop is strong enough to ignite it. `k` is the pop's loudness, ~0.2..1.3.
fn pop_burst(plume: &mut Plume, rng: &mut Rng, embers: &mut Vec<Ember>, tip: Vec3, axis: Vec3, r_pipe: f32, k: f32) {
    let kk = k.clamp(0.2, 1.3);
    // the gas itself: a quick grey-blue jet that billows out and thins away
    for _ in 0..(5 + (6.0 * kk) as usize) {
        let speed = plume.range(2.5, 9.5) * (0.5 + 0.6 * kk);
        let dir = rng.cone(axis, 0.3);
        let mut b = Blob::new(tip + axis * plume.range(0.0, 0.1), dir * speed, plume.range(0.04, 0.07));
        b.r_max = 0.24;
        b.haze = haze_mass(plume.range(3.5, 7.0) * (0.6 + 0.6 * kk), b.r);
        b.soot = haze_mass(plume.range(0.0, 2.5), b.r);
        b.temp = 560.0;
        b.tau_haze = 1.9;
        b.life = 2.4;
        plume.spawn(b);
    }
    // flame: strong pops light the gas, weak ones are dry
    if kk > 0.55 || rng.f() < 0.4 {
        let sc = 0.45 + 0.5 * kk;
        backfire(plume, embers, tip, axis, r_pipe, 0.55 * sc, true);
    }
}

#[derive(Default)]
pub struct PuffState {
    prev_backfire: Vec<f32>,
    exhaust_acc: f32,
    crackle: f32,
    last_tick: i64,
    rng: u32,
    /// `FX_TEST=backfire` (or `pop`): fires one every 1.3 s of fx time (the first at `FX_TEST_AT` seconds, default 1.3),
    /// for looking at the flame in screenshots.
    test: Option<(bool, f32)>,
    test_read: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn puff_system(
    sim: Res<Sim>,
    time: Res<Time>,
    g: Option<Res<RigGeom>>,
    paths: Option<Res<FlowPaths>>,
    mut pool: ResMut<PuffPool>,
    plume: Option<ResMut<Plume>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut lights: Query<(&mut PointLight, &mut Transform), (With<TailLight>, Without<Puff>)>,
    mut q: Query<(&mut Puff, &mut Transform, &mut Visibility)>,
    mut st: Local<PuffState>,
) {
    if pool.rings[0].is_empty() || std::env::var_os("NO_PUFF").is_some() {
        return;
    }
    if st.rng == 0 {
        st.rng = 0x9E37_79B9;
    }
    let mut rng = Rng(st.rng);
    let real_dt = time.delta_secs();
    let dt = if sim.paused { 0.0 } else { real_dt * sim.time_scale * crate::volume::fx_slow() };
    let tt = time.elapsed_secs();
    let tel = &sim.tel;
    let axis = pool.axis;
    let r_pipe = pool.radius;
    let n = sim.backfire_flash.len();
    let mut embers: Vec<Ember> = Vec::new();
    let mut mist: Vec<(Vec3, Vec3, f32, f32, f32, Vec3)> = Vec::new();
    let mut plume = plume;

    // --- backfires: one burst per fresh event
    st.prev_backfire.resize(n, 0.0);
    let mut fresh = 0;
    for l in 0..n {
        let v = sim.backfire_flash[l];
        if v > st.prev_backfire[l] + 0.05 {
            fresh += 1;
        }
        st.prev_backfire[l] = v;
    }
    if let (Some(plume), true) = (plume.as_deref_mut(), fresh > 0) {
        for i in 0..fresh.min(3) {
            let tip = pool.tip(i);
            backfire(plume, &mut embers, tip, axis, r_pipe, 1.0, false);
        }
    }
    let backfire_light = sim.backfire_flash.iter().cloned().fold(0.0f32, f32::max);

    // --- developer hook: a backfire (or a pop) on a timer
    if !st.test_read {
        st.test_read = true;
        let at: f32 = std::env::var("FX_TEST_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(1.3);
        st.test = match std::env::var("FX_TEST").as_deref() {
            Ok("backfire") => Some((true, at - 1.3)),
            Ok("pop") => Some((false, at - 1.3)),
            _ => None,
        };
    }
    if let (Some((big, last)), Some(plume)) = (st.test, plume.as_deref_mut()) {
        let now = plume.time;
        if now - last > 1.3 {
            st.test = Some((big, now));
            eprintln!("FX_TEST fired at wall {tt:.2} s, fx {now:.2} s");
            if big {
                backfire(plume, &mut embers, pool.tip(0), axis, r_pipe, 1.0, false);
            } else {
                pop_burst(plume, &mut rng, &mut embers, pool.tip(0), axis, r_pipe, 0.8);
            }
            st.crackle = 0.8;
        }
    }

    // --- decel / limiter crackle: small pops
    let diesel_engine = sim.cfg().ignition_type == IgnitionType::Compression;
    let crackle_on = !diesel_engine
        && (tel.is_rev_limiting
            || (tel.throttle < 0.02 && tel.rpm > 3800.0 && tel.is_running && tel.air_fuel_ratio < 30.0 && tel.exhaust_gas_temp_k > 800.0));
    let tick = (tt * 40.0).floor() as i64;
    if crackle_on && tick != st.last_tick && hash01(tick as f32) > 0.55 {
        st.crackle = 0.45;
        if let Some(plume) = plume.as_deref_mut() {
            backfire(plume, &mut embers, pool.tip(tick as usize), axis, r_pipe, 0.4, true);
        }
    }
    st.last_tick = tick;
    // burble / overrun pops reported by the synth (or the sim's own gate when there is no sound)
    for i in 0..sim.pops_this_frame.min(3) {
        let louder = if sim.tuning.is_cat_deleted() || sim.tuning.muffler == Muffler::Delete { 1.3 } else { 1.0 };
        let k = sim.pop_strength * louder;
        if let Some(plume) = plume.as_deref_mut() {
            pop_burst(plume, &mut rng, &mut embers, pool.tip(i as usize), axis, r_pipe, k);
        }
        st.crackle = st.crackle.max(0.25 + 0.35 * k.min(1.2));
    }
    st.crackle = (st.crackle - real_dt * 6.0).max(0.0);
    let intensity = backfire_light.max(st.crackle);
    let flame_glow = plume.as_deref().map_or(0.0, |p| (p.flame_power * 0.35).min(1.2));
    for (mut l, mut tf) in &mut lights {
        l.intensity = 900_000.0 * intensity.max(flame_glow);
        tf.translation = plume.as_deref().map_or(pool.tip(0) + axis * 0.3, |p| p.flame_centre + axis * 0.1);
    }

    // --- the exhaust leaving the pipe: haze, steam and soot, a puff at a time
    if let (Some(plume), true) = (plume.as_deref_mut(), tel.is_running && dt > 0.0) {
        let diesel = sim.cfg().ignition_type == IgnitionType::Compression;
        let thr = tel.throttle as f32;
        let flow = (tel.rpm as f32 / 6500.0).clamp(0.0, 1.2) * (0.15 + 0.85 * thr);
        let egt = tel.exhaust_gas_temp_k as f32;
        let afr = tel.air_fuel_ratio as f32;
        let cold = 1.0 - ((egt - 430.0) / 220.0).clamp(0.0, 1.0);
        let rich = if afr > 1.0 { ((12.6 - afr) / 3.0).clamp(0.0, 1.0) } else { 0.0 };
        let soot = rich.max(if diesel { ((thr - 0.5) * 1.4).clamp(0.0, 0.8) } else { 0.0 });
        // what shows: a trace of haze from clean warm exhaust, steam when the pipe is cold, smoke when it is rich
        let steam = 0.10 + 0.9 * cold * cold + 0.35 * cold;
        let v_exit = 3.0 + 22.0 * flow;
        let r0 = (0.85 * r_pipe).max(0.018);
        // puffs per second: enough that they overlap into a continuous jet, however fast it flows
        let rate = (v_exit / (0.8 * r0)).clamp(110.0, 520.0);
        st.exhaust_acc += rate * dt;
        let count = (st.exhaust_acc as usize).min(60);
        st.exhaust_acc -= count as f32;
        let t_gas = (egt * 0.55).clamp(340.0, 760.0);
        // mass flux of haze: so that the jet leaves the pipe with an extinction of `sigma_exit`
        let sigma_exit = 14.0 * steam;
        let sigma_soot = 24.0 * soot;
        let per = v_exit / rate;
        let tips = pool.tips.len().max(1);
        for i in 0..count {
            let tip = pool.tip(i);
            let sp = v_exit * plume.range(0.75, 1.3);
            let dir = rng.cone(axis, 0.07);
            // each puff leaves a fraction of a frame ago: spread them along the path they would have taken
            let ahead = plume.range(0.0, 1.0) * sp * dt;
            let mut b = Blob::new(tip + axis * (0.02 + ahead) + rng.unit() * 0.004, dir * sp, r0);
            b.r_max = 0.2;
            // the pipe's flux of haze, shared among the puffs: extinction x the mouth's area x the path a puff stands for
            let slab = std::f32::consts::PI * r0 * r0 * per * tips as f32;
            b.haze = sigma_exit * slab;
            b.soot = sigma_soot * slab;
            b.temp = t_gas;
            b.tau_cool = 0.45;
            b.tau_haze = 1.0 + 1.6 * cold + 1.4 * soot;
            b.life = 2.4;
            b.lift = 0.3;
            plume.spawn(b);
        }
        // the pulses of the blow-down, one stronger puff per valve opening (at most a few a frame)
        if let (Some(g), Some(paths)) = (g.as_ref(), paths.as_ref()) {
            let lay = &g.lay;
            if sim.tables.len() == lay.n() && paths.exhaust.len() == lay.n() {
                let mut pulses = 0;
                for ci in 0..lay.n() {
                    let th = local_angle(&sim, ci);
                    let from = (sim.alpha_v_prev as f32 - sim.offsets[ci]).rem_euclid(720.0);
                    let span = (th - from).rem_euclid(720.0);
                    let evo = g.exh_peak_deg - g.exh_dur_deg * 0.5;
                    if span > 0.0 && span < 360.0 && (evo - from).rem_euclid(720.0) < span && pulses < 6 {
                        pulses += 1;
                        let k = ((sim.tables[ci].pressure_at(490.0) - 1.2e5) / 5.0e5).clamp(0.1, 1.6);
                        let tip = pool.tip(ci);
                        let mut b = Blob::new(tip + axis * 0.03, rng.cone(axis, 0.1) * (v_exit * (1.2 + 0.9 * k)), r0 * 1.25);
                        b.r_max = 0.2;
                        b.haze = haze_mass(sigma_exit * 1.6, r0 * 1.25);
                        b.soot = haze_mass(sigma_soot * 1.4, r0 * 1.25);
                        b.temp = t_gas * 1.15;
                        b.tau_haze = 1.0 + 1.6 * cold + 1.4 * soot;
                        b.life = 2.2;
                        plume.spawn(b);
                    }
                }
            }
        }
    }

    // --- per-cylinder: fuel spray
    if let Some(g) = g.as_ref() {
        let lay = &g.lay;
        if tel.is_running && sim.tables.len() == lay.n() {
            let diesel = sim.cfg().ignition_type == IgnitionType::Compression;
            let b = lay.bore;
            let seat_y = lay.deck + 0.001;
            for ci in 0..lay.n() {
                let cg = &lay.cyls[ci];
                let tilt = Quat::from_rotation_x(lay.tilts[cg.bank]);
                let th = local_angle(&sim, ci);
                // port-injected spray toward the open intake valves (early intake stroke)
                if !diesel {
                    let dd = (th - g.int_peak_deg + 360.0).rem_euclid(720.0) - 360.0;
                    if valve_open_amount(g, th, true) > 0.12 && dd < 25.0 && dd > -g.int_dur_deg * 0.5 {
                        let side = -cg.sigma;
                        let from = Vec3::new(cg.x, lay.deck + 0.075, side * (b * 0.5 + 0.045));
                        for _ in 0..3 {
                            let tgt = Vec3::new(cg.x + rng.r(-0.2, 0.2) * b, seat_y, side * 0.19 * b);
                            let dir = (tgt - from).normalize_or_zero();
                            let v = tilt * (rng.cone(dir, 0.12) * rng.r(0.9, 1.6));
                            mist.push((bank_to_engine(lay.tilts[cg.bank], from), v, rng.r(0.07, 0.13), rng.r(3.0, 6.0), 0.0, Vec3::new(0.55, 0.85, 1.0)));
                        }
                    }
                } else if (345.0..380.0).contains(&th) {
                    // diesel: direct injection plume into the chamber
                    for _ in 0..5 {
                        let from = Vec3::new(cg.x, lay.deck - 0.004, 0.0);
                        let v = tilt * (rng.cone(Vec3::NEG_Y, 0.55) * rng.r(1.2, 2.4));
                        mist.push((bank_to_engine(lay.tilts[cg.bank], from), v, rng.r(0.06, 0.11), 5.0, 0.0, Vec3::new(1.0, 0.9, 0.55)));
                    }
                }
            }
        }
    }

    // --- allocate pool slots
    for e in &embers {
        spawn_puff(&mut pool, &mut q, &mut rng, Kind::Ember, e.pos, e.vel, e.life, 0.016, 0.005, e.hot, Vec3::ONE, 9.8, 0.7);
    }
    for (pos, vel, life, hot, _, tint) in &mist {
        spawn_puff(&mut pool, &mut q, &mut rng, Kind::Mist, *pos, *vel, *life, 0.004, 0.012, *hot, *tint, 0.0, 0.0);
    }
    st.rng = rng.0;

    // --- integrate + shade
    let cam_rot = cam.iter().next().map(|c| c.compute_transform().rotation).unwrap_or(Quat::IDENTITY);
    let (right, up) = (cam_rot * Vec3::X, cam_rot * Vec3::Y);
    for (mut p, mut tf, mut vis) in &mut q {
        if !p.alive {
            continue;
        }
        p.age += dt;
        if p.age >= p.life {
            p.alive = false;
            *vis = Visibility::Hidden;
            continue;
        }
        let drag = (-p.drag * dt).exp();
        p.vel *= drag;
        p.vel.y -= p.grav * dt;
        let step = p.vel * dt;
        p.pos += step;
        let t = p.age / p.life;
        let size = p.size0 + (p.size1 - p.size0) * (1.0 - (1.0 - t) * (1.0 - t));
        let color = match p.kind {
            Kind::Ember => {
                // white-yellow when new, glowing orange, then a cooling red
                let c = Vec3::new(1.0, 0.62 - 0.38 * t, 0.16 - 0.1 * t) * p.hot * (1.0 - t * t);
                LinearRgba::new(c.x, c.y, c.z, 1.0)
            }
            Kind::Mist => {
                let c = p.tint * p.hot;
                LinearRgba::new(c.x, c.y, c.z, (1.0 - t).powf(0.7))
            }
        };
        tf.translation = p.pos;
        // a moving point of light is a streak: as long as it travels while the shutter is open
        let vs = Vec2::new(p.vel.dot(right), p.vel.dot(up));
        let streak = if p.kind == Kind::Ember { vs.length() / 45.0 } else { 0.0 };
        let ang = vs.y.atan2(vs.x);
        tf.rotation = cam_rot * Quat::from_rotation_z(if streak > 1e-4 { ang } else { 0.0 });
        tf.scale = Vec3::new((size * 2.0 + streak).max(1e-4), (size * 2.0).max(1e-4), 1.0);
        if let Some(mut m) = materials.get_mut(&p.mat) {
            m.base_color = Color::LinearRgba(color);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_puff(
    pool: &mut PuffPool,
    q: &mut Query<(&mut Puff, &mut Transform, &mut Visibility)>,
    _rng: &mut Rng,
    kind: Kind,
    pos: Vec3,
    vel: Vec3,
    life: f32,
    size0: f32,
    size1: f32,
    hot: f32,
    tint: Vec3,
    grav: f32,
    drag: f32,
) {
    let k = kind as usize;
    let ring = &pool.rings[k];
    if ring.is_empty() {
        return;
    }
    let ent = ring[pool.next[k] % ring.len()];
    pool.next[k] = (pool.next[k] + 1) % ring.len();
    if let Ok((mut p, mut tf, mut vis)) = q.get_mut(ent) {
        p.alive = true;
        p.pos = pos;
        p.vel = vel;
        p.age = 0.0;
        p.life = life.max(0.01);
        p.size0 = size0;
        p.size1 = size1;
        p.hot = hot;
        p.tint = tint;
        p.grav = grav;
        p.drag = drag;
        tf.translation = pos;
        *vis = Visibility::Inherited;
    }
}
