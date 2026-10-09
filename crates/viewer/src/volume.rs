//! Volumetric smoke, steam, exhaust haze and flame at the tailpipe.
//!
//! A few hundred *blobs* carry the physics: each is a Gaussian puff of haze, soot and burning gas that is thrown
//! out of the pipe, slowed by drag against the air (and the car's own airflow), lifted by its heat, stirred by a
//! divergence-free turbulent velocity field, grown by entrainment and cooled by mixing. Every frame the blobs are
//! splatted into a 3D texture (mass-conserving Gaussian kernels) and a ray marcher (`plume.wgsl`) integrates that
//! field along each camera ray: the haze scatters the key and fill lights and shadows itself, soot absorbs, and
//! flames emit with the colour of a black body at their temperature and light the smoke around them. Fine
//! turbulent structure the grid cannot hold is added in the shader from moving noise.
//!
//! The blobs live in rig-root space and are aged in *fx time* (real time x the sim's time scale), so slowing the
//! sim down stretches every puff and flame out to be looked at.

use crate::scene::{f32_to_f16, FLOOR_Y};
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, Extent3d, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;

/// The grid the blobs are splatted into: 2.5 cm cells, 2.4 m along the exhaust, 0.9 m up, 1.2 m across.
const NX: usize = 96;
const NY: usize = 36;
const NZ: usize = 48;
const VOX: f32 = 0.025;
/// Behind the pipe's mouth the domain starts this far (m), so a puff leaving the pipe is inside it.
const BACK: f32 = 0.2;
/// Cap on live blobs.
const MAX_BLOBS: usize = 2600;
const T_AMB: f32 = 293.0;

/// Developer hook for looking at the flames and smoke in a screenshot: `FX_SLOW=0.05` runs the fx clock (and nothing
/// else) at that fraction of real time.
pub fn fx_slow() -> f32 {
    static SLOW: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SLOW.get_or_init(|| std::env::var("FX_SLOW").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0))
}

pub struct VolumePlugin;

impl Plugin for VolumePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "plume.wgsl");
        app.add_plugins(MaterialPlugin::<PlumeMaterial>::default());
    }
}

#[derive(Clone, Copy, ShaderType, Default, Debug)]
pub struct PlumeUniform {
    pub box_min: Vec4,
    pub box_max: Vec4,
    pub tex_min: Vec4,
    pub tex_size: Vec4,
    pub sun_dir: Vec4,
    pub sun_col: Vec4,
    pub fill_dir: Vec4,
    pub fill_col: Vec4,
    pub amb: Vec4,
    pub misc: Vec4,
    pub wind: Vec4,
    pub flame_pos: Vec4,
    pub haze_tint: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct PlumeMaterial {
    #[uniform(0)]
    pub u: PlumeUniform,
    #[texture(1, dimension = "3d")]
    #[sampler(2)]
    pub field: Handle<Image>,
}

impl Material for PlumeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://viewer/plume.wgsl".into()
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
        // the far faces of the box start every ray (from inside or outside), and the depth test is the shader's own
        // business: a hardware test against the far faces would throw away the part of the volume in front of the floor
        descriptor.primitive.cull_mode = Some(Face::Front);
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_compare = Some(CompareFunction::Always);
            // the shader writes the depth of the thing it draws, for the depth-of-field pass
            ds.depth_write_enabled = Some(true);
        }
        Ok(())
    }
}

/// What `fx::build_fx` asks for; `setup_plume` builds it where the asset stores are at hand.
#[derive(Resource)]
pub struct PendingPlume {
    pub root: Entity,
    pub tips: Vec<Vec3>,
    pub axis: Vec3,
    pub radius: f32,
}

#[derive(Component)]
pub struct PlumeBox;

