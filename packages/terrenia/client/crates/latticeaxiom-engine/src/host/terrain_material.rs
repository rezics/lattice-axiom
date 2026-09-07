//! Repeating array textures retain voxel scale across greedy terrain quads.

use bevy::{
    asset::{Asset, Handle, load_internal_asset, uuid_handle},
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin},
    prelude::{App, Image, Reflect, StandardMaterial, Vec4},
    render::render_resource::AsBindGroup,
    shader::{Shader, ShaderRef},
};

const TERRAIN_SHADER: Handle<Shader> = uuid_handle!("e8979eb7-b38d-4a46-b0dc-8dd0da2d7418");
const TERRAIN_DEPTH_SHADER: Handle<Shader> = uuid_handle!("83d18216-075f-48ae-b72b-7b9bde5d00a4");

pub(super) type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainMaterialExtension>;

#[derive(Asset, AsBindGroup, Clone, Debug, Reflect)]
pub(super) struct TerrainMaterialExtension {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub texture: Handle<Image>,
    #[uniform(102)]
    pub settings: Vec4,
}

impl MaterialExtension for TerrainMaterialExtension {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.clone().into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        TERRAIN_DEPTH_SHADER.clone().into()
    }
}

pub(super) fn install(app: &mut App) {
    load_internal_asset!(
        app,
        TERRAIN_SHADER,
        "terrain_material.wgsl",
        Shader::from_wgsl
    );
    load_internal_asset!(
        app,
        TERRAIN_DEPTH_SHADER,
        "terrain_depth.wgsl",
        Shader::from_wgsl
    );
    app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
}
