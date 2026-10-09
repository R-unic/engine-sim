//! The gas moving through the intake and the exhaust, seen through the translucent runners and manifolds.
//!
//! Parcels of gas follow the pipes' own centre lines: fresh charge (cool, a little blue, carrying the fuel mist of the
//! injectors) into every cylinder while its intake valve is open, and burned gas out of it at the exhaust valve's
//! opening, glowing when it is hot and fading to grey as the pipe cools it. A parcel is a soft Gaussian blob (a
//! volume seen through a lens) stretched along its flow and stirred sideways by swirl that grows downstream;
//! every parcel of every cylinder is one quad of a single dynamic mesh, drawn in one call.

use crate::fx::{hash01, FlowPaths};
use crate::rig::anim::{local_angle, valve_open_amount, RigGeom};
use crate::scene::MainCamera;
use crate::sim::Sim;
use bevy::asset::embedded_asset;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

pub struct GasPlugin;

impl Plugin for GasPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "gas.wgsl");
        app.add_plugins(MaterialPlugin::<GasMaterial>::default()).add_systems(Startup, setup_gas);
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct GasMaterial {}

impl Material for GasMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://viewer/gas.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Parcel {
    ci: usize,
    intake: bool,
    s: f32,
    seed: f32,
    alive: bool,
    /// How hot it left the valve, K (exhaust), and how fast it flows, m/s.
    t0: f32,
    v: f32,
}

#[derive(Resource)]
pub struct GasField {
    mesh: Handle<Mesh>,
    parcels: Vec<Parcel>,
    cylinders: usize,
}

const INTAKE_PER_CYL: usize = 8;
const EXHAUST_PER_CYL: usize = 12;

fn setup_gas(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<GasMaterial>>) {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
    // one parcel that is not there, so that the first upload is not an empty buffer
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, -50.0, 0.0]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32, 1.0]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; 4]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    let handle = meshes.add(mesh);
    commands.spawn((Mesh3d(handle.clone()), MeshMaterial3d(mats.add(GasMaterial {})), Transform::default(), Visibility::Visible, NoFrustumCulling, bevy::light::NotShadowCaster));
    commands.insert_resource(GasField { mesh: handle, parcels: Vec::new(), cylinders: 0 });
}

fn blackbody_glow(t: f32) -> Vec3 {
    // dull red at 800 K through orange to yellow at 1500 K (normalised colour; the brightness is set by the caller)
    let k = ((t - 800.0) / 800.0).clamp(0.0, 1.0);
    Vec3::new(1.0, 0.18 + 0.62 * k, 0.03 + 0.4 * k * k)
}

