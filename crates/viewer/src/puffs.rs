//! Pooled billboard particles for everything that burns, sprays or smokes: tailpipe backfire
//! fireballs + embers + soot, manifold flash, exhaust-valve blowdown, fuel-injector spray and the
//! continuous exhaust plume (white vapour when cold, black soot when rich).
//!
//! Particles live in rig-root space and are aged in *fx time* (real time x the sim's time scale), so
//! slowing the sim down stretches every flame and smoke puff out to be inspected.

use crate::fx::{hash01, FlowPaths, PolyPath};
use crate::rig::anim::{local_angle, valve_open_amount, RigGeom};
use crate::rig::engine_build::bank_to_engine;
use crate::rig::Ctx;
use crate::scene::MainCamera;
use crate::sim::Sim;
use bevy::asset::RenderAssetUsages;
use bevy::light::PointLight;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use engine_core::{IgnitionType, Muffler};
use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fire = 0,
    Smoke = 1,
    Ember = 2,
    Mist = 3,
}

const POOL: [usize; 4] = [170, 80, 70, 160];

#[derive(Resource, Clone)]
pub struct PuffTex {
    pub soft: Handle<Image>,
    pub lumpy: Handle<Image>,
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
    buoy: f32,
    drag: f32,
    roll: f32,
    spin: f32,
    alpha: f32,
}

#[derive(Resource, Default)]
pub struct PuffPool {
    rings: [Vec<Entity>; 4],
    next: [usize; 4],
    tail: Vec3,
}

#[derive(Component)]
pub struct TailLight;

fn gradient_tex(images: &mut Assets<Image>, lumpy: bool) -> Handle<Image> {
    let n = 96u32;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let mut edge = 1.0;
            if lumpy {
                // cheap fbm-ish lumps so smoke has a ragged outline instead of a perfect disc
                let ang = v.atan2(u);
                // low-order wobble only: higher harmonics turned the outline into a spiky star
                let w = 0.6 * (2.0 * ang + 1.3).sin() + 0.3 * (3.0 * ang + 0.4).sin() + 0.1 * (5.0 * ang + 2.1).sin();
                edge = 0.86 + 0.09 * w;
                let blot = 0.5 + 0.5 * (4.1 * u + 1.7).sin() * (3.3 * v + 0.6).sin();
                edge *= 0.92 + 0.16 * blot;
            }
            let a = (1.0 - r / edge).clamp(0.0, 1.0);
            let a = a * a * (3.0 - 2.0 * a);
            let a = if lumpy { a.powf(1.15) } else { a.powf(1.3) };
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
    commands.insert_resource(PuffTex { soft: gradient_tex(&mut images, false), lumpy: gradient_tex(&mut images, true) });
}

