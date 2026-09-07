//! Chunk-batched tapered grass blades with stable placement and bounded detail.
use super::chunk_mesh::ProductionTerrainPalette;
use bevy::{
    asset::{Asset, Handle, RenderAssetUsages, load_internal_asset, uuid_handle},
    mesh::{Indices, PrimitiveTopology},
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::{Shader, ShaderRef},
};
use latticeaxiom_storage::ChunkCoordinate;
use latticeaxiom_voxel_mesh::{Face, LayerMergeKey, MeshBuffer, MeshGroup};
use std::collections::BTreeMap;

const WIND: Handle<Shader> = uuid_handle!("14ff103a-e0e5-4eab-99fa-60620d0eb8e8");
pub(super) type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassWind>;
#[derive(Asset, AsBindGroup, Clone, Debug, Reflect)]
pub(super) struct GrassWind {
    #[uniform(100)]
    settings: Vec4,
}
impl MaterialExtension for GrassWind {
    fn vertex_shader() -> ShaderRef {
        WIND.clone().into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}
pub(super) fn install(app: &mut App) {
    load_internal_asset!(app, WIND, "grass_wind.wgsl", Shader::from_wgsl);
    app.add_plugins(MaterialPlugin::<GrassMaterial>::default());
}
pub(super) fn material() -> GrassMaterial {
    GrassMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.9,
            cull_mode: None,
            double_sided: true,
            ..Default::default()
        },
        extension: GrassWind {
            settings: Vec4::new(0.045, 1.5, 32.0, 56.0),
        },
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct FoliageStyle {
    pub color: [f32; 3],
    pub height: f32,
    pub decorative: bool,
}

fn hash(x: u32, z: u32) -> u32 {
    let mut n = x.wrapping_mul(0x9e37_79b9) ^ z.wrapping_mul(0x85eb_ca6b);
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb_352d);
    n ^ (n >> 15)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "Chunk-local coordinates are bounded to 32 voxels"
)]
pub(super) fn chunk_mesh(
    geometry: &MeshBuffer<LayerMergeKey>,
    palette: &ProductionTerrainPalette,
    chunk: ChunkCoordinate,
) -> Option<Mesh> {
    let mut roots = BTreeMap::new();
    for group in MeshGroup::ALL {
        for face in Face::ALL {
            for quad in geometry.group(group, face) {
                let Some(style) = palette.foliage_style(quad.merge_key()) else {
                    continue;
                };
                if style.decorative && face != Face::PosY {
                    continue;
                }
                let (u, v) = match face {
                    Face::PosX => (1, 2),
                    Face::NegX => (2, 1),
                    Face::PosY => (2, 0),
                    Face::NegY => (0, 2),
                    Face::PosZ => (0, 1),
                    Face::NegZ => (1, 0),
                };
                for a in 0..quad.width() {
                    for b in 0..quad.height() {
                        let mut p = quad.minimum();
                        p[u] += a;
                        p[v] += b;
                        let wx = chunk.x.cast_unsigned().wrapping_mul(32).wrapping_add(p[0]);
                        let wz = chunk.z.cast_unsigned().wrapping_mul(32).wrapping_add(p[2]);
                        let seed = hash(wx, wz);
                        if style.decorative
                            && (!seed.is_multiple_of(5) || hash(wx / 7, wz / 7).is_multiple_of(5))
                        {
                            continue;
                        }
                        if roots.len() >= 1024 {
                            break;
                        }
                        roots.insert(p, (style, seed));
                    }
                }
            }
        }
    }
    if roots.is_empty() {
        return None;
    }
    let mut mesh = Blades::default();
    for (p, (style, seed)) in roots {
        let offset = if style.decorative { 1.0 } else { 0.0 };
        let root = Vec3::new(p[0] as f32 + 0.5, p[1] as f32 + offset, p[2] as f32 + 0.5);
        mesh.tuft(root, seed, style.height, style.color);
    }
    Some(mesh.finish())
}

pub(super) fn tuft_mesh(color: [f32; 3]) -> Mesh {
    let mut mesh = Blades::default();
    mesh.tuft(Vec3::new(0.0, -0.45, 0.0), 17, 0.85, color);
    mesh.finish()
}

#[derive(Default)]
struct Blades {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}
impl Blades {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "Modulo values fit exactly in f32; at most 1024 six-blade tufts enter one mesh"
    )]
    fn tuft(&mut self, root: Vec3, seed: u32, height: f32, color: [f32; 3]) {
        let jitter = Vec3::new(
            (seed % 31) as f32 / 80.0 - 0.18,
            0.0,
            ((seed >> 8) % 31) as f32 / 80.0 - 0.18,
        );
        for blade in 0..6 {
            let n = hash(seed, blade);
            let angle = (n % 6283) as f32 / 1000.0;
            let axis = Vec3::new(angle.cos(), 0.0, angle.sin());
            let bend = Vec3::new(-axis.z, 0.0, axis.x);
            let h = height * (0.65 + (n % 100) as f32 / 220.0);
            let width = 0.025 + (n % 11) as f32 / 600.0;
            let root = root + jitter + axis * 0.06;
            let middle = root + Vec3::Y * h * 0.56 + bend * h * 0.12;
            let tip = root + Vec3::Y * h + bend * h * 0.35;
            let positions = [
                root - axis * width,
                root + axis * width,
                middle - axis * width * 0.5,
                middle + axis * width * 0.5,
                tip,
            ];
            let base = self.positions.len() as u32;
            for (index, p) in positions.into_iter().enumerate() {
                self.positions.push(p.to_array());
                self.normals.push(bend.to_array());
                let brightness = if index < 2 {
                    0.58
                } else if index < 4 {
                    0.92
                } else {
                    1.12
                };
                self.colors.push([
                    color[0] * brightness,
                    color[1] * brightness,
                    color[2] * brightness,
                    1.0,
                ]);
                self.uvs.push([p.y - root.y, 0.0]);
            }
            self.indices.extend([
                base,
                base + 1,
                base + 2,
                base + 1,
                base + 3,
                base + 2,
                base + 2,
                base + 3,
                base + 4,
            ]);
        }
    }
    fn finish(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn tuft_has_tapered_non_degenerate_geometry_inside_one_item_cell() {
        let mesh = tuft_mesh([0.2, 0.5, 0.1]);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("grass positions");
        };
        assert!(
            positions
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite() && v.abs() < 0.6))
        );
        let indices = mesh
            .indices()
            .expect("triangle indices")
            .iter()
            .collect::<Vec<_>>();
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] =
                [triangle[0], triangle[1], triangle[2]].map(|i| Vec3::from(positions[i]));
            assert!((b - a).cross(c - a).length_squared() > 0.000_001);
        }
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("blade gradient");
        };
        assert!(
            colors[0][1] < colors[4][1],
            "tips catch more light than roots"
        );
    }
}