#[allow(clippy::too_many_arguments)]
pub fn gas_system(
    sim: Res<Sim>,
    g: Option<Res<RigGeom>>,
    paths: Option<Res<FlowPaths>>,
    mut field: ResMut<GasField>,
    mut meshes: ResMut<Assets<Mesh>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    time: Res<Time>,
) {
    let (Some(g), Some(paths)) = (g, paths) else { return };
    if std::env::var_os("NO_GAS").is_some() {
        return;
    }
    let n = g.lay.n();
    if sim.tables.len() != n || paths.intake.len() != n || paths.exhaust.len() != n {
        return;
    }
    if field.cylinders != n || paths.is_changed() {
        field.cylinders = n;
        field.parcels.clear();
        for ci in 0..n {
            for k in 0..INTAKE_PER_CYL {
                field.parcels.push(Parcel { ci, intake: true, s: 1e9, seed: ((ci * 31 + k * 17) % 97) as f32 / 97.0, alive: false, t0: 0.0, v: 0.0 });
            }
            for k in 0..EXHAUST_PER_CYL {
                field.parcels.push(Parcel { ci, intake: false, s: 1e9, seed: ((ci * 13 + k * 29) % 89) as f32 / 89.0, alive: false, t0: 0.0, v: 0.0 });
            }
        }
    }
    let dt = sim.vis_dt;
    let tel = &sim.tel;
    let load = (tel.throttle as f32).max(0.0);
    let map_ratio = (tel.manifold_pressure_pa / tel.atmospheric_pressure_pa.max(1.0)) as f32;
    let egt = tel.exhaust_gas_temp_k as f32;
    let t_now = time.elapsed_secs();
    let cam_rot = cam.iter().next().map(|c| c.compute_transform().rotation).unwrap_or(Quat::IDENTITY);
    let (right, up, back) = (cam_rot * Vec3::X, cam_rot * Vec3::Y, cam_rot * Vec3::Z);

    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(field.parcels.len() * 4);
    let mut uv0: Vec<[f32; 2]> = Vec::with_capacity(field.parcels.len() * 4);
    let mut uv1: Vec<[f32; 2]> = Vec::with_capacity(field.parcels.len() * 4);
    let mut col: Vec<[f32; 4]> = Vec::with_capacity(field.parcels.len() * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(field.parcels.len() * 6);

    for p in field.parcels.iter_mut() {
        let ci = p.ci;
        let th = local_angle(&sim, ci);
        let open = valve_open_amount(&g, th, p.intake);
        let path = if p.intake { &paths.intake[ci] } else { &paths.exhaust[ci] };
        let len = path.len();
        let pr = sim.tables[ci].pressure_at(490.0);
        let k = ((pr - 1.2e5) / 5.0e5).clamp(0.1, 1.6);
        if p.alive {
            p.s += p.v * dt;
            if p.s > len {
                p.alive = false;
            }
        }
        if !p.alive && tel.is_running {
            p.seed = (p.seed * 13.37 + 0.618).fract();
            if p.seed < open * 0.4 {
                p.alive = true;
                p.s = 0.0;
                // the speed of the flow while this parcel was set off
                p.v = if p.intake { 2.2 + 5.5 * (0.15 + 0.85 * load) * map_ratio.clamp(0.2, 2.0) } else { 3.0 + 9.0 * k };
                p.t0 = egt * (0.85 + 0.25 * k);
            }
        }
        if !p.alive || len < 0.02 {
            // the mesh keeps its size from frame to frame (a resized one is reallocated by the renderer): a parcel that
            // is not there is a degenerate quad
            let base = pos.len() as u32;
            for _ in 0..4 {
                pos.push([0.0, -50.0, 0.0]);
                uv0.push([0.0, 0.0]);
                uv1.push([0.0, 1.0]);
                col.push([0.0; 4]);
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            continue;
        }
        let life = (p.s / len.max(0.01)).clamp(0.0, 1.0);
        let c = path.at(p.s);
        let tan = (path.at((p.s + 0.02).min(len)) - path.at((p.s - 0.02).max(0.0))).normalize_or_zero();
        // swirl across the pipe that grows as the parcel goes downstream
        let a = Vec3::Y.cross(tan).normalize_or_zero();
        let b = tan.cross(a);
        let ph = p.seed * 40.0 + t_now * 6.0 * (0.4 + p.v * 0.1);
        let off = (a * ph.sin() + b * (ph * 1.3 + 1.7).cos()) * (0.002 + 0.006 * life) * (0.5 + hash01(p.seed * 7.0));
        let centre = c + off;
        // size, colour, absorption
        let (sigma, rgb, absorb) = if p.intake {
            let sig = 0.007 + 0.008 * life + 0.003 * load;
            // cool charge: pale blue; a little warmer (and more yellow with the fuel mist) as it nears the valve
            let strength = 0.5 + 0.9 * (0.2 + 0.8 * load) * map_ratio.clamp(0.2, 1.5);
            (sig, Vec3::new(0.40, 0.72, 1.0) * strength + Vec3::new(0.30, 0.18, 0.0) * life * 0.4, 0.05)
        } else {
            let tg = T_AMB + (p.t0 - T_AMB) * (-p.s / 2.6).exp();
            let glow = ((tg - 750.0) / 600.0).clamp(0.0, 1.0);
            let sig = 0.008 + 0.012 * life + 0.006 * k;
            // hot burned gas glows red-orange; once it has cooled it is a grey haze
            let rgb = blackbody_glow(tg) * glow * glow * (1.2 + 1.6 * k) + Vec3::new(0.30, 0.32, 0.36) * (1.0 - glow) * 0.22;
            (sig, rgb, 0.10 + 0.20 * (1.0 - glow))
        };
        let stretch = 1.0 + (p.v * 0.045).min(1.8);
        // the long axis follows the flow as the camera sees it
        let ts = Vec2::new(tan.dot(right), tan.dot(up));
        let ang = if ts.length() > 0.05 { ts.y.atan2(ts.x) } else { 0.0 };
        let (sn, cs) = ang.sin_cos();
        let half = 3.0 * sigma;
        let ax = (right * cs + up * sn) * half * stretch;
        let ay = (-right * sn + up * cs) * half;
        let base = pos.len() as u32;
        for (u, v) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let q = centre + ax * u + ay * v;
            pos.push(q.to_array());
            uv0.push([u, v]);
            uv1.push([p.seed, life]);
            col.push([rgb.x, rgb.y, rgb.z, absorb]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let nrm: Vec<[f32; 3]> = vec![back.to_array(); pos.len()];
    if let Some(mut mesh) = meshes.get_mut(&field.mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv0);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
        mesh.insert_indices(Indices::U32(idx));
    }
}

const T_AMB: f32 = 293.0;
