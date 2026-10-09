//! Effects: the paths the gas follows through every intake runner and exhaust pipe (`gas.rs` draws the gas on them),
//! the tailpipe light for backfires/pops, and a flickering firelight inside the engine.

use crate::rig::Ctx;
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
pub struct FireLight;

/// Total combustion glow this frame (sum over cylinders), for the firelight.
#[derive(Resource, Default)]
pub struct FireGlow(pub f32);

#[allow(clippy::too_many_arguments)]
pub fn build_fx(
    cx: &mut Ctx,
    root: Entity,
    _intake: &[Vec<Vec3>],
    _exhaust: &[Vec<Vec3>],
    tail_tips: &[Vec3],
    tail_dir: Vec3,
    tail_r: f32,
    x_mid: f32,
    tex: &crate::puffs::PuffTex,
) {
    let tail_tip = tail_tips.first().copied().unwrap_or(Vec3::ZERO);
    // tailpipe light (the flame itself is the volume in volume.rs)
    crate::puffs::build_pool(cx, root, tex, tail_tips, tail_dir, tail_r);
    cx.cmd.insert_resource(crate::volume::PendingPlume { root, tips: tail_tips.to_vec(), axis: tail_dir, radius: tail_r });
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

pub fn fire_light_system(glow: Res<FireGlow>, mut q: Query<&mut PointLight, With<FireLight>>) {
    for mut l in &mut q {
        l.intensity = (glow.0 * 55_000.0).min(900_000.0);
    }
}
