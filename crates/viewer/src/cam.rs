//! Smooth orbit camera with named focus presets.

use crate::scene::{FloorMark, MainCamera};
use crate::sim::Sim;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::post_process::dof::DepthOfField;
use bevy::prelude::*;

#[derive(Clone)]
pub struct Anchor {
    pub name: &'static str,
    pub target: Vec3,
    pub dist: f32,
    pub yaw: f32,
    pub pitch: f32,
}

/// Focus points the rig publishes after (re)building, so presets always frame the right parts.
#[derive(Resource, Default)]
pub struct Anchors(pub Vec<Anchor>);

#[derive(Resource)]
pub struct CamRig {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub t_target: Vec3,
    pub t_yaw: f32,
    pub t_pitch: f32,
    pub t_dist: f32,
    pub preset: usize,
    pub auto_orbit: bool,
    pub ui_wants_pointer: bool,
    pub shake: f32,
    /// Ignore mouse/keyboard camera input (automated screenshots).
    pub locked: bool,
}

impl Default for CamRig {
    fn default() -> Self {
        Self {
            target: Vec3::new(0.3, 0.0, 0.0),
            yaw: 0.9,
            pitch: 0.35,
            dist: 1.6,
            t_target: Vec3::new(0.3, 0.0, 0.0),
            t_yaw: 0.9,
            t_pitch: 0.35,
            t_dist: 1.6,
            preset: usize::MAX,
            auto_orbit: false,
            ui_wants_pointer: false,
            shake: 0.6,
            locked: false,
        }
    }
}

impl CamRig {
    pub fn goto(&mut self, a: &Anchor) {
        self.t_target = a.target;
        self.t_yaw = a.yaw;
        self.t_pitch = a.pitch;
        self.t_dist = a.dist;
    }
}

pub fn camera_input(
    mut rig: ResMut<CamRig>,
    anchors: Res<Anchors>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
) {
    if rig.locked {
        return;
    }
    if keys.just_pressed(KeyCode::KeyC) && !anchors.0.is_empty() {
        let next = if rig.preset == usize::MAX { 0 } else { (rig.preset + 1) % anchors.0.len() };
        rig.preset = next;
        let a = anchors.0[next].clone();
        rig.goto(&a);
    }
    if keys.just_pressed(KeyCode::KeyO) {
        rig.auto_orbit = !rig.auto_orbit;
    }
    if rig.ui_wants_pointer || rig.locked {
        return;
    }
    let d = motion.delta;
    if buttons.pressed(MouseButton::Left) {
        rig.t_yaw -= d.x * 0.0055;
        rig.t_pitch = (rig.t_pitch + d.y * 0.0055).clamp(-0.2, 1.45);
        rig.auto_orbit = false;
    }
    if buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle) {
        let yaw = rig.t_yaw;
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let up = Vec3::Y;
        let k = rig.t_dist * 0.0013;
        rig.t_target += (-right * d.x + up * d.y) * k;
    }
    if scroll.delta.y != 0.0 {
        rig.t_dist = (rig.t_dist * (1.0 - scroll.delta.y * 0.09)).clamp(0.12, 14.0);
    }
}

pub fn camera_update(
    time: Res<Time>,
    mut rig: ResMut<CamRig>,
    mut q: Query<(&mut Transform, &mut DepthOfField), With<MainCamera>>,
    sim: Res<Sim>,
) {
    let dt = time.delta_secs();
    if rig.auto_orbit {
        rig.t_yaw += dt * 0.16;
    }
    let k = 1.0 - (-dt * 6.0).exp();
    rig.target = rig.target.lerp(rig.t_target, k);
    rig.yaw += (rig.t_yaw - rig.yaw) * k;
    rig.pitch += (rig.t_pitch - rig.pitch) * k;
    rig.dist += (rig.t_dist - rig.dist) * k;

    let (sy, cy) = rig.yaw.sin_cos();
    let (sp, cp) = rig.pitch.sin_cos();
    let mut pos = rig.target + Vec3::new(cp * sy, sp, cp * cy) * rig.dist;
    // engine vibration shake, subtle and scaled with distance
    let rpm_w = (sim.tel.rpm / 6000.0) as f32;
    let amp = 0.00035 * rig.shake * (0.3 + rpm_w) * if sim.tel.is_running { 1.0 } else { 0.0 };
    let t = time.elapsed_secs() * 61.0;
    pos += Vec3::new((t * 1.3).sin(), (t * 1.7).cos(), (t * 0.9).sin()) * amp;
    for (mut tf, mut dof) in &mut q {
        *tf = Transform::from_translation(pos).looking_at(rig.target, Vec3::Y);
        dof.focal_distance = rig.dist;
        // developer hook: `NO_DOF=1` closes the aperture, for judging effects without the blur
        if std::env::var_os("NO_DOF").is_some() {
            dof.aperture_f_stops = 64.0;
        }
    }
}

/// Scrolls the floor's lane marks with vehicle speed so driving reads as motion.
pub fn scroll_floor(sim: Res<Sim>, mut q: Query<(&FloorMark, &mut Transform)>) {
    let span = 60.0;
    for (m, mut tf) in &mut q {
        // the car travels toward -X, so the floor streams toward +X
        tf.translation.x = (m.x0 + sim.floor_offset).rem_euclid(span) - span * 0.5 + 10.0;
    }
}