/// One Gaussian puff of the plume.
#[derive(Clone, Copy, Debug)]
pub struct Blob {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Radius now and the radius it grows toward (m).
    pub r: f32,
    pub r_max: f32,
    /// Optical mass of haze and soot (extinction x m^3) and of burning gas (emission x m^3).
    pub haze: f32,
    pub soot: f32,
    pub heat: f32,
    /// Flame temperature (K) and the gas temperature that lifts the blob.
    pub t_flame: f32,
    pub temp: f32,
    /// Burning: it lights `ignite` seconds after birth (a flame front travels through the gas), burns at full for about
    /// `tau_burn` and is gone soon after (the heat falls as exp(-(t/tau)^2): a flame is bright and then it is out);
    /// its temperature cools from `t_flame` with `tau_flame_t` once lit.
    pub tau_burn: f32,
    pub tau_flame_t: f32,
    pub ignite: f32,
    pub tau_cool: f32,
    /// Seconds a puff of haze takes to thin out of sight by evaporation, and its lifetime.
    pub tau_haze: f32,
    pub age: f32,
    pub life: f32,
    pub seed: f32,
    /// How strongly its heat lifts it (flames: a little, a hot exhaust puff: a good deal).
    pub lift: f32,
}

impl Blob {
    /// How much of its heat is being released now (0..1): nothing before it lights, then a quick rise, then the burn-out.
    pub fn burn(&self) -> f32 {
        if self.heat <= 0.0 || self.age < self.ignite {
            return 0.0;
        }
        let tb = self.age - self.ignite;
        let rise = (tb / 0.02).min(1.0);
        (-(tb / self.tau_burn).powi(2)).exp() * rise * rise * (3.0 - 2.0 * rise)
    }

    /// Flame temperature now, K.
    pub fn flame_temp(&self) -> f32 {
        let tb = (self.age - self.ignite).max(0.0);
        T_AMB + (self.t_flame - T_AMB) * (-tb / self.tau_flame_t).exp()
    }

    /// Soot in it now: what it was born with and a share of the carbon of the fuel that has burnt.
    pub fn soot_now(&self) -> f32 {
        if self.heat <= 0.0 || self.age < self.ignite {
            return self.soot;
        }
        let tb = self.age - self.ignite;
        self.soot + 0.35 * self.heat * (1.0 - (-(tb / self.tau_burn).powi(2)).exp())
    }

    pub fn new(pos: Vec3, vel: Vec3, r: f32) -> Blob {
        Blob {
            pos,
            vel,
            r,
            r_max: 0.4,
            haze: 0.0,
            soot: 0.0,
            heat: 0.0,
            t_flame: 0.0,
            temp: T_AMB,
            tau_burn: 0.1,
            tau_flame_t: 0.1,
            ignite: 0.0,
            tau_cool: 0.5,
            tau_haze: 2.2,
            age: 0.0,
            life: 2.4,
            seed: 0.0,
            lift: 0.28,
        }
    }
}

#[derive(Resource)]
pub struct Plume {
    pub image: Handle<Image>,
    pub material: Handle<PlumeMaterial>,
    pub entity: Entity,
    pub origin: Vec3,
    pub tips: Vec<Vec3>,
    pub blobs: Vec<Blob>,
    /// (haze, soot, emission, emission x temperature) per cell, accumulated every frame.
    grid: Vec<[f32; 4]>,
    /// Cells touched last frame: [min, max) per axis; they are cleared first, and rewritten in the texture.
    dirty: Option<([usize; 3], [usize; 3])>,
    pub time: f32,
    /// Emission-weighted centre of the flames and their total emission (for the tailpipe light), this frame.
    pub flame_centre: Vec3,
    pub flame_power: f32,
    rng: u32,
}

