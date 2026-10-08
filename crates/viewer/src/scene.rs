//! Camera, studio lighting, procedural HDR environment map, floor.

use crate::look::Mats;
use bevy::asset::RenderAssetUsages;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, GeneratedEnvironmentMapLight, NotShadowCaster};
use bevy::anti_alias::smaa::Smaa;
use bevy::post_process::bloom::Bloom;
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::post_process::motion_blur::MotionBlur;
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct FloorMark {
    pub x0: f32,
}

fn f32_to_f16(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32;
    let man = b & 0x7fffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7bff; // clamp to max finite
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = (man | 0x800000) >> (1 - e);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    sign | ((e as u16) << 10) | ((man + 0x1000) >> 13) as u16
}

/// Studio-style environment: dark room, large soft boxes (key, fill, rim) and a faint warm floor
/// bounce. This is what gives the metals something to reflect.
fn studio_radiance(d: Vec3) -> Vec3 {
    let d = d.normalize();
    let up = d.y.clamp(-1.0, 1.0);
    // vertical gradient
    let sky = Vec3::new(0.20, 0.235, 0.30) * (0.35 + 0.65 * (0.5 + 0.5 * up)).powf(1.4);
    let floor = Vec3::new(0.09, 0.08, 0.07) * (-up).max(0.0);
    // faint horizon band, like a cyclorama lit from below
    let horizon = Vec3::new(0.18, 0.2, 0.26) * (-(up * 5.0).powi(2)).exp();
    let mut c = sky + floor + horizon;
    // soft boxes: (direction, half angular size, colour * intensity)
    let boxes: [(Vec3, f32, Vec3); 6] = [
        (Vec3::new(-0.55, 0.78, 0.35), 0.55, Vec3::new(1.0, 0.96, 0.9) * 6.5), // big key
        (Vec3::new(0.85, 0.30, -0.45), 0.38, Vec3::new(0.7, 0.85, 1.0) * 3.8), // cool fill
        (Vec3::new(0.1, 0.35, -0.95), 0.30, Vec3::new(1.0, 0.8, 0.6) * 5.0),   // warm back rim
        (Vec3::new(0.0, 1.0, 0.0), 0.7, Vec3::new(1.0, 1.0, 1.0) * 2.6),       // overhead strip
        (Vec3::new(-0.9, 0.15, -0.3), 0.32, Vec3::new(0.9, 0.9, 1.0) * 2.4),
        (Vec3::new(0.75, 0.2, 0.6), 0.34, Vec3::new(0.45, 0.8, 1.0) * 2.8),    // cyan accent
    ];
    for (dir, size, col) in boxes {
        let dir = dir.normalize();
        let cosang = d.dot(dir).clamp(-1.0, 1.0);
        let ang = cosang.acos();
        let t = ((size - ang) / (size * 0.35)).clamp(0.0, 1.0);
        c += col * t * t * (3.0 - 2.0 * t);
    }
    c
}

