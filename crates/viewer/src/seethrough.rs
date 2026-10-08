//! Keeps the thing you are looking at visible: any rig part that sits between the camera and the
//! orbit target fades to a faint ghost, then solidifies again once it is out of the way.
//!
//! A bundle of rays goes from the camera to a small disc around the target (so a part only has to
//! clip the line of sight, not hit the exact centre). Whatever they hit short of the subject gets
//! its own translucent copy of its material; shared materials are never touched, so only the
//! blocking part fades, not every part that happens to use the same steel.

use crate::cam::CamRig;
use crate::scene::MainCamera;
use bevy::camera::primitives::Aabb;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use std::collections::HashSet;

/// Marks rig meshes that may fade (everything spawned through `Ctx::put`).
#[derive(Component)]
pub struct Fadable;

/// Material is already translucent (glass, flames, glow volumes): nothing to fade.
#[derive(Component)]
pub struct NoFade;

#[derive(Component)]
pub struct Fade {
    orig: Handle<StandardMaterial>,
    ghost: Handle<StandardMaterial>,
    alpha: f32,
}

#[derive(Resource)]
pub struct SeeThrough {
    pub enabled: bool,
}

impl Default for SeeThrough {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Opacity of a part that is in the way.
const GHOST_ALPHA: f32 = 0.06;
const FADE_OUT_RATE: f32 = 9.0;
const FADE_IN_RATE: f32 = 4.5;

#[allow(clippy::too_many_arguments)]
pub fn see_through_system(
    mut commands: Commands,
    mut ray: MeshRayCast,
    cfg: Res<SeeThrough>,
    rig: Res<CamRig>,
    time: Res<Time<Real>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    fadable: Query<(), (With<Fadable>, Without<NoFade>)>,
    boxes: Query<(Entity, &GlobalTransform, &Aabb), (With<Fadable>, Without<NoFade>)>,
    mut parts: Query<(&mut MeshMaterial3d<StandardMaterial>, Option<&mut Fade>), With<Fadable>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut occluders: Local<HashSet<Entity>>,
    mut active: Local<HashSet<Entity>>,
    mut frame: Local<u32>,
) {
    *frame += 1;
    let Ok(cam_tf) = cam.single() else { return };

    // ---- find what is in the way (every other frame is plenty)
    if *frame % 2 == 0 || !cfg.enabled {
        let mut hit_set: HashSet<Entity> = HashSet::new();
        let mut extra: Vec<Entity> = Vec::new();
        let origin = cam_tf.translation();
        let target = rig.target;
        let to_target = target - origin;
        let dist = to_target.length();
        if cfg.enabled && dist > 0.2 {
            // parts closer to the subject than this stay solid: they are the subject
            let keep = 0.2 * dist + 0.06;
            let limit = (dist - keep).max(0.05);
            let right = cam_tf.right().as_vec3();
            let up = cam_tf.up().as_vec3();
            let a = 0.05 * dist + 0.025;
            let mut aims = vec![target];
            for k in 0..8 {
                let ang = std::f32::consts::TAU * k as f32 / 8.0;
                aims.push(target + (right * ang.cos() + up * ang.sin()) * a);
            }
            let filter = |e: Entity| fadable.contains(e);
            let settings = MeshRayCastSettings::default()
                .with_filter(&filter)
                .with_visibility(RayCastVisibility::Visible)
                .never_early_exit();
            for p in aims {
                let Ok(dir) = Dir3::new(p - origin) else { continue };
                for (e, hit) in ray.cast_ray(Ray3d::new(origin, dir), &settings) {
                    if hit.distance < limit {
                        hit_set.insert(*e);
                    }
                }
            }
        }
        // a part rarely fades alone: the radiator is a core, tanks, rails and a shroud. Pull in the
        // small parts touching whatever was hit, as long as they are also in front of the subject.
        if !hit_set.is_empty() {
            let world = |gt: &GlobalTransform, a: &Aabb| -> (Vec3, Vec3) {
                let m = Mat3::from(gt.affine().matrix3);
                let abs = Mat3::from_cols(m.x_axis.abs(), m.y_axis.abs(), m.z_axis.abs());
                (gt.transform_point(Vec3::from(a.center)), abs * Vec3::from(a.half_extents))
            };
            let seeds: Vec<(Vec3, Vec3)> = hit_set
                .iter()
                .filter_map(|e| boxes.get(*e).ok())
                .map(|(_, gt, a)| world(gt, a))
                .filter(|(_, h)| h.length() < 0.35)
                .collect();
            let limit = (dist - (0.2 * dist + 0.06)).max(0.05);
            for (e, gt, a) in &boxes {
                if hit_set.contains(&e) {
                    continue;
                }
                let (c, h) = world(gt, a);
                if h.length() > 0.45 || (c - origin).length() - h.length() > limit {
                    continue;
                }
                let touches = seeds.iter().any(|(sc, sh)| {
                    let d = (c - *sc).abs();
                    let reach = *sh + h + Vec3::splat(0.012);
                    d.x <= reach.x && d.y <= reach.y && d.z <= reach.z
                });
                if touches {
                    extra.push(e);
                }
            }
            hit_set.extend(extra.drain(..));
        }
        *occluders = hit_set;
    }

    // ---- give new occluders a translucent copy of their material
    let newcomers: Vec<Entity> = occluders.iter().copied().filter(|e| !active.contains(e)).collect();
    for e in newcomers {
        let Ok((mut mat, _)) = parts.get_mut(e) else { continue };
        let Some(orig) = materials.get(&mat.0).cloned() else { continue };
        if orig.alpha_mode != AlphaMode::Opaque {
            commands.entity(e).insert(NoFade);
            continue;
        }
        // a ghost is faint diffuse only: no mirror highlights to keep it looking solid
        let ghost = materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            metallic: 0.0,
            perceptual_roughness: 0.85,
            reflectance: 0.04,
            clearcoat: 0.0,
            ..orig
        });
        commands.entity(e).insert(Fade { orig: mat.0.clone(), ghost: ghost.clone(), alpha: 1.0 });
        mat.0 = ghost;
        active.insert(e);
    }

    // ---- fade everything that is currently ghosted
    let dt = time.delta_secs().min(0.1);
    let mut done: Vec<Entity> = Vec::new();
    for &e in active.iter() {
        let Ok((mut mat, fade)) = parts.get_mut(e) else {
            done.push(e);
            continue;
        };
        let Some(mut fade) = fade else {
            // the Fade component is still being applied this frame
            continue;
        };
        let blocked = occluders.contains(&e);
        let (target, rate) = if blocked { (GHOST_ALPHA, FADE_OUT_RATE) } else { (1.0, FADE_IN_RATE) };
        fade.alpha += (target - fade.alpha) * (1.0 - (-rate * dt).exp());
        if !blocked && fade.alpha > 0.985 {
            mat.0 = fade.orig.clone();
            commands.entity(e).remove::<Fade>();
            done.push(e);
            continue;
        }
        // follow the original's animated bits (heat glow, highlights) while ghosted
        if let Some(o) = materials.get(&fade.orig).map(|o| (o.base_color, o.emissive)) {
            if let Some(mut g) = materials.get_mut(&fade.ghost) {
                let c = o.0.to_linear();
                g.base_color = Color::linear_rgba(c.red, c.green, c.blue, c.alpha * fade.alpha);
                g.emissive = o.1 * fade.alpha;
            }
        }
    }
    for e in done {
        active.remove(&e);
    }
}
