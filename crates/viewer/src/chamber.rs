//! The flame inside a combustion chamber: a spark kernel that grows from the plug into a wrinkled, turbulent flame
//! front, with the hot burned gas glowing behind it. It is drawn by ray-marching the chamber's own volume
//! (`chamber_flame.wgsl`): the front is a thin bright sheet (the white-yellow reaction zone with a blue edge), pushed
//! about by turbulence, and the burned gas behind it glows with the colour of a black body at its temperature.

use crate::rig::anim::FlameC;
use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

pub struct ChamberPlugin;

impl Plugin for ChamberPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "chamber_flame.wgsl");
        app.add_plugins(MaterialPlugin::<ChamberFlameMaterial>::default());
    }
}

#[derive(Clone, Copy, ShaderType, Default, Debug)]
pub struct FlameUniform {
    /// x: radius of the flame front (m), y: how hot it is burning now (0..1), z: time (s), w: seed of this chamber.
    pub state: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct ChamberFlameMaterial {
    #[uniform(0)]
    pub u: FlameUniform,
}

impl Material for ChamberFlameMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://viewer/chamber_flame.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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
        // the far faces start the ray; the shader depth-tests against the opaque scene itself
        descriptor.primitive.cull_mode = Some(Face::Front);
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_compare = Some(CompareFunction::Always);
            ds.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// The material of a chamber's flame (the entity carries the same handle as its `MeshMaterial3d`).
#[derive(Component)]
pub struct ChamberMat(pub Handle<ChamberFlameMaterial>);

/// Gives every freshly built flame volume its own material.
pub fn attach_flame_materials(mut commands: Commands, q: Query<(Entity, &FlameC), Without<ChamberMat>>, mut mats: ResMut<Assets<ChamberFlameMaterial>>) {
    for (e, f) in &q {
        let h = mats.add(ChamberFlameMaterial { u: FlameUniform { state: Vec4::new(0.0, 0.0, 0.0, f.ci as f32 * 7.31) } });
        commands.entity(e).insert((MeshMaterial3d(h.clone()), ChamberMat(h)));
    }
}