/// Spawns the particle pool under the rig root (called from `fx::build_fx`).
pub fn build_pool(cx: &mut Ctx, root: Entity, tex: &PuffTex, tail: Vec3) {
    let quad = cx.meshes.add(Rectangle::new(1.0, 1.0));
    let mut pool = PuffPool { tail, ..default() };
    for (k, kind) in [Kind::Fire, Kind::Smoke, Kind::Ember, Kind::Mist].into_iter().enumerate() {
        for _ in 0..POOL[k] {
            let mat = cx.material(match kind {
                Kind::Smoke => StandardMaterial {
                    base_color: Color::NONE,
                    base_color_texture: Some(tex.lumpy.clone()),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    reflectance: 0.0,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                },
                _ => StandardMaterial {
                    base_color: Color::NONE,
                    base_color_texture: Some(tex.soft.clone()),
                    alpha_mode: AlphaMode::Add,
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                },
            });
            let e = cx
                .cmd
                .spawn((
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_scale(Vec3::ZERO),
                    Visibility::Hidden,
                    Puff {
                        kind,
                        mat,
                        alive: false,
                        pos: Vec3::ZERO,
                        vel: Vec3::ZERO,
                        age: 0.0,
                        life: 1.0,
                        size0: 0.0,
                        size1: 0.0,
                        hot: 1.0,
                        tint: Vec3::ONE,
                        grav: 0.0,
                        buoy: 0.0,
                        drag: 0.0,
                        roll: 0.0,
                        spin: 0.0,
                        alpha: 1.0,
                    },
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

struct Emit {
    kind: Kind,
    pos: Vec3,
    vel: Vec3,
    life: f32,
    size0: f32,
    size1: f32,
    hot: f32,
    tint: Vec3,
    grav: f32,
    buoy: f32,
    drag: f32,
    alpha: f32,
}

impl Emit {
    fn new(kind: Kind, pos: Vec3, vel: Vec3, life: f32) -> Self {
        Self { kind, pos, vel, life, size0: 0.02, size1: 0.05, hot: 1.0, tint: Vec3::ONE, grav: 0.0, buoy: 0.0, drag: 0.0, alpha: 1.0 }
    }
    fn size(mut self, a: f32, b: f32) -> Self {
        self.size0 = a;
        self.size1 = b;
        self
    }
    fn hot(mut self, h: f32) -> Self {
        self.hot = h;
        self
    }
    fn tint(mut self, t: Vec3) -> Self {
        self.tint = t;
        self
    }
    fn phys(mut self, grav: f32, buoy: f32, drag: f32) -> Self {
        self.grav = grav;
        self.buoy = buoy;
        self.drag = drag;
        self
    }
    fn alpha(mut self, a: f32) -> Self {
        self.alpha = a;
        self
    }
}

fn tail_axis() -> Vec3 {
    Vec3::new(1.0, 0.1, 0.1).normalize()
}

fn tail_burst(out: &mut Vec<Emit>, rng: &mut Rng, tip: Vec3, k: f32, small: bool) {
    let axis = tail_axis();
    let sc = if small { 0.45 } else { 1.0 };
    // core fireball
    out.push(
        Emit::new(Kind::Fire, tip + axis * 0.04, axis * 2.5, 0.2)
            .size(0.05 * sc, (0.2 + 0.22 * k) * sc)
            .hot(10.0)
            .phys(0.0, 0.0, 2.5),
    );
    // streaming flame tongues
    let n = if small { 6 } else { 18 };
    for _ in 0..n {
        let sp = rng.r(4.0, 15.0) * (0.55 + 0.45 * k) * (0.6 + 0.4 * sc);
        let v = rng.cone(axis, 0.24) * sp;
        out.push(
            Emit::new(Kind::Fire, tip + axis * rng.r(0.0, 0.12), v, rng.r(0.08, 0.26) * (0.6 + 0.4 * sc))
                .size(rng.r(0.02, 0.04) * sc, rng.r(0.08, 0.17) * sc)
                .hot(rng.r(5.0, 11.0))
                .phys(0.0, 0.5, 3.2),
        );
    }
    // burning fuel droplets flung out
    for _ in 0..(if small { 2 } else { 7 }) {
        let v = rng.cone(axis, 0.7) * rng.r(4.0, 17.0);
        out.push(Emit::new(Kind::Ember, tip + axis * 0.03, v, rng.r(0.3, 0.8)).size(0.014, 0.004).hot(14.0).phys(9.8, 0.0, 0.7));
    }
    // sooty smoke left behind
    if !small {
        for _ in 0..6 {
            let v = rng.cone(axis, 0.5) * rng.r(1.5, 5.0);
            out.push(
                Emit::new(Kind::Smoke, tip + axis * rng.r(0.0, 0.2), v, rng.r(1.0, 1.8))
                    .size(0.05, rng.r(0.22, 0.34))
                    .tint(Vec3::splat(0.045))
                    .alpha(0.5)
                    .phys(0.0, 0.35, 1.8),
            );
        }
    }
}

/// One burble / overrun pop at the tailpipe: a jet of exhaust gas shoved out of the pipe (always),
/// lit by a flame when the pop is strong enough to ignite it. `k` is the pop's loudness, ~0.2..1.3.
fn pop_burst(out: &mut Vec<Emit>, rng: &mut Rng, tip: Vec3, k: f32) {
    let axis = tail_axis();
    let kk = k.clamp(0.2, 1.3);
    // the gas itself: a quick grey-blue jet that billows out and thins away
    for _ in 0..(9 + (9.0 * kk) as usize) {
        let v = rng.cone(axis, 0.3) * rng.r(2.5, 9.5) * (0.5 + 0.6 * kk);
        out.push(
            Emit::new(Kind::Smoke, tip + axis * rng.r(0.0, 0.12), v, rng.r(0.7, 1.5))
                .size(0.04, rng.r(0.2, 0.36) * (0.6 + 0.6 * kk))
                .tint(Vec3::new(0.7, 0.74, 0.8))
                .alpha(0.6)
                .phys(0.0, 0.3, 2.6),
        );
    }
    // flame: strong pops light the gas, weak ones are dry
    if kk > 0.55 || rng.f() < 0.4 {
        let sc = 0.45 + 0.5 * kk;
        out.push(
            Emit::new(Kind::Fire, tip + axis * 0.03, axis * 2.0, 0.14)
                .size(0.03 * sc, 0.13 * sc)
                .hot(7.0)
                .phys(0.0, 0.0, 3.0),
        );
        for _ in 0..(3 + (4.0 * kk) as usize) {
            let v = rng.cone(axis, 0.22) * rng.r(4.0, 12.0) * (0.5 + 0.5 * kk);
            out.push(
                Emit::new(Kind::Fire, tip + axis * rng.r(0.0, 0.1), v, rng.r(0.06, 0.18))
                    .size(rng.r(0.015, 0.03) * sc, rng.r(0.05, 0.11) * sc)
                    .hot(rng.r(4.0, 9.0))
                    .phys(0.0, 0.4, 3.2),
            );
        }
        if rng.f() < 0.6 {
            let v = rng.cone(axis, 0.6) * rng.r(3.0, 10.0);
            out.push(Emit::new(Kind::Ember, tip + axis * 0.03, v, rng.r(0.25, 0.6)).size(0.012, 0.004).hot(12.0).phys(9.8, 0.0, 0.7));
        }
    }
}

fn manifold_flash(out: &mut Vec<Emit>, rng: &mut Rng, path: &PolyPath) {
    let len = path.len();
    if len < 0.05 {
        return;
    }
    for _ in 0..12 {
        let s = rng.r(0.0, len * 0.6);
        let a = path.at(s);
        let dir = (path.at((s + 0.03).min(len)) - a).normalize_or_zero();
        out.push(
            Emit::new(Kind::Fire, a + rng.unit() * 0.01, dir * rng.r(3.0, 9.0), rng.r(0.08, 0.2))
                .size(0.025, rng.r(0.05, 0.085))
                .hot(rng.r(6.0, 12.0))
                .phys(0.0, 0.0, 2.0),
        );
    }
}

#[derive(Default)]
pub struct PuffState {
    prev_backfire: Vec<f32>,
    smoke_acc: f32,
    crackle: f32,
    last_tick: i64,
    rng: u32,
}

fn fire_ramp(t: f32) -> Vec3 {
    let (a, b, c) = (Vec3::new(1.0, 0.82, 0.45), Vec3::new(1.0, 0.42, 0.08), Vec3::new(0.5, 0.07, 0.01));
    if t < 0.4 {
        a.lerp(b, t / 0.4)
    } else {
        b.lerp(c, (t - 0.4) / 0.6)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn puff_system(
    sim: Res<Sim>,
    time: Res<Time>,
    g: Option<Res<RigGeom>>,
    paths: Option<Res<FlowPaths>>,
    mut pool: ResMut<PuffPool>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut lights: Query<(&mut PointLight, &mut Transform), (With<TailLight>, Without<Puff>)>,
    mut q: Query<(&mut Puff, &mut Transform, &mut Visibility)>,
    mut st: Local<PuffState>,
) {
    if pool.rings[0].is_empty() {
        return;
    }
    if st.rng == 0 {
        st.rng = 0x9E37_79B9;
    }
    let mut rng = Rng(st.rng);
    let real_dt = time.delta_secs();
    let dt = if sim.paused { 0.0 } else { real_dt * sim.time_scale };
    let tt = time.elapsed_secs();
    let tel = &sim.tel;
    let tip = pool.tail;
    let n = sim.backfire_flash.len();
    let mut out: Vec<Emit> = Vec::new();
    let mut flash = 0.0f32;

    // --- backfires: one burst per fresh event
    st.prev_backfire.resize(n, 0.0);
    for l in 0..n {
        let v = sim.backfire_flash[l];
        if v > st.prev_backfire[l] + 0.05 {
            flash = 1.0;
            tail_burst(&mut out, &mut rng, tip, 1.0, false);
            if let Some(p) = paths.as_ref().and_then(|p| p.exhaust.get(l)) {
                manifold_flash(&mut out, &mut rng, p);
            }
        }
        st.prev_backfire[l] = v;
    }
    let backfire_light = sim.backfire_flash.iter().cloned().fold(0.0f32, f32::max);

    // --- decel / limiter crackle: small pops
    let crackle_on = tel.is_rev_limiting
        || (tel.throttle < 0.02 && tel.rpm > 3800.0 && tel.is_running && tel.air_fuel_ratio < 30.0 && tel.exhaust_gas_temp_k > 800.0);
    let tick = (tt * 40.0).floor() as i64;
    if crackle_on && tick != st.last_tick && hash01(tick as f32) > 0.55 {
        st.crackle = 0.45;
        tail_burst(&mut out, &mut rng, tip, 0.4, true);
    }
    st.last_tick = tick;
    // burble / overrun pops reported by the synth (or the sim's own gate when there is no sound)
    for _ in 0..sim.pops_this_frame.min(3) {
        let louder = if sim.tuning.is_cat_deleted() || sim.tuning.muffler == Muffler::Delete { 1.3 } else { 1.0 };
        let k = sim.pop_strength * louder;
        pop_burst(&mut out, &mut rng, tip, k);
        st.crackle = st.crackle.max(0.25 + 0.35 * k.min(1.2));
    }
    st.crackle = (st.crackle - real_dt * 6.0).max(0.0);
    let intensity = backfire_light.max(st.crackle);
    for (mut l, mut tf) in &mut lights {
        l.intensity = 900_000.0 * intensity;
        tf.translation = tip + tail_axis() * 0.3;
    }
    let _ = flash;

    // --- continuous exhaust plume
    if tel.is_running && dt > 0.0 {
        let diesel = sim.cfg().ignition_type == IgnitionType::Compression;
        let flow = (tel.rpm as f32 / 7000.0).clamp(0.0, 1.2) * (0.25 + 0.75 * tel.throttle as f32);
        let egt = tel.exhaust_gas_temp_k as f32;
        let afr = tel.air_fuel_ratio as f32;
        let cold = 1.0 - ((egt - 430.0) / 220.0).clamp(0.0, 1.0);
        let rich = if afr > 1.0 { ((12.6 - afr) / 3.0).clamp(0.0, 1.0) } else { 0.0 };
        let soot = rich.max(if diesel { ((tel.throttle as f32 - 0.5) * 1.4).clamp(0.0, 0.8) } else { 0.0 });
        let vis = (0.05 + 0.3 * cold).max(0.5 * soot);
        let grey = 0.75 * (1.0 - soot) + 0.04 * soot;
        // emission rate is per fx-second but capped by real frames so slow-mo does not starve the plume
        st.smoke_acc += (10.0 + 45.0 * flow) * dt.max(real_dt * 0.25);
        let mut guard = 0;
        while st.smoke_acc >= 1.0 && guard < 6 {
            st.smoke_acc -= 1.0;
            guard += 1;
            let axis = tail_axis();
            let v = rng.cone(axis, 0.18) * (1.0 + 2.2 * flow);
            out.push(
                Emit::new(Kind::Smoke, tip + axis * 0.02 + rng.unit() * 0.008, v, rng.r(1.3, 2.0))
                    .size(0.025, rng.r(0.12, 0.2))
                    .tint(Vec3::splat(grey))
                    .alpha(vis)
                    .phys(0.0, 0.22, 1.5),
            );
        }
    }

    // --- per-cylinder: fuel spray + exhaust blowdown
    if let (Some(g), Some(paths)) = (g.as_ref(), paths.as_ref()) {
        let lay = &g.lay;
        if tel.is_running && sim.tables.len() == lay.n() && paths.exhaust.len() == lay.n() {
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
                    if valve_open_amount(&g, th, true) > 0.12 && dd < 25.0 && dd > -g.int_dur_deg * 0.5 {
                        let side = -cg.sigma;
                        let from = Vec3::new(cg.x, lay.deck + 0.075, side * (b * 0.5 + 0.045));
                        for _ in 0..3 {
                            let tgt = Vec3::new(cg.x + rng.r(-0.2, 0.2) * b, seat_y, side * 0.19 * b);
                            let dir = (tgt - from).normalize_or_zero();
                            let v = tilt * (rng.cone(dir, 0.12) * rng.r(0.9, 1.6));
                            out.push(
                                Emit::new(Kind::Mist, bank_to_engine(lay.tilts[cg.bank], from), v, rng.r(0.07, 0.13))
                                    .size(0.004, 0.012)
                                    .hot(rng.r(3.0, 6.0))
                                    .tint(Vec3::new(0.55, 0.85, 1.0)),
                            );
                        }
                    }
                } else if (345.0..380.0).contains(&th) {
                    // diesel: direct injection plume into the chamber
                    for _ in 0..5 {
                        let from = Vec3::new(cg.x, lay.deck - 0.004, 0.0);
                        let v = tilt * (rng.cone(Vec3::NEG_Y, 0.55) * rng.r(1.2, 2.4));
                        out.push(
                            Emit::new(Kind::Mist, bank_to_engine(lay.tilts[cg.bank], from), v, rng.r(0.06, 0.11))
                                .size(0.004, 0.011)
                                .hot(5.0)
                                .tint(Vec3::new(1.0, 0.9, 0.55)),
                        );
                    }
                }
                // exhaust-valve-open blowdown puff, strength from the recorded cylinder pressure
                let from = (sim.alpha_v_prev as f32 - sim.offsets[ci]).rem_euclid(720.0);
                let span = (th - from).rem_euclid(720.0);
                let evo = g.exh_peak_deg - g.exh_dur_deg * 0.5;
                if span > 0.0 && span < 360.0 && (evo - from).rem_euclid(720.0) < span {
                    let k = ((sim.tables[ci].pressure_at(490.0) - 1.2e5) / 5.0e5).clamp(0.1, 1.6);
                    let path = &paths.exhaust[ci];
                    let a = path.at(0.0);
                    let dir = (path.at(0.04) - a).normalize_or_zero();
                    for _ in 0..3 {
                        out.push(
                            Emit::new(Kind::Fire, a + rng.unit() * 0.006, dir * (2.0 + 6.0 * k) * rng.r(0.6, 1.0), rng.r(0.08, 0.14))
                                .size(0.02, 0.045 + 0.02 * k)
                                .hot(2.5 + 4.0 * k)
                                .phys(0.0, 0.0, 2.5),
                        );
                    }
                }
            }
        }
    }

    // --- allocate pool slots
    for e in out {
        let k = e.kind as usize;
        let ring = &pool.rings[k];
        if ring.is_empty() {
            continue;
        }
        let ent = ring[pool.next[k] % ring.len()];
        pool.next[k] = (pool.next[k] + 1) % ring.len();
        if let Ok((mut p, mut tf, mut vis)) = q.get_mut(ent) {
            p.alive = true;
            p.pos = e.pos;
            p.vel = e.vel;
            p.age = 0.0;
            p.life = e.life.max(0.01);
            p.size0 = e.size0;
            p.size1 = e.size1;
            p.hot = e.hot;
            p.tint = e.tint;
            p.grav = e.grav;
            p.buoy = e.buoy;
            p.drag = e.drag;
            p.alpha = e.alpha;
            p.roll = rng.r(0.0, TAU);
            p.spin = rng.r(-1.5, 1.5);
            tf.translation = e.pos;
            *vis = Visibility::Inherited;
        }
    }
    st.rng = rng.0;

    // --- integrate + shade
    let cam_rot = cam.iter().next().map(|c| c.compute_transform().rotation).unwrap_or(Quat::IDENTITY);
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
        p.vel.y += (p.buoy - p.grav) * dt;
        let step = p.vel * dt;
        p.pos += step;
        let t = p.age / p.life;
        let grow = 1.0 - (1.0 - t) * (1.0 - t);
        let size = p.size0 + (p.size1 - p.size0) * grow;
        let fade_in = (p.age / if p.kind == Kind::Smoke { 0.12 } else { 0.012 }).min(1.0);
        let color = match p.kind {
            Kind::Fire => {
                let c = fire_ramp(t) * p.hot;
                LinearRgba::new(c.x, c.y, c.z, (1.0 - t).powf(1.4) * fade_in)
            }
            Kind::Smoke => LinearRgba::new(p.tint.x, p.tint.y, p.tint.z, p.alpha * (1.0 - t).powf(0.8) * fade_in),
            Kind::Ember => {
                let c = Vec3::new(1.0, 0.55 - 0.3 * t, 0.12) * p.hot;
                LinearRgba::new(c.x, c.y, c.z, 1.0 - t * t)
            }
            Kind::Mist => {
                let c = p.tint * p.hot;
                LinearRgba::new(c.x, c.y, c.z, (1.0 - t).powf(0.7))
            }
        };
        tf.translation = p.pos;
        tf.rotation = cam_rot * Quat::from_rotation_z(p.roll + p.spin * p.age);
        tf.scale = Vec3::splat(size.max(1e-4));
        if let Some(mut m) = materials.get_mut(&p.mat) {
            m.base_color = Color::LinearRgba(color);
        }
    }
}
