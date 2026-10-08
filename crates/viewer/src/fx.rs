//! Effects: gas-flow particles along every intake/exhaust path (timed to the valve events), the
//! tailpipe flame + light for backfires/pops, and a flickering firelight inside the engine.

use crate::meshgen::MB;
use crate::rig::anim::{local_angle, valve_open_amount, RigGeom};
use crate::rig::Ctx;
use crate::sim::Sim;
use bevy::light::PointLight;
use bevy::prelude::*;

pub struct PolyPath {
    pub pts: Vec<Vec3>,
    pub cum: Vec<f32>,
}

impl PolyPath {
    pub fn new(pts: Vec<Vec3>) -> Self {
        let mut cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for i in 0..pts.len() {
            if i > 0 {
                acc += pts[i].distance(pts[i - 1]);
            }
            cum.push(acc);
        }
        Self { pts, cum }
    }

    pub fn len(&self) -> f32 {
        *self.cum.last().unwrap_or(&0.0)
    }

    pub fn at(&self, s: f32) -> Vec3 {
        if self.pts.len() < 2 {
            return self.pts.first().copied().unwrap_or(Vec3::ZERO);
        }
        let s = s.clamp(0.0, self.len());
        let i = self.cum.partition_point(|&c| c < s).clamp(1, self.pts.len() - 1);
        let (c0, c1) = (self.cum[i - 1], self.cum[i]);
        let t = if c1 > c0 { (s - c0) / (c1 - c0) } else { 0.0 };
        self.pts[i - 1].lerp(self.pts[i], t)
    }
}

#[derive(Resource)]
pub struct FlowPaths {
    pub intake: Vec<PolyPath>,
    pub exhaust: Vec<PolyPath>,
}

#[derive(Component)]
pub struct FlowParticle {
    pub ci: usize,
    pub intake: bool,
    pub s: f32,
    pub seed: f32,
    pub alive: bool,
}

#[derive(Component)]
pub struct FireLight;

/// Total combustion glow this frame (sum over cylinders), for the firelight.
#[derive(Resource, Default)]
pub struct FireGlow(pub f32);

pub fn build_fx(cx: &mut Ctx, root: Entity, intake: &[Vec<Vec3>], _exhaust: &[Vec<Vec3>], tail_tip: Vec3, x_mid: f32, tex: &crate::puffs::PuffTex) {
    let sphere = cx.mesh(MB::sphere(0.0062, 8));
    let intake_mat = cx.material(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(0.25 * 9.0, 0.8 * 9.0, 1.0 * 9.0),
        alpha_mode: AlphaMode::Add,
        unlit: false,
        ..default()
    });
    let exhaust_mat = cx.material(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(1.0 * 10.0, 0.38 * 10.0, 0.06 * 10.0),
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    let n = intake.len();
    for ci in 0..n {
        for k in 0..9 {
            let e = cx
                .put(root, &sphere, &intake_mat, Transform::from_scale(Vec3::ZERO))
                .to_owned();
            cx.cmd.entity(e).insert((
                FlowParticle { ci, intake: true, s: 1e9, seed: ((ci * 31 + k * 17) % 97) as f32 / 97.0, alive: false },
                Visibility::Hidden,
            ));
        }
        for k in 0..12 {
            let e = cx.put(root, &sphere, &exhaust_mat, Transform::from_scale(Vec3::ZERO));
            cx.cmd.entity(e).insert((
                FlowParticle { ci, intake: false, s: 1e9, seed: ((ci * 13 + k * 29) % 89) as f32 / 89.0, alive: false },
                Visibility::Hidden,
            ));
        }
    }

    // tailpipe light (flame itself is the particle system in puffs.rs)
    crate::puffs::build_pool(cx, root, tex, tail_tip);
    let l = cx
        .cmd
        .spawn((
            PointLight { intensity: 0.0, color: Color::srgb(1.0, 0.5, 0.15), range: 4.0, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(tail_tip + Vec3::new(0.25, 0.0, 0.0)),
            crate::puffs::TailLight,
            ChildOf(root),
        ))
        .id();
    let _ = l;
    // firelight inside the engine
    cx.cmd.spawn((
        PointLight { intensity: 0.0, color: Color::srgb(1.0, 0.45, 0.12), range: 1.6, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(x_mid, 0.1, 0.0),
        FireLight,
        ChildOf(root),
    ));
}

pub fn hash01(x: f32) -> f32 {
    (x * 12.9898).sin().abs().fract()
}

pub fn flow_system(
    sim: Res<Sim>,
    g: Option<Res<RigGeom>>,
    paths: Option<Res<FlowPaths>>,
    mut q: Query<(&mut FlowParticle, &mut Transform, &mut Visibility)>,
) {
    let (Some(g), Some(paths)) = (g, paths) else { return };
    if sim.tables.len() != g.lay.n() || paths.intake.len() != g.lay.n() {
        return;
    }
    let dt = sim.vis_dt;
    let t = &sim.tel;
    let load = (t.throttle as f32).max(0.0);
    let map_ratio = (t.manifold_pressure_pa / t.atmospheric_pressure_pa.max(1.0)) as f32;
    for (mut p, mut tf, mut vis) in &mut q {
        let ci = p.ci;
        let th = local_angle(&sim, ci);
        let open = valve_open_amount(&g, th, p.intake);
        let path = if p.intake { &paths.intake[ci] } else { &paths.exhaust[ci] };
        let len = path.len();
        let pr = sim.tables[ci].pressure_at(490.0);
        let k = ((pr - 1.2e5) / 5.0e5).clamp(0.1, 1.6);
        let v = if p.intake { 2.2 + 5.5 * (0.15 + 0.85 * load) * map_ratio.clamp(0.2, 2.0) } else { 3.0 + 9.0 * k };
        if p.alive {
            p.s += v * dt;
            if p.s > len {
                p.alive = false;
            }
        }
        if !p.alive && sim.tel.is_running {
            p.seed = (p.seed * 13.37 + 0.618).fract();
            if p.seed < open * 0.4 {
                p.alive = true;
                p.s = 0.0;
            }
        }
        if p.alive {
            *vis = Visibility::Inherited;
            let jitter = Vec3::new(hash01(p.seed * 7.0 + 1.0) - 0.5, hash01(p.seed * 3.0 + 2.0) - 0.5, hash01(p.seed * 5.0 + 3.0) - 0.5) * 0.012;
            tf.translation = path.at(p.s) + jitter;
            let life = (p.s / len.max(0.01)).clamp(0.0, 1.0);
            let size = if p.intake { 0.7 + 0.8 * load } else { (0.6 + 0.8 * k) * (1.0 - 0.55 * life) };
            tf.scale = Vec3::splat(size);
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

pub fn fire_light_system(glow: Res<FireGlow>, mut q: Query<&mut PointLight, With<FireLight>>) {
    for mut l in &mut q {
        l.intensity = (glow.0 * 55_000.0).min(900_000.0);
    }
}
