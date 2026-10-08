//! PBR material library. Metals are physically plausible (high metallic, low-ish roughness) and
//! rely on the studio environment map for their look.

use bevy::prelude::*;

#[derive(Resource, Clone)]
pub struct Mats {
    pub steel: Handle<StandardMaterial>,
    pub polished: Handle<StandardMaterial>,
    pub dark_steel: Handle<StandardMaterial>,
    pub cast_iron: Handle<StandardMaterial>,
    pub aluminum: Handle<StandardMaterial>,
    pub brushed_alu: Handle<StandardMaterial>,
    pub brass: Handle<StandardMaterial>,
    pub copper: Handle<StandardMaterial>,
    pub anodized_red: Handle<StandardMaterial>,
    pub anodized_blue: Handle<StandardMaterial>,
    pub gold_foil: Handle<StandardMaterial>,
    pub carbon: Handle<StandardMaterial>,
    pub rubber: Handle<StandardMaterial>,
    pub glass: Handle<StandardMaterial>,
    pub ghost_casing: Handle<StandardMaterial>,
    pub head_glass: Handle<StandardMaterial>,
    pub paint_red: Handle<StandardMaterial>,
    pub floor: Handle<StandardMaterial>,
    pub floor_mark: Handle<StandardMaterial>,
    pub plug_ceramic: Handle<StandardMaterial>,
    pub hot_metal: Handle<StandardMaterial>,
    pub chrome: Handle<StandardMaterial>,
    pub stainless: Handle<StandardMaterial>,
    pub ti_burnt: Handle<StandardMaterial>,
    pub filter_red: Handle<StandardMaterial>,
    pub filter_black: Handle<StandardMaterial>,
    pub silicone_blue: Handle<StandardMaterial>,
    pub silicone_black: Handle<StandardMaterial>,
    pub braided: Handle<StandardMaterial>,
    pub paint_black: Handle<StandardMaterial>,
    pub anodized_purple: Handle<StandardMaterial>,
    pub frame_steel: Handle<StandardMaterial>,
    pub radiator_core: Handle<StandardMaterial>,
    pub hose_orange: Handle<StandardMaterial>,
}

fn metal(color: Color, rough: f32) -> StandardMaterial {
    StandardMaterial { base_color: color, metallic: 1.0, perceptual_roughness: rough, reflectance: 0.5, ..default() }
}

