//! The 3D powertrain rig: built procedurally from the selected engine's real dimensions, then
//! animated every frame from the sim (see `anim`).

pub mod accessories;
pub mod anim;
pub mod driveline;
pub mod engine_build;
pub mod parts;
pub mod plumbing;
pub mod tags;
pub mod turbo;

use crate::look::Mats;
use crate::meshgen::MB;
use bevy::prelude::*;

#[derive(Component)]
pub struct RigRoot;

/// Spawn helper bundling the asset stores.
pub struct Ctx<'a, 'w, 's> {
    pub cmd: &'a mut Commands<'w, 's>,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub mats: &'a Mats,
}

impl<'a, 'w, 's> Ctx<'a, 'w, 's> {
    pub fn mesh(&mut self, mb: MB) -> Handle<Mesh> {
        self.meshes.add(mb.into_mesh())
    }

    /// Empty transform node.
    pub fn node(&mut self, parent: Entity, t: Transform) -> Entity {
        self.cmd.spawn((t, Visibility::default(), ChildOf(parent))).id()
    }

    pub fn put(&mut self, parent: Entity, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, t: Transform) -> Entity {
        self.cmd.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), t, ChildOf(parent), crate::seethrough::Fadable)).id()
    }

    /// Build a mesh and place it in one go.
    pub fn spawn(&mut self, parent: Entity, mb: MB, mat: &Handle<StandardMaterial>, t: Transform) -> Entity {
        let h = self.mesh(mb);
        self.put(parent, &h, mat, t)
    }

    /// A one-off material (for parts whose emissive/colour is animated per-entity).
    pub fn material(&mut self, m: StandardMaterial) -> Handle<StandardMaterial> {
        self.materials.add(m)
    }

    /// An invisible, nameable point on the rig (see `labels.rs`).
    pub fn tag(&mut self, parent: Entity, local: Vec3, key: &'static str, ci: Option<usize>) {
        self.cmd.spawn((Transform::from_translation(local), Visibility::default(), ChildOf(parent), crate::labels::PartTag { key, ci }));
    }
}

/// Cubic Bezier sample, `n` segments.
pub fn bezier(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, n: usize) -> Vec<Vec3> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        })
        .collect()
}

/// Catmull-Rom spline through the given points, `per` samples between each pair.
pub fn spline(pts: &[Vec3], per: usize) -> Vec<Vec3> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..n - 1 {
        let p0 = if i == 0 { pts[0] * 2.0 - pts[1] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 >= n { pts[n - 1] * 2.0 - pts[n - 2] } else { pts[i + 2] };
        for s in 0..per {
            let t = s as f32 / per as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            out.push(
                0.5 * ((2.0 * p1)
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3),
            );
        }
    }
    out.push(pts[n - 1]);
    out
}

use crate::cam::{Anchor, Anchors, CamRig};
use crate::layout::Layout;
use crate::sim::Sim;

