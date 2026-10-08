//! Animation components + systems. Everything is driven by the sim's visual playback state
//! (`Sim::alpha_v` etc.) and the per-angle cycle tables, so it looks right at any playback speed.

use crate::layout::Layout;
use crate::meshgen::cam_bump;
use crate::sim::Sim;
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Shared geometry/valve constants for animation, stored as a resource at rig build time.
#[derive(Resource, Clone)]
#[allow(dead_code)]
pub struct RigGeom {
    pub lay: Layout,
    pub valve_lift: f32,
    pub int_peak_deg: f32,
    pub exh_peak_deg: f32,
    pub int_dur_deg: f32,
    pub exh_dur_deg: f32,
    pub spring_len: f32,
    pub bridge_rest_y: f32,
    pub cam_base_r: f32,
    pub is_turbo: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SpinSrc {
    /// Crankshaft: -alpha_v about +X.
    Crank,
    /// Camshafts: half crank speed.
    Cam,
    /// Gearbox input shaft.
    Input,
    /// Output shaft / driveshaft.
    Output,
    /// Wheels, about +Z (forward rolling).
    Wheel,
    /// Turbo shaft.
    Turbo,
}

/// Rotates its entity about a local axis: `rotation = base * axis_angle(axis, ratio * src + phase)`.
#[derive(Component)]
pub struct Spin {
    pub src: SpinSrc,
    pub axis: Vec3,
    pub ratio: f32,
    pub phase: f32,
    pub base: Quat,
}

impl Spin {
    pub fn x(src: SpinSrc, ratio: f32, phase: f32) -> Self {
        Self { src, axis: Vec3::X, ratio, phase, base: Quat::IDENTITY }
    }
}

#[derive(Component)]
pub struct PistonC(pub usize);
#[derive(Component)]
pub struct RodC(pub usize);
#[derive(Component)]
pub struct ValveC {
    pub ci: usize,
    pub intake: bool,
    pub base_y: f32,
}
#[derive(Component)]
pub struct BridgeC {
    pub ci: usize,
    pub intake: bool,
}
#[derive(Component)]
pub struct SpringC {
    pub ci: usize,
    pub intake: bool,
}
#[derive(Component)]
#[allow(dead_code)]
pub struct FlameC {
    pub ci: usize,
    pub mat: Handle<StandardMaterial>,
    pub crown_to_deck: f32,
    /// Brightness held (and decaying in fx time) after the burn so a ~20 ms flame stays visible.
    pub hold: f32,
    pub r_hold: f32,
    pub burn_prev: f32,
}
/// One bright lick on the expanding flame front.
#[derive(Component)]
pub struct Flamelet {
    pub ci: usize,
    pub k: usize,
    pub mat: Handle<StandardMaterial>,
}
/// Jagged arc variants across the plug gap; one is shown per frame while the spark lasts.
#[derive(Component)]
pub struct SparkBolt {
    pub ci: usize,
    pub variant: usize,
}
#[derive(Component)]
pub struct SparkC {
    pub ci: usize,
    pub mat: Handle<StandardMaterial>,
    pub timer: f32,
    pub color: Color,
}
#[derive(Component)]
pub struct ThrottlePlate;
#[derive(Component)]
#[allow(dead_code)]
pub struct HeatGlow {
    pub mat: Handle<StandardMaterial>,
    pub gain: f32,
    pub base: Color,
}
#[derive(Component)]
pub struct ClutchDisc {
    pub rest_x: f32,
    pub travel: f32,
}
#[derive(Component)]
pub struct GearHighlight {
    pub gear_idx: usize,
    pub mat: Handle<StandardMaterial>,
}
#[derive(Component)]
pub struct BrakeGlow {
    pub mat: Handle<StandardMaterial>,
}
#[derive(Component)]
pub struct WastegateC;
#[derive(Component)]
pub struct BoostGlow {
    pub mat: Handle<StandardMaterial>,
}
#[derive(Component)]
pub struct PressureTint {
    pub ci: usize,
    pub mat: Handle<StandardMaterial>,
}
pub fn spin_angle(sim: &Sim, src: SpinSrc) -> f32 {
    match src {
        SpinSrc::Crank => -((sim.alpha_v.rem_euclid(720.0)) as f32).to_radians(),
        SpinSrc::Cam => -((sim.alpha_v.rem_euclid(720.0)) as f32).to_radians() * 0.5,
        SpinSrc::Input => -sim.input_angle.rem_euclid(TAU),
        SpinSrc::Output => -sim.shaft_angle.rem_euclid(TAU),
        SpinSrc::Wheel => sim.wheel_angle.rem_euclid(TAU),
        SpinSrc::Turbo => sim.turbo_angle.rem_euclid(TAU),
    }
}

pub fn spin_system(sim: Res<Sim>, mut q: Query<(&Spin, &mut Transform)>) {
    for (s, mut t) in &mut q {
        let a = spin_angle(&sim, s.src) * s.ratio + s.phase;
        t.rotation = s.base * Quat::from_axis_angle(s.axis, a);
    }
}

/// Cylinder-local crank angle (deg, 0..720) for this cylinder at the visual playback angle.
pub fn local_angle(sim: &Sim, ci: usize) -> f32 {
    ((sim.alpha_v as f32) - sim.offsets[ci]).rem_euclid(720.0)
}

/// Piston pin distance from the crank axis along the bore (slider-crank).
pub fn pin_dist(r: f32, l: f32, theta: f32) -> f32 {
    r * theta.cos() + (l * l - r * r * theta.sin() * theta.sin()).sqrt()
}

pub fn valve_open_amount(g: &RigGeom, local_deg: f32, intake: bool) -> f32 {
    valve_lift(g, local_deg, intake) / g.valve_lift.max(1e-6)
}

fn valve_lift(g: &RigGeom, local_deg: f32, intake: bool) -> f32 {
    let (peak, dur) = if intake { (g.int_peak_deg, g.int_dur_deg) } else { (g.exh_peak_deg, g.exh_dur_deg) };
    let mut d = local_deg - peak;
    d = (d + 360.0).rem_euclid(720.0) - 360.0;
    let d_cam = (d * 0.5).to_radians();
    g.valve_lift * cam_bump(d_cam, (dur * 0.25).to_radians())
}

pub fn piston_system(
    sim: Res<Sim>,
    g: Option<Res<RigGeom>>,
    mut pistons: Query<(&PistonC, &mut Transform), (Without<RodC>, Without<ValveC>)>,
    mut rods: Query<(&RodC, &mut Transform), (Without<PistonC>, Without<ValveC>)>,
) {
    let Some(g) = g else { return };
    let lay = &g.lay;
    if sim.tables.len() != lay.n() {
        return;
    }
    for (p, mut t) in &mut pistons {
        let cg = &lay.cyls[p.0];
        let th = local_angle(&sim, p.0).to_radians();
        t.translation = Vec3::new(cg.x, pin_dist(lay.r, lay.rod_len, th), 0.0);
    }
    for (rd, mut t) in &mut rods {
        let cg = &lay.cyls[rd.0];
        let th = local_angle(&sim, rd.0).to_radians();
        let pd = pin_dist(lay.r, lay.rod_len, th);
        let small = Vec2::new(pd, 0.0); // (Y, Z) in bank frame
        let big = Vec2::new(lay.r * th.cos(), -lay.r * th.sin());
        let d = (big - small).normalize_or_zero();
        let gamma = (-d.y).atan2(-d.x);
        t.translation = Vec3::new(cg.x, small.x, small.y);
        t.rotation = Quat::from_rotation_x(gamma);
    }
}

pub fn valve_system(
    sim: Res<Sim>,
    g: Option<Res<RigGeom>>,
    mut valves: Query<(&ValveC, &mut Transform), (Without<SpringC>, Without<BridgeC>)>,
    mut springs: Query<(&SpringC, &mut Transform), (Without<ValveC>, Without<BridgeC>)>,
    mut bridges: Query<(&BridgeC, &mut Transform), (Without<ValveC>, Without<SpringC>)>,
) {
    let Some(g) = g else { return };
    if sim.tables.len() != g.lay.n() {
        return;
    }
    for (v, mut t) in &mut valves {
        let lift = valve_lift(&g, local_angle(&sim, v.ci), v.intake);
        t.translation.y = v.base_y - lift;
    }
    for (s, mut t) in &mut springs {
        let lift = valve_lift(&g, local_angle(&sim, s.ci), s.intake);
        t.scale.y = ((g.spring_len - lift) / g.spring_len).max(0.2);
    }
    for (b, mut t) in &mut bridges {
        let lift = valve_lift(&g, local_angle(&sim, b.ci), b.intake);
        t.translation.y = g.bridge_rest_y - lift;
    }
}

fn flame_color(heat: f32) -> LinearRgba {
    // deep orange -> yellow -> white-blue as it gets hotter/denser
    let h = heat.clamp(0.0, 1.0);
    let c = Vec3::new(1.0, 0.28 + 0.55 * h, 0.04 + 0.7 * h * h);
    LinearRgba::rgb(c.x, c.y, c.z)
}

pub fn flame_system(
    sim: Res<Sim>,
    g: Option<Res<RigGeom>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut glow: ResMut<crate::fx::FireGlow>,
    mut q: Query<(&mut FlameC, &mut Transform, &mut Visibility)>,
    mut lets: Query<(&Flamelet, &mut Transform, &mut Visibility), (Without<FlameC>, Without<SparkC>)>,
    mut sparks: Query<(&mut SparkC, &mut Transform, &mut Visibility), Without<FlameC>>,
    time: Res<Time>,
) {
    let Some(g) = g else { return };
    let lay = &g.lay;
    if sim.tables.len() != lay.n() {
        return;
    }
    let dt = time.delta_secs();
    let fx_dt = if sim.paused { 0.0 } else { dt * sim.time_scale };
    let tt = time.elapsed_secs();
    glow.0 = 0.0;
    // per cylinder: (front radius, brightness, crown y, gap) while the flame front is alive
    let mut front: Vec<Option<(f32, f32, f32, f32)>> = vec![None; lay.n()];
    for (mut f, mut t, mut vis) in &mut q {
        let th_deg = local_angle(&sim, f.ci);
        let table = &sim.tables[f.ci];
        let burn = table.burn_at(th_deg);
        let p = table.pressure_at(th_deg);
        let in_power = (360.0..560.0).contains(&th_deg);
        let edge = ((560.0 - th_deg) / 60.0).clamp(0.0, 1.0);
        let pterm = ((p - 1.2e6) / 5.5e6).clamp(0.0, 1.0);
        // glow tracks heat release while burning, then fades with falling pressure
        let live = if in_power && burn > 0.001 { pterm.max(0.2 * burn.sqrt()) * edge } else { 0.0 };
        if burn < f.burn_prev - 0.3 {
            f.r_hold = 0.0;
        }
        f.burn_prev = burn;
        f.hold = (f.hold * (-fx_dt / 0.07).exp()).max(live);
        if live > 0.0 {
            // flat disc-shaped chamber: front area grows with burned fraction, starting at the plug
            let r_now = (0.5 * lay.bore * burn.clamp(0.0, 1.0).sqrt()).max(0.003);
            f.r_hold = f.r_hold.max(r_now);
        }
        if sim.scrub {
            // turned by hand: show exactly this angle, no afterglow
            f.hold = live;
            f.r_hold = if live > 0.0 { (0.5 * lay.bore * burn.clamp(0.0, 1.0).sqrt()).max(0.003) } else { 0.0 };
        }
        let theta = th_deg.to_radians();
        let crown = pin_dist(lay.r, lay.rod_len, theta) + lay.comp_h;
        let gap = (lay.deck - crown).max(0.0006);
        if f.hold < 0.012 {
            *vis = Visibility::Hidden;
            continue;
        }
        let hold = f.hold;
        glow.0 += hold;
        *vis = Visibility::Inherited;
        let cg = &lay.cyls[f.ci];
        t.translation = Vec3::new(cg.x, crown + gap * 0.5, 0.0);
        t.scale = Vec3::new(f.r_hold, gap * 0.5, f.r_hold);
        if let Some(mut m) = materials.get_mut(&f.mat) {
            let e = 2.0 + 16.0 * hold;
            let c = flame_color(0.0 + 0.45 * hold);
            m.emissive = LinearRgba::rgb(c.red * e, c.green * e, c.blue * e);
        }
        if burn < 0.995 || hold > 0.25 {
            front[f.ci] = Some((f.r_hold, hold, crown, gap));
        }
    }
    // bright flamelets riding the expanding front
    for (fl, mut t, mut vis) in &mut lets {
        let Some((r, h, crown, gap)) = front[fl.ci] else {
            *vis = Visibility::Hidden;
            continue;
        };
        if fl.k == 0 {
            if let Some(mut m) = materials.get_mut(&fl.mat) {
                let c = flame_color(0.7 + 0.3 * h);
                let e = 25.0 + 45.0 * h;
                m.emissive = LinearRgba::rgb(c.red * e, c.green * e, c.blue * e);
            }
        }
        let seed = fl.ci as f32 * 0.37 + fl.k as f32 * 1.71;
        let a = fl.k as f32 * TAU / 5.0 + 5.0 * crate::fx::hash01(tt * 17.0 + seed);
        let rr = r * (0.6 + 0.4 * crate::fx::hash01(tt * 29.0 + seed * 3.0));
        let y = crown + gap * (0.12 + 0.76 * crate::fx::hash01(tt * 23.0 + seed * 5.0));
        let sz = (gap * 0.42).clamp(0.003, 0.02) * (0.6 + 0.8 * crate::fx::hash01(tt * 41.0 + seed * 7.0)) * h.sqrt();
        *vis = Visibility::Inherited;
        t.translation = Vec3::new(lay.cyls[fl.ci].x + rr * a.cos(), y, rr * a.sin());
        t.scale = Vec3::splat(sz);
    }
    // Spark: flash when the recorded cycle table says ignition happened in the bins we just crossed.
    for (mut s, mut t, mut vis) in &mut sparks {
        let ci = s.ci;
        let to = local_angle(&sim, ci);
        // scrubbing: look a few degrees either side of the exact angle, since nothing is moving
        let from = if sim.scrub { (to - 4.0).rem_euclid(720.0) } else { ((sim.alpha_v_prev as f32) - sim.offsets[ci]).rem_euclid(720.0) };
        let span = (to - from).rem_euclid(720.0);
        let mut kind = 0u8;
        if span > 0.0 && span < 360.0 {
            let n = span.ceil() as i32 + 1;
            for k in 0..=n {
                let bin = ((from + k as f32).rem_euclid(720.0)) as usize % 720;
                let fl = sim.tables[ci].flags[bin];
                if fl & 2 != 0 {
                    kind = 2;
                } else if fl & 1 != 0 && kind == 0 {
                    kind = 1;
                }
            }
        }
        if kind == 1 {
            s.timer = 0.07;
            s.color = Color::srgb(0.7, 0.85, 1.0);
        } else if kind == 2 {
            s.timer = 0.12;
            s.color = Color::srgb(1.0, 0.1, 0.05);
        }
        if sim.scrub {
            s.timer = if kind != 0 { 0.07 } else { 0.0 };
        }
        s.timer = (s.timer - dt).max(0.0);
        if s.timer > 0.0 {
            *vis = Visibility::Inherited;
            let k = (s.timer / 0.07).min(1.0);
            t.scale = Vec3::splat(0.5 + k);
            if let Some(mut m) = materials.get_mut(&s.mat) {
                let c = s.color.to_linear();
                m.emissive = LinearRgba::rgb(c.red * 60.0 * k, c.green * 60.0 * k, c.blue * 60.0 * k);
            }
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

/// Shows one jagged arc variant (re-picked every frame, so it crackles) while a spark is active.
pub fn spark_bolt_system(time: Res<Time>, sparks: Query<&SparkC>, mut bolts: Query<(&SparkBolt, &mut Visibility)>) {
    let mut timers: Vec<f32> = Vec::new();
    for s in &sparks {
        if timers.len() <= s.ci {
            timers.resize(s.ci + 1, 0.0);
        }
        timers[s.ci] = s.timer;
    }
    let pick = (time.elapsed_secs() * 50.0) as usize;
    for (b, mut vis) in &mut bolts {
        let on = timers.get(b.ci).copied().unwrap_or(0.0) > 0.0 && (pick + b.ci) % 3 == b.variant;
        *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
    }
}

pub fn throttle_plate_system(sim: Res<Sim>, mut q: Query<&mut Transform, With<ThrottlePlate>>) {
    // closed plate faces the airflow (normal along X); wide open turns edge-on
    let open = sim.tel.throttle.max(sim.tel.manifold_pressure_pa * 0.0) as f32;
    let ang = (0.06 + open * 1.45).min(1.5);
    for mut t in &mut q {
        t.rotation = Quat::from_rotation_z(ang);
    }
}

pub fn heat_glow_system(
    sim: Res<Sim>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<&HeatGlow>,
) {
    // Exhaust glow: ramps from invisible to dull cherry-red to orange with EGT and load.
    let egt = sim.tel.exhaust_gas_temp_k as f32;
    let heat = ((egt - 600.0) / 450.0).clamp(0.0, 1.0) * if sim.tel.is_running { 1.0 } else { 0.0 };
    for g in &q {
        if let Some(mut m) = materials.get_mut(&g.mat) {
            let h = (heat * g.gain).clamp(0.0, 1.5);
            // black-body-ish ramp: dull red first, orange only when really hot
            let e = h * h * h * 2.4;
            m.emissive = LinearRgba::rgb(1.0 * e, (0.03 + 0.10 * h) * e, 0.006 * e);
        }
    }
}

pub fn pressure_tint_system(
    sim: Res<Sim>,
    g: Option<Res<RigGeom>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<&PressureTint>,
) {
    let Some(g) = g else { return };
    if sim.tables.len() != g.lay.n() {
        return;
    }
    for p in &q {
        let th = local_angle(&sim, p.ci);
        let pr = sim.tables[p.ci].pressure_at(th);
        let k = ((pr - 101_325.0) / 4.0e6).clamp(0.0, 1.0);
        if let Some(mut m) = materials.get_mut(&p.mat) {
            m.base_color = Color::srgba(0.75 + 0.25 * k, 0.88 - 0.35 * k, 1.0 - 0.8 * k, 0.09 + 0.1 * k);
            // the cylinder being followed in the analysis panel glows in the colour of its current stroke
            m.emissive = if sim.focus_cyl == Some(p.ci) {
                let c = crate::cycle::stroke_color(crate::cycle::stroke_of(local_angle(&sim, p.ci)));
                let lin = Color::srgb_u8(c.r(), c.g(), c.b()).to_linear();
                LinearRgba::rgb(lin.red * 2.2, lin.green * 2.2, lin.blue * 2.2)
            } else {
                LinearRgba::BLACK
            };
        }
    }
}

pub fn clutch_system(sim: Res<Sim>, mut q: Query<(&ClutchDisc, &mut Transform)>) {
    let open = 1.0 - sim.tel.clutch_engagement as f32;
    for (c, mut t) in &mut q {
        t.translation.x = c.rest_x + c.travel * open;
    }
}

pub fn gear_highlight_system(
    sim: Res<Sim>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<&GearHighlight>,
) {
    for h in &q {
        let on = h.gear_idx == sim.gear_idx && sim.gear_idx != 1;
        if let Some(mut m) = materials.get_mut(&h.mat) {
            m.emissive = if on { LinearRgba::rgb(0.0, 1.6, 0.5) } else { LinearRgba::BLACK };
        }
    }
}

pub fn brake_glow_system(sim: Res<Sim>, mut materials: ResMut<Assets<StandardMaterial>>, q: Query<&BrakeGlow>) {
    // Rotor temperature follows recent braking energy: a slow integrator kept in the telemetry brake
    // input and vehicle speed. Cheap proxy: glow with brake * speed.
    let heat = (sim.tel.brake as f32 * (sim.tel.vehicle_speed_mps as f32 / 25.0)).clamp(0.0, 1.0);
    for b in &q {
        if let Some(mut m) = materials.get_mut(&b.mat) {
            let e = heat * heat * 4.0;
            m.emissive = LinearRgba::rgb(e, 0.18 * e, 0.0);
        }
    }
}

pub fn boost_glow_system(sim: Res<Sim>, mut materials: ResMut<Assets<StandardMaterial>>, q: Query<&BoostGlow>) {
    let k = (sim.tel.boost_pressure_pa as f32 / 150_000.0).clamp(0.0, 1.0);
    for b in &q {
        if let Some(mut m) = materials.get_mut(&b.mat) {
            m.emissive = LinearRgba::rgb(0.1 * k * 6.0, 0.45 * k * 6.0, 1.0 * k * 6.0);
        }
    }
}