impl Plume {
    pub fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.rand()
    }

    pub fn unit(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, std::f32::consts::TAU);
        let s = (1.0 - z * z).sqrt();
        Vec3::new(s * a.cos(), z, s * a.sin())
    }

    pub fn spawn(&mut self, mut b: Blob) {
        if self.blobs.len() >= MAX_BLOBS {
            // the oldest thinnest one makes room
            self.blobs.remove(0);
        }
        b.seed = self.rand();
        self.blobs.push(b);
    }

    /// Advances every blob by `dt` seconds (fx time).
    pub fn step(&mut self, dt: f32, wind: Vec3) {
        self.time += dt;
        if dt <= 0.0 {
            return;
        }
        let t = self.time;
        let origin = self.origin;
        let top = origin.y + NY as f32 * VOX;
        let (zlo, zhi) = (origin.z, origin.z + NZ as f32 * VOX);
        self.blobs.retain_mut(|b| {
            b.age += dt;
            if b.age >= b.life {
                return false;
            }
            // drag against the air that is moving past (the car's own airflow): a blob of radius r loses speed over
            // about r / |dv|; a puff that has grown big follows the air
            let rel = b.vel - wind;
            let speed = rel.length();
            let rate = 1.1 * speed / (b.r + 0.045) + 0.25;
            b.vel = wind + rel * (-rate * dt).exp();
            // heat lifts it, mixing cools it
            b.temp = T_AMB + (b.temp - T_AMB) * (-dt / b.tau_cool).exp();
            b.vel.y += 9.81 * ((b.temp - T_AMB) / T_AMB) * b.lift * dt;
            // turbulent stirring, stronger the faster it moves relative to the air and the bigger the eddies
            let sw = swirl(b.pos * 1.0, t) * (0.12 + 0.30 * speed.min(30.0) / 30.0 * 1.4) * (1.0 + 4.0 * b.r).min(2.0);
            let step = (b.vel + sw) * dt;
            b.pos += step;
            // entrainment of air: spreading angle of a turbulent jet, plus a little diffusion
            b.r = (b.r + (0.085 * speed + 0.018) * dt).min(b.r_max);
            // haze thins as it mixes with dry air: steam re-evaporates
            let fade = (-dt / b.tau_haze).exp();
            b.haze *= fade;
            b.soot *= (-dt / (b.tau_haze * 1.8)).exp();
            // the road: a puff that meets it slides along it
            let floor = FLOOR_Y + 0.35 * b.r;
            if b.pos.y < floor {
                b.pos.y = floor;
                b.vel.y = b.vel.y.max(0.0) * 0.15;
                b.vel.x *= 0.985;
            }
            // the ends of the domain: let it go
            !(b.pos.y > top || b.pos.z < zlo || b.pos.z > zhi || b.pos.x > origin.x + NX as f32 * VOX)
        });
    }

    /// Splats the blobs into the grid and writes the texture; returns the box (world) around what is alive, if anything.
    pub fn splat(&mut self, images: &mut Assets<Image>) -> Option<(Vec3, Vec3)> {
        // clear what was written last frame (the texture is rewritten there too)
        let prev = self.dirty.take();
        if let Some((lo, hi)) = prev {
            for z in lo[2]..hi[2] {
                for y in lo[1]..hi[1] {
                    let row = (z * NY + y) * NX;
                    for c in &mut self.grid[row + lo[0]..row + hi[0]] {
                        *c = [0.0; 4];
                    }
                }
            }
        }
        let (mut lo, mut hi) = ([usize::MAX; 3], [0usize; 3]);
        let (mut bmin, mut bmax) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let (mut centre, mut power) = (Vec3::ZERO, 0.0f32);
        let origin = self.origin;
        let mut wx = [0.0f32; 24];
        let mut wy = [0.0f32; 24];
        let mut wz = [0.0f32; 24];
        for b in &self.blobs {
            let burn = b.burn();
            let (haze, soot, heat) = (b.haze, b.soot_now(), b.heat * burn);
            if haze + soot + heat < 1.0e-7 {
                continue;
            }
            let t_now = if heat > 0.0 { b.flame_temp() } else { 0.0 };
            let sg = (0.5 * b.r).max(0.6 * VOX);
            // a blob moving fast through the air is smeared along its path: flames and jets are streaks, not balls
            let speed = b.vel.length();
            let stretch = if heat > 0.0 { 1.0 + 0.07 * speed } else { 1.0 + 0.035 * speed }.min(if heat > 0.0 { 4.0 } else { 2.6 });
            let dir = if speed > 1.0e-3 { b.vel / speed } else { Vec3::X };
            // the kernel is a Gaussian that is eased to nothing between 2.2 and 3.2 sigma: a plain cut anywhere short of
            // that leaves a visible edge (a box of flame) wherever the blob is bright
            let reach = 3.2 * sg;
            let ext = Vec3::new(
                reach * (1.0 + (stretch * stretch - 1.0) * dir.x * dir.x).sqrt(),
                reach * (1.0 + (stretch * stretch - 1.0) * dir.y * dir.y).sqrt(),
                reach * (1.0 + (stretch * stretch - 1.0) * dir.z * dir.z).sqrt(),
            );
            bmin = bmin.min(b.pos - ext);
            bmax = bmax.max(b.pos + ext);
            power += heat;
            centre += b.pos * heat;
            let inv_vol = 1.0 / (VOX * VOX * VOX);
            let (mh, ms, me) = (haze * inv_vol, soot * inv_vol, heat * inv_vol);
            let met = me * t_now;
            let c = (b.pos - origin) / VOX - Vec3::splat(0.5);
            let rad = (ext / VOX).ceil().as_ivec3().clamp(IVec3::ONE, IVec3::splat(12));
            let i0 = c.floor().as_ivec3() - rad + IVec3::ONE;
            let n = IVec3::splat(2) * rad;
            let (xlo, xhi) = (i0.x.max(0), (i0.x + n.x).min(NX as i32));
            let (ylo, yhi) = (i0.y.max(0), (i0.y + n.y).min(NY as i32));
            let (zlo, zhi) = (i0.z.max(0), (i0.z + n.z).min(NZ as i32));
            if xlo >= xhi || ylo >= yhi || zlo >= zhi {
                continue;
            }
            if stretch < 1.06 {
                // 1-D kernels along each axis (cell centres), normalised: the product conserves the mass of the blob
                let kern = |w: &mut [f32; 24], c: f32, i0: i32, n: usize| {
                    let mut s = 0.0;
                    for k in 0..n {
                        let d = ((i0 + k as i32) as f32 - c) * VOX / sg;
                        let v = (-0.5 * d * d).exp() * taper(d.abs());
                        w[k] = v;
                        s += v;
                    }
                    for k in 0..n {
                        w[k] /= s;
                    }
                };
                kern(&mut wx, c.x, i0.x, n.x as usize);
                kern(&mut wy, c.y, i0.y, n.y as usize);
                kern(&mut wz, c.z, i0.z, n.z as usize);
                for z in zlo..zhi {
                    let kz = (z - i0.z) as usize;
                    for y in ylo..yhi {
                        let wyz = wy[(y - i0.y) as usize] * wz[kz];
                        let row = (z as usize * NY + y as usize) * NX;
                        for x in xlo..xhi {
                            let w = wyz * wx[(x - i0.x) as usize];
                            let cell = &mut self.grid[row + x as usize];
                            cell[0] += mh * w;
                            cell[1] += ms * w;
                            cell[2] += me * w;
                            cell[3] += met * w;
                        }
                    }
                }
            } else {
                // an anisotropic Gaussian: wider by `stretch` along the direction of travel
                let inv2 = 1.0 / (2.0 * sg * sg);
                let ist2 = 1.0 / (stretch * stretch);
                let mut sum = 0.0;
                let mut cells: Vec<(usize, f32)> = Vec::with_capacity(((xhi - xlo) * (yhi - ylo) * (zhi - zlo)) as usize);
                for z in zlo..zhi {
                    for y in ylo..yhi {
                        let row = (z as usize * NY + y as usize) * NX;
                        for x in xlo..xhi {
                            let d = (Vec3::new(x as f32, y as f32, z as f32) - c) * VOX;
                            let along = d.dot(dir);
                            let q = along * along * ist2 + (d.length_squared() - along * along);
                            let w = (-q * inv2).exp() * taper((q * inv2 * 2.0).sqrt());
                            if w > 1.0e-5 {
                                cells.push((row + x as usize, w));
                                sum += w;
                            }
                        }
                    }
                }
                let norm = 1.0 / sum.max(1.0e-9);
                for (i, w) in cells {
                    let w = w * norm;
                    let cell = &mut self.grid[i];
                    cell[0] += mh * w;
                    cell[1] += ms * w;
                    cell[2] += me * w;
                    cell[3] += met * w;
                }
            }
            lo = [lo[0].min(xlo as usize), lo[1].min(ylo as usize), lo[2].min(zlo as usize)];
            hi = [hi[0].max(xhi as usize), hi[1].max(yhi as usize), hi[2].max(zhi as usize)];
        }
        self.flame_power = power;
        self.flame_centre = if power > 1.0e-6 { centre / power } else { self.tips.first().copied().unwrap_or(Vec3::ZERO) };
        let touched = lo[0] != usize::MAX;
        let cur = if touched { Some((lo, hi)) } else { None };
        self.dirty = cur;
        let Some(mut img) = images.get_mut(&self.image) else { return None };
        let Some(data) = img.data.as_mut() else { return None };
        // the cells to write: what was touched now and what was there last frame
        let (wlo, whi) = match (prev, cur) {
            (Some((a, b)), Some((c, d))) => ([a[0].min(c[0]), a[1].min(c[1]), a[2].min(c[2])], [b[0].max(d[0]), b[1].max(d[1]), b[2].max(d[2])]),
            (Some(r), None) | (None, Some(r)) => r,
            (None, None) => return None,
        };
        for z in wlo[2]..whi[2] {
            for y in wlo[1]..whi[1] {
                let row = (z * NY + y) * NX;
                for x in wlo[0]..whi[0] {
                    let c = self.grid[row + x];
                    let t = if c[2] > 1.0e-6 { (c[3] / c[2]) / 2500.0 } else { 0.0 };
                    let o = (row + x) * 8;
                    for (k, v) in [c[0], c[1], c[2], t].iter().enumerate() {
                        data[o + 2 * k..o + 2 * k + 2].copy_from_slice(&f32_to_f16(*v).to_le_bytes());
                    }
                }
            }
        }
        if touched {
            Some((bmin, bmax))
        } else {
            None
        }
    }
}