/// Rebuilds the whole rig when the sim asks for it (engine/transmission swap).
#[allow(clippy::too_many_arguments)]
pub fn rig_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mats: Res<Mats>,
    mut sim: ResMut<Sim>,
    roots: Query<Entity, With<RigRoot>>,
    mut anchors: ResMut<Anchors>,
    mut cam: ResMut<CamRig>,
    puff_tex: Res<crate::puffs::PuffTex>,
) {
    if !sim.rebuild {
        return;
    }
    sim.rebuild = false;
    sim.built_key = Some(crate::sim::visual_key(&sim.tuning));
    for e in &roots {
        commands.entity(e).despawn();
    }
    let cfg = sim.cfg().clone();
    let lay = Layout::plan(&cfg);
    if std::env::var_os("RIG_LOG").is_some() {
        eprintln!("RIG {} cylinders x = {:?}, deck {:.3}, bore {:.3}, tilts {:?}", cfg.id, lay.cyls.iter().map(|c| (c.x * 1000.0).round() / 1000.0).collect::<Vec<_>>(), lay.deck, lay.bore, lay.tilts);
    }
    let root = commands.spawn((RigRoot, Transform::default(), Visibility::default())).id();
    let mut cx = Ctx { cmd: &mut commands, meshes: &mut meshes, materials: &mut materials, mats: &mats };
    let eb = engine_build::build_engine(&mut cx, root, &cfg, &lay);
    let trans = sim.transmissions[sim.trans_idx].clone();
    let d = driveline::dims(&lay, &trans);
    let tuning = sim.tuning.clone();
    let pl = plumbing::build_plumbing(&mut cx, root, &cfg, &lay, &eb, &d, &tuning);
    accessories::build_chassis(&mut cx, root, &lay, &d, pl.front_x);
    let dl = driveline::build_driveline(&mut cx, root, eb.crank, &cfg, &lay, &trans, &d);
    tags::add_tags(&mut cx, root, &cfg, &lay, &eb, &d, &pl, &trans, &tuning);
    let mut geom = eb.geom.clone();
    geom.is_turbo = pl.n_turbos > 0;
    commands.insert_resource(geom);
    commands.insert_resource(crate::fx::FlowPaths {
        intake: pl.intake_paths.iter().map(|p| crate::fx::PolyPath::new(p.clone())).collect(),
        exhaust: pl.exhaust_paths.iter().map(|p| crate::fx::PolyPath::new(p.clone())).collect(),
    });
    {
        let mut cx2 = Ctx { cmd: &mut commands, meshes: &mut meshes, materials: &mut materials, mats: &mats };
        crate::fx::build_fx(&mut cx2, root, &pl.intake_paths, &pl.exhaust_paths, &pl.tail_tips, pl.tail_dir, pl.tail_r, eb.x_mid, &puff_tex);
    }

    let len = lay.x_rear - lay.x_front;
    let mut list = vec![Anchor {
        name: "Engine",
        target: Vec3::new(eb.x_mid, 0.06, 0.0),
        dist: len * 1.25 + 0.55,
        yaw: 0.85,
        pitch: 0.32,
    }];
    if let Some(tc) = pl.turbo_center {
        let (name, dist) = if pl.n_turbos > 1 { ("Turbochargers", 1.35 + 0.12 * pl.n_turbos as f32) } else { ("Turbocharger", 1.0) };
        list.push(Anchor { name, target: tc + Vec3::new(0.0, 0.02, 0.05), dist, yaw: 0.9, pitch: 0.3 });
    }
    list.push(Anchor {
        name: "Belts & cooling",
        target: Vec3::new(lay.x_front - 0.07, 0.05, 0.0),
        dist: 0.95,
        yaw: -1.2,
        pitch: 0.36,
    });
    list.push(Anchor {
        name: "Intake",
        target: Vec3::new(eb.x_mid, 0.2, 0.0),
        dist: len * 1.0 + 0.5,
        yaw: -0.7,
        pitch: 0.5,
    });
    list.extend(dl.anchors);
    list.push(Anchor {
        name: "Exhaust",
        target: pl.muffler_center - Vec3::new(0.9, 0.0, 0.0),
        dist: 2.4,
        yaw: 1.7,
        pitch: 0.25,
    });
    list.push(Anchor {
        name: "Full powertrain",
        target: Vec3::new((lay.x_front + d.x_diff) * 0.5, -0.15, 0.0),
        dist: (d.x_diff - lay.x_front) * 1.15 + 1.0,
        yaw: 0.55,
        pitch: 0.28,
    });
    // keep the highlighted preset on the same view across a rebuild (the list can gain or lose entries)
    if let Some(old) = anchors.0.get(cam.preset).map(|a| a.name) {
        let key = old.trim_end_matches('s');
        if let Some(i) = list.iter().position(|a| a.name.trim_end_matches('s') == key) {
            cam.preset = i;
        } else if cam.preset >= list.len() {
            cam.preset = list.len() - 1;
        }
    }
    anchors.0 = list;
    let _ = pl.throttle_center;
    if cam.preset == usize::MAX {
        cam.preset = 0;
        let a = anchors.0[0].clone();
        cam.goto(&a);
        cam.target = cam.t_target;
        cam.yaw = cam.t_yaw;
        cam.pitch = cam.t_pitch;
        cam.dist = cam.t_dist;
    }
}