impl Mats {
    pub fn new(m: &mut Assets<StandardMaterial>) -> Self {
        Self {
            steel: m.add(metal(Color::srgb(0.62, 0.64, 0.67), 0.32)),
            polished: m.add(metal(Color::srgb(0.9, 0.91, 0.93), 0.1)),
            dark_steel: m.add(metal(Color::srgb(0.22, 0.23, 0.26), 0.42)),
            cast_iron: m.add(metal(Color::srgb(0.17, 0.17, 0.18), 0.62)),
            aluminum: m.add(metal(Color::srgb(0.55, 0.57, 0.6), 0.42)),
            brushed_alu: m.add(metal(Color::srgb(0.74, 0.76, 0.8), 0.4)),
            brass: m.add(metal(Color::srgb(0.85, 0.65, 0.25), 0.28)),
            copper: m.add(metal(Color::srgb(0.95, 0.55, 0.38), 0.25)),
            anodized_red: m.add(metal(Color::srgb(0.75, 0.05, 0.06), 0.3)),
            anodized_blue: m.add(metal(Color::srgb(0.08, 0.28, 0.85), 0.3)),
            gold_foil: m.add(metal(Color::srgb(0.95, 0.72, 0.2), 0.22)),
            carbon: m.add(StandardMaterial {
                base_color: Color::srgb(0.03, 0.03, 0.035),
                metallic: 0.3,
                perceptual_roughness: 0.28,
                clearcoat: 1.0,
                clearcoat_perceptual_roughness: 0.06,
                ..default()
            }),
            rubber: m.add(StandardMaterial {
                base_color: Color::srgb(0.035, 0.035, 0.04),
                perceptual_roughness: 0.88,
                reflectance: 0.2,
                ..default()
            }),
            glass: m.add(StandardMaterial {
                base_color: Color::srgba(0.75, 0.88, 1.0, 0.10),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.04,
                metallic: 0.0,
                reflectance: 0.6,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
            ghost_casing: m.add(StandardMaterial {
                base_color: Color::srgba(0.45, 0.5, 0.58, 0.16),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.18,
                metallic: 0.6,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
            head_glass: m.add(StandardMaterial {
                base_color: Color::srgba(0.16, 0.2, 0.28, 0.42),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.12,
                metallic: 0.5,
                reflectance: 0.7,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
            paint_red: m.add(StandardMaterial {
                base_color: Color::srgb(0.65, 0.02, 0.03),
                metallic: 0.2,
                perceptual_roughness: 0.35,
                clearcoat: 1.0,
                clearcoat_perceptual_roughness: 0.08,
                ..default()
            }),
            floor: m.add(StandardMaterial {
                base_color: Color::srgb(0.03, 0.033, 0.04),
                metallic: 0.15,
                perceptual_roughness: 0.16,
                reflectance: 0.7,
                ..default()
            }),
            // dim cyan light strips, scrolled with vehicle speed as a motion cue
            floor_mark: m.add(StandardMaterial {
                base_color: Color::srgb(0.12, 0.2, 0.26),
                emissive: LinearRgba::rgb(0.25, 0.7, 1.0),
                perceptual_roughness: 0.4,
                ..default()
            }),
            plug_ceramic: m.add(StandardMaterial {
                base_color: Color::srgb(0.93, 0.92, 0.88),
                perceptual_roughness: 0.18,
                clearcoat: 0.6,
                ..default()
            }),
            hot_metal: m.add(StandardMaterial {
                base_color: Color::srgb(0.18, 0.16, 0.15),
                metallic: 1.0,
                perceptual_roughness: 0.55,
                ..default()
            }),
            chrome: m.add(metal(Color::srgb(0.96, 0.97, 1.0), 0.04)),
            stainless: m.add(metal(Color::srgb(0.7, 0.72, 0.75), 0.3)),
            // electro-polished titanium with the blue/purple heat tint of a used header
            ti_burnt: m.add(metal(Color::srgb(0.38, 0.36, 0.55), 0.22)),
            // oiled cotton gauze
            filter_red: m.add(StandardMaterial {
                base_color: Color::srgb(0.7, 0.04, 0.05),
                perceptual_roughness: 0.82,
                reflectance: 0.15,
                ..default()
            }),
            filter_black: m.add(StandardMaterial {
                base_color: Color::srgb(0.025, 0.025, 0.03),
                perceptual_roughness: 0.9,
                reflectance: 0.12,
                ..default()
            }),
            silicone_blue: m.add(StandardMaterial {
                base_color: Color::srgb(0.03, 0.17, 0.9),
                perceptual_roughness: 0.22,
                reflectance: 0.55,
                clearcoat: 0.6,
                clearcoat_perceptual_roughness: 0.15,
                ..default()
            }),
            silicone_black: m.add(StandardMaterial {
                base_color: Color::srgb(0.02, 0.02, 0.024),
                perceptual_roughness: 0.4,
                reflectance: 0.4,
                clearcoat: 0.5,
                ..default()
            }),
            braided: m.add(metal(Color::srgb(0.5, 0.52, 0.56), 0.5)),
            paint_black: m.add(StandardMaterial {
                base_color: Color::srgb(0.012, 0.012, 0.015),
                metallic: 0.5,
                perceptual_roughness: 0.3,
                clearcoat: 1.0,
                clearcoat_perceptual_roughness: 0.1,
                ..default()
            }),
            anodized_purple: m.add(metal(Color::srgb(0.38, 0.08, 0.7), 0.3)),
            frame_steel: m.add(StandardMaterial {
                base_color: Color::srgb(0.07, 0.075, 0.085),
                metallic: 0.85,
                perceptual_roughness: 0.48,
                ..default()
            }),
            radiator_core: m.add(StandardMaterial {
                base_color: Color::srgb(0.05, 0.05, 0.055),
                metallic: 0.6,
                perceptual_roughness: 0.55,
                ..default()
            }),
            hose_orange: m.add(StandardMaterial {
                base_color: Color::srgb(0.95, 0.38, 0.03),
                perceptual_roughness: 0.3,
                reflectance: 0.5,
                clearcoat: 0.6,
                ..default()
            }),
        }
    }
}