pub fn make_environment(images: &mut Assets<Image>) -> Handle<Image> {
    let size = 128u32;
    let mut data: Vec<u8> = Vec::with_capacity((size * size * 6 * 8) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let dir = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                };
                let c = studio_radiance(dir);
                for ch in [c.x, c.y, c.z, 1.0] {
                    data.extend_from_slice(&f32_to_f16(ch).to_le_bytes());
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    images.add(img)
}

pub fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mats: Res<Mats>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let env = make_environment(&mut images);

    commands.spawn((
        MainCamera,
        Camera3d::default(),
        Hdr,
        Tonemapping::AcesFitted,
        Bloom { intensity: 0.2, ..Bloom::NATURAL },
        ScreenSpaceAmbientOcclusion { quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High, constant_object_thickness: 0.12 },
        Msaa::Off,
        Smaa::default(),
        MotionBlur { shutter_angle: 0.6, samples: 6, ..default() },
        DepthOfField {
            mode: DepthOfFieldMode::Bokeh,
            focal_distance: 1.5,
            aperture_f_stops: 4.5,
            max_depth: 40.0,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection { fov: 38f32.to_radians(), near: 0.02, far: 220.0, ..default() }),
        Transform::from_xyz(0.6, 0.5, 1.4).looking_at(Vec3::new(0.3, 0.0, 0.0), Vec3::Y),
        GeneratedEnvironmentMapLight { environment_map: env, intensity: 240.0, ..default() },
        DistanceFog {
            color: Color::srgb(0.045, 0.055, 0.075),
            falloff: FogFalloff::Exponential { density: 0.016 },
            ..default()
        },
    ));

    // key light with shadows, tuned for a ~4m long scene
    commands.spawn((
        DirectionalLight { illuminance: 6_500.0, shadow_maps_enabled: true, contact_shadows_enabled: true, color: Color::srgb(1.0, 0.96, 0.9), ..default() },
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            minimum_distance: 0.05,
            maximum_distance: 14.0,
            first_cascade_far_bound: 0.8,
            overlap_proportion: 0.25,
        }
        .build(),
        Transform::from_xyz(-3.0, 6.0, 2.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 1_600.0, shadow_maps_enabled: false, color: Color::srgb(0.65, 0.78, 1.0), ..default() },
        Transform::from_xyz(4.0, 2.5, -3.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // cool overhead pool of light on the floor around the car, and a warm kicker from behind
    commands.spawn((
        SpotLight {
            intensity: 40_000.0,
            range: 14.0,
            radius: 1.6,
            color: Color::srgb(0.85, 0.92, 1.0),
            shadow_maps_enabled: false,
            inner_angle: 0.55,
            outer_angle: 0.95,
            ..default()
        },
        Transform::from_xyz(1.2, 4.2, 0.8).looking_at(Vec3::new(1.2, -0.5, 0.0), Vec3::X),
    ));
    commands.spawn((
        PointLight { intensity: 160_000.0, range: 9.0, color: Color::srgb(1.0, 0.55, 0.25), shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-2.2, 0.15, -1.6),
    ));
    commands.spawn((
        PointLight { intensity: 120_000.0, range: 9.0, color: Color::srgb(0.25, 0.7, 1.0), shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(3.5, 0.4, 2.4),
    ));

    // cyclorama backdrop: a big inside-out dome, dark at the zenith, a cool glow at the horizon
    commands.spawn((
        Mesh3d(meshes.add(backdrop_mesh(100.0))),
        MeshMaterial3d(backdrop_mat(&mut materials)),
        Transform::from_xyz(0.0, -0.56, 0.0),
        NotShadowCaster,
    ));

    // floor
    let floor_y = -0.56;
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(100.0))),
        MeshMaterial3d(mats.floor.clone()),
        Transform::from_xyz(0.0, floor_y, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));
    // moving lane marks: pure motion cue, scrolled by vehicle speed
    let mark = meshes.add(Cuboid::new(1.6, 0.004, 0.05));
    for side in [-1.0f32, 1.0] {
        for i in 0..24 {
            commands.spawn((
                FloorMark { x0: i as f32 * 2.5 - 20.0 },
                Mesh3d(mark.clone()),
                MeshMaterial3d(mats.floor_mark.clone()),
                Transform::from_xyz(i as f32 * 2.5 - 20.0, floor_y + 0.003, side * 1.9),
            ));
        }
    }
}

/// Inside-out sphere with a vertical colour ramp baked into the vertices.
fn backdrop_mesh(radius: f32) -> Mesh {
    let mut mesh = Mesh::from(Sphere::new(radius).mesh().uv(64, 32));
    let colors: Vec<[f32; 4]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) => p
            .iter()
            .map(|v| {
                let h = v[1] / radius;
                let top = Vec3::new(0.004, 0.005, 0.009);
                let mid = Vec3::new(0.025, 0.032, 0.05);
                let horizon = Vec3::new(0.085, 0.11, 0.15);
                let c = if h >= 0.0 {
                    let t = (h / 0.45).clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    let band = (-(h * 7.0).powi(2)).exp();
                    horizon.lerp(mid, t).lerp(top, (h * 1.4).clamp(0.0, 1.0)) + Vec3::new(0.04, 0.05, 0.07) * band
                } else {
                    Vec3::new(0.02, 0.022, 0.028)
                };
                [c.x, c.y, c.z, 1.0]
            })
            .collect(),
        _ => Vec::new(),
    };
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

fn backdrop_mat(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        fog_enabled: false,
        cull_mode: Some(bevy::render::render_resource::Face::Front),
        ..default()
    })
}