/// 1 up to 2.2 sigma, then eased to 0 at 3.2 sigma.
#[inline]
fn taper(u: f32) -> f32 {
    let t = ((u - 2.2) / 1.0).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// Divergence-free swirl (the curl of a vector potential made of two octaves of sines): eddies that carry the blobs
/// around each other without piling them up. Unit magnitude order.
pub fn swirl(p: Vec3, t: f32) -> Vec3 {
    fn octave(p: Vec3, t: f32, a: f32, b: f32, ph: f32) -> Vec3 {
        let (th1, th2, th3) = (1.3 * t + ph, 0.9 * t + 2.0 * ph, 1.7 * t + 3.0 * ph);
        let (ph1, ph2, ph3) = (0.7 * t + 1.0, 1.1 * t + 2.0, 0.5 * t + 3.0);
        let dyz = (a * p.x + th3).sin() * (-b * (b * p.y + ph3).sin()) - a * (a * p.z + th2).cos() * (b * p.x + ph2).cos();
        let dzx = (a * p.y + th1).sin() * (-b * (b * p.z + ph1).sin()) - a * (a * p.x + th3).cos() * (b * p.y + ph3).cos();
        let dxy = (a * p.z + th2).sin() * (-b * (b * p.x + ph2).sin()) - a * (a * p.y + th1).cos() * (b * p.z + ph1).cos();
        Vec3::new(dyz, dzx, dxy) / (a + b)
    }
    octave(p, t, 4.5, 7.3, 0.0) + 0.45 * octave(p, t * 1.4, 11.0, 17.0, 1.7)
}

/// Builds the plume once `fx::build_fx` has asked for it.
pub fn setup_plume(
    mut commands: Commands,
    pending: Option<Res<PendingPlume>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<PlumeMaterial>>,
) {
    let Some(p) = pending else { return };
    let tip = p.tips.first().copied().unwrap_or(Vec3::ZERO);
    let origin = Vec3::new(tip.x - BACK, FLOOR_Y, tip.z - 0.5 * NZ as f32 * VOX);
    if std::env::var_os("PLUME_LOG").is_some() {
        eprintln!("PLUME tip {tip:?} axis {:?} radius {:.3} origin {origin:?}", p.axis, p.radius);
    }
    let mut img = Image::new_fill(
        Extent3d { width: NX as u32, height: NY as u32, depth_or_array_layers: NZ as u32 },
        TextureDimension::D3,
        &[0u8; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    let image = images.add(img);
    let size = Vec3::new(NX as f32, NY as f32, NZ as f32) * VOX;
    let u = PlumeUniform {
        tex_min: origin.extend(0.0),
        tex_size: size.extend(0.0),
        sun_dir: Vec4::new(-3.0, 6.0, 2.5, 0.0).normalize(),
        sun_col: Vec4::new(6.5, 6.2, 5.8, 0.0),
        fill_dir: Vec4::new(4.0, 2.5, -3.5, 0.0).normalize(),
        fill_col: Vec4::new(1.0, 1.25, 1.6, 0.0),
        amb: Vec4::new(0.55, 0.62, 0.78, 0.0),
        misc: Vec4::new(0.0, 56.0, 1.8, 0.9),
        haze_tint: Vec4::new(0.86, 0.9, 0.97, 0.0),
        ..default()
    };
    let material = mats.add(PlumeMaterial { u, field: image.clone() });
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let entity = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(origin + size * 0.5).with_scale(size),
            Visibility::Hidden,
            PlumeBox,
            bevy::light::NotShadowCaster,
            ChildOf(p.root),
        ))
        .id();
    commands.insert_resource(Plume {
        image,
        material,
        entity,
        origin,
        tips: p.tips.clone(),
        blobs: Vec::new(),
        grid: vec![[0.0; 4]; NX * NY * NZ],
        dirty: None,
        time: 0.0,
        flame_centre: tip,
        flame_power: 0.0,
        rng: 0x2F6B_91A7,
    });
    commands.remove_resource::<PendingPlume>();
}

/// Integrates the blobs, splats them and keeps the box and the uniforms current.
pub fn plume_system(
    sim: Res<crate::sim::Sim>,
    time: Res<Time>,
    plume: Option<ResMut<Plume>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<PlumeMaterial>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<PlumeBox>>,
) {
    let Some(mut plume) = plume else { return };
    if std::env::var_os("NO_PLUME").is_some() {
        if let Ok((_, mut vis)) = q.get_mut(plume.entity) {
            *vis = Visibility::Hidden;
        }
        return;
    }
    let real_dt = time.delta_secs();
    let dt = if sim.paused { 0.0 } else { real_dt * sim.time_scale * fx_slow() };
    // the air moves past the car at the car's speed; the wake behind it is slower
    let wind = Vec3::new((sim.tel.vehicle_speed_mps as f32 * 0.3).min(6.0), 0.0, 0.0);
    plume.step(dt, wind);
    let bounds = plume.splat(&mut images);
    if std::env::var_os("PLUME_LOG").is_some() && (time.elapsed_secs() * 2.0) as i64 != ((time.elapsed_secs() - real_dt) * 2.0) as i64 {
        let (h, so) = plume.blobs.iter().fold((0.0f32, 0.0f32), |a, b| (a.0 + b.haze, a.1 + b.soot));
        eprintln!("PLUME blobs {} haze {h:.4} soot {so:.4} wind {:.1} bounds {:?} t {:.2}", plume.blobs.len(), wind.x, bounds, plume.time);
    }
    let Ok((mut tf, mut vis)) = q.get_mut(plume.entity) else { return };
    let Some((lo, hi)) = bounds else {
        *vis = Visibility::Hidden;
        return;
    };
    let size_tex = Vec3::new(NX as f32, NY as f32, NZ as f32) * VOX;
    let (lo, hi) = if std::env::var_os("PLUME_FULLBOX").is_some() { (plume.origin, plume.origin + size_tex) } else { (lo, hi) };
    let lo = lo.max(plume.origin);
    let hi = hi.min(plume.origin + size_tex);
    if hi.x <= lo.x || hi.y <= lo.y || hi.z <= lo.z {
        *vis = Visibility::Hidden;
        return;
    }
    // the box the ray is clipped to is the mesh itself
    tf.translation = (lo + hi) * 0.5;
    tf.scale = hi - lo;
    *vis = Visibility::Inherited;
    if let Some(mut m) = mats.get_mut(&plume.material) {
        m.u.box_min = lo.extend(0.0);
        m.u.box_max = hi.extend(0.0);
        m.u.misc.x = plume.time;
        m.u.wind = wind.extend(0.0);
        m.u.flame_pos = plume.flame_centre.extend((plume.flame_power * 0.9).min(40.0));
    }
}
