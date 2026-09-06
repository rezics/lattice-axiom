//! Bounded client presentation for deterministic far-terrain shells.

use std::{collections::BTreeMap, sync::Arc};

use bevy::{
    asset::{Assets, Handle, RenderAssetUsages},
    camera::visibility::VisibilityRange,
    color::ColorToComponents,
    mesh::{Indices, Mesh, PrimitiveTopology},
    prelude::{
        Color, Commands, Component, DetectChangesMut, Entity, Mesh3d, MeshMaterial3d, Query, Res,
        ResMut, Resource, Transform, Vec3, With,
    },
};
use latticeaxiom_gameplay::BlockId;
use latticeaxiom_player::LocalPlayerInput;
use latticeaxiom_worldgen::{
    D4MaterialRoleV1, FarTerrainBorderSideV1, FarTerrainTileAddressV1, FarTerrainTileV1,
    FarTerrainWallAxisV1,
};

use super::{
    ProductionSpine, TerrainDistanceStatusV1,
    chunk_mesh::{ChunkGroupMesh, ProductionTerrainMaterials, block_color},
};

/// Maximum number of CPU far shells converted and uploaded in one frame.
pub(super) const FAR_TERRAIN_TILE_UPLOAD_CAP_PER_FRAME: usize = 2;

const FALLBACK_SURFACE_COLOR: [f32; 4] = [0.38, 0.41, 0.43, 1.0];

/// GPU-complete contiguous frontier. `None` fail-closes to full-detail terrain.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub(super) struct FarTerrainPresentationStatusV1 {
    contiguous_meters: Option<u32>,
}

impl FarTerrainPresentationStatusV1 {
    pub(super) const fn contiguous_meters(self) -> Option<u32> {
        self.contiguous_meters
    }
}

/// Package-selected colors for semantic far-surface roles.
#[derive(Clone, Debug, Resource)]
pub(super) struct FarTerrainRolePalette {
    colors: BTreeMap<D4MaterialRoleV1, [f32; 4]>,
}

impl FarTerrainRolePalette {
    pub(super) fn from_blocks(
        blocks: &BTreeMap<D4MaterialRoleV1, BlockId>,
        resources: &latticeaxiom_render_contracts::ResolvedResourcePacks,
    ) -> Self {
        Self {
            colors: blocks
                .iter()
                .map(|(role, block)| {
                    let [r, g, b, a] = resources.material(block.as_str()).map_or_else(
                        || block_color(block.as_str()),
                        latticeaxiom_render_contracts::ResourceMaterial::rgba,
                    );
                    // Both authored and fallback near atlases are sampled as
                    // sRGB; mesh vertex colors must already be linear.
                    (*role, Color::srgba(r, g, b, a).to_linear().to_f32_array())
                })
                .collect(),
        }
    }

    fn color(&self, role: D4MaterialRoleV1) -> [f32; 4] {
        self.colors
            .get(&role)
            .copied()
            .unwrap_or(FALLBACK_SURFACE_COLOR)
    }
}

/// Root identity for one presentation-only far tile.
#[derive(Clone, Component, Debug)]
pub(super) struct FarTerrainPresentation {
    address: FarTerrainTileAddressV1,
    source: Arc<FarTerrainTileV1>,
    solid_mesh: Handle<Mesh>,
    water_mesh: Option<Handle<Mesh>>,
}

/// Solid and water are separate mesh/material lanes with the same ownership range.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) struct FarTerrainSurfaceMesh {
    span_meters: u16,
    lane: FarTerrainSurfaceLane,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FarTerrainSurfaceLane {
    Solid,
    Water,
}

#[derive(Clone, Debug, Default)]
struct FarTerrainCpuMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl FarTerrainCpuMesh {
    fn append_triangle(&mut self, points: [[f32; 3]; 3], color: [f32; 4], uvs: [[f32; 2]; 3]) {
        let first = Vec3::from(points[0]);
        let second = Vec3::from(points[1]);
        let third = Vec3::from(points[2]);
        let cross = (second - first).cross(third - first);
        let normal = if cross.length_squared() > f32::EPSILON {
            cross.normalize().to_array()
        } else {
            Vec3::Y.to_array()
        };
        let base = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        self.positions.extend(points);
        self.normals.extend([normal; 3]);
        self.colors.extend([color; 3]);
        self.uvs.extend(uvs);
        self.indices
            .extend([base, base.saturating_add(1), base.saturating_add(2)]);
    }

    fn append_quad(&mut self, points: [[f32; 3]; 4], color: [f32; 4]) {
        self.append_triangle(
            [points[0], points[1], points[2]],
            color,
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        );
        self.append_triangle(
            [points[0], points[2], points[3]],
            color,
            [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0]],
        );
    }

    fn into_bevy_mesh(self) -> Option<Mesh> {
        if self.indices.is_empty() {
            return None;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        Some(mesh)
    }
}

#[derive(Clone, Debug)]
struct FarTerrainCpuMeshes {
    solid: FarTerrainCpuMesh,
    water: FarTerrainCpuMesh,
}

impl FarTerrainCpuMeshes {
    fn from_tile(tile: &FarTerrainTileV1, palette: &FarTerrainRolePalette) -> Self {
        let mut solid = FarTerrainCpuMesh::default();
        append_solid_top(&mut solid, tile, palette);
        append_cliff_walls(&mut solid, tile, palette);
        append_border_skirts(&mut solid, tile, palette);
        Self {
            solid,
            water: water_mesh(tile),
        }
    }
}

fn append_solid_top(
    solid: &mut FarTerrainCpuMesh,
    tile: &FarTerrainTileV1,
    palette: &FarTerrainRolePalette,
) {
    let shell = tile.shell();
    let vertices = shell.vertices();
    for triangle in shell.top_triangles() {
        let [first, second, third] = triangle.0.map(|index| usize::try_from(index).unwrap_or(0));
        let Some(vertex) = vertices.get(first) else {
            continue;
        };
        let Some(points) = [first, second, third]
            .map(|index| vertices.get(index).map(surface_point))
            .into_iter()
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Ok(points) = <Vec<[f32; 3]> as TryInto<[[f32; 3]; 3]>>::try_into(points) else {
            continue;
        };
        solid.append_triangle(
            points,
            palette.color(vertex.material),
            points.map(|point| planar_uv(point, tile)),
        );
    }
}

fn append_cliff_walls(
    solid: &mut FarTerrainCpuMesh,
    tile: &FarTerrainTileV1,
    palette: &FarTerrainRolePalette,
) {
    for wall in tile.shell().cliff_walls() {
        let bottom = surface_height(wall.bottom_y);
        let top = surface_height(wall.top_y);
        let plane = f32::from(wall.plane_offset);
        let segment = f32::from(wall.segment_offset);
        let points = match wall.axis {
            FarTerrainWallAxisV1::X => [
                [plane, bottom, segment],
                [plane, top, segment],
                [plane, top, segment + 1.0],
                [plane, bottom, segment + 1.0],
            ],
            FarTerrainWallAxisV1::Z => [
                [segment, bottom, plane],
                [segment + 1.0, bottom, plane],
                [segment + 1.0, top, plane],
                [segment, top, plane],
            ],
        };
        solid.append_quad(points, palette.color(wall.material));
    }
}

#[allow(clippy::too_many_lines)] // Four border orientations keep their winding explicit.
fn append_border_skirts(
    solid: &mut FarTerrainCpuMesh,
    tile: &FarTerrainTileV1,
    palette: &FarTerrainRolePalette,
) {
    let vertices = tile.shell().vertices();
    let edge = tile.base_tile_edge_voxels();
    let step = tile.sample_step_voxels();
    for skirt in tile.shell().border_skirts() {
        if !owns_border_skirt(skirt.side) {
            continue;
        }
        let first_offset = skirt.segment.saturating_mul(step);
        let second_offset = skirt.segment.saturating_add(1).saturating_mul(step);
        let span = edge.saturating_mul(step);
        let first_top = surface_height(skirt.first_top_y);
        let second_top = surface_height(skirt.second_top_y);
        let bottom = surface_height(skirt.bottom_y);
        let (points, color_vertex) = match skirt.side {
            FarTerrainBorderSideV1::North => (
                [
                    [f32::from(first_offset), bottom, 0.0],
                    [f32::from(first_offset), first_top, 0.0],
                    [f32::from(second_offset), second_top, 0.0],
                    [f32::from(second_offset), bottom, 0.0],
                ],
                vertex_index(first_offset, 0, step, edge),
            ),
            FarTerrainBorderSideV1::South => (
                [
                    [f32::from(second_offset), bottom, f32::from(span)],
                    [f32::from(second_offset), second_top, f32::from(span)],
                    [f32::from(first_offset), first_top, f32::from(span)],
                    [f32::from(first_offset), bottom, f32::from(span)],
                ],
                vertex_index(first_offset, span, step, edge),
            ),
            FarTerrainBorderSideV1::West => (
                [
                    [0.0, bottom, f32::from(second_offset)],
                    [0.0, second_top, f32::from(second_offset)],
                    [0.0, first_top, f32::from(first_offset)],
                    [0.0, bottom, f32::from(first_offset)],
                ],
                vertex_index(0, first_offset, step, edge),
            ),
            FarTerrainBorderSideV1::East => (
                [
                    [f32::from(span), bottom, f32::from(first_offset)],
                    [f32::from(span), first_top, f32::from(first_offset)],
                    [f32::from(span), second_top, f32::from(second_offset)],
                    [f32::from(span), bottom, f32::from(second_offset)],
                ],
                vertex_index(span, first_offset, step, edge),
            ),
        };
        let color = vertices
            .get(color_vertex)
            .map_or(FALLBACK_SURFACE_COLOR, |vertex| {
                palette.color(vertex.material)
            });
        solid.append_quad(points, color);
    }
}

fn owns_border_skirt(side: FarTerrainBorderSideV1) -> bool {
    // East/South is the stable half-open ownership rule. Adjacent equal-LOD
    // tiles and coarse/fine neighbors therefore never submit coplanar skirts.
    matches!(
        side,
        FarTerrainBorderSideV1::East | FarTerrainBorderSideV1::South
    )
}

fn water_mesh(tile: &FarTerrainTileV1) -> FarTerrainCpuMesh {
    let shell = tile.shell();
    let water_vertices = shell.water_vertices();
    let mut water = FarTerrainCpuMesh::default();
    for triangle in shell.top_triangles() {
        let indices = triangle.0.map(|index| usize::try_from(index).unwrap_or(0));
        let Some(points) = indices
            .map(|index| {
                water_vertices.get(index).and_then(|vertex| {
                    vertex.surface_y.map(|surface_y| {
                        [
                            f32::from(vertex.local_x),
                            surface_height(surface_y),
                            f32::from(vertex.local_z),
                        ]
                    })
                })
            })
            .into_iter()
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Ok(points) = <Vec<[f32; 3]> as TryInto<[[f32; 3]; 3]>>::try_into(points) else {
            continue;
        };
        water.append_triangle(points, [1.0; 4], points.map(|point| planar_uv(point, tile)));
    }
    water
}

fn vertex_index(local_x: u16, local_z: u16, step: u16, edge: u16) -> usize {
    let coarse_x = local_x.checked_div(step).unwrap_or(0);
    let coarse_z = local_z.checked_div(step).unwrap_or(0);
    usize::from(coarse_z)
        .saturating_mul(usize::from(edge).saturating_add(1))
        .saturating_add(usize::from(coarse_x))
}

fn surface_point(vertex: &latticeaxiom_worldgen::FarTerrainVertexV1) -> [f32; 3] {
    [
        f32::from(vertex.local_x),
        surface_height(vertex.solid_y),
        f32::from(vertex.local_z),
    ]
}

#[expect(
    clippy::cast_precision_loss,
    reason = "world height is represented by Bevy's f32 transform space"
)]
fn surface_height(cell_y: i32) -> f32 {
    cell_y as f32 + 1.0
}

fn planar_uv(point: [f32; 3], tile: &FarTerrainTileV1) -> [f32; 2] {
    let span = f32::from(
        tile.base_tile_edge_voxels()
            .saturating_mul(tile.sample_step_voxels())
            .max(1),
    );
    [point[0] / span, point[2] / span]
}

/// Reconciles presentation-only far entities and advances the GPU-complete fog frontier.
#[allow(clippy::too_many_arguments)] // ECS resources are independent presentation contracts.
#[allow(clippy::too_many_lines)] // Reconciliation is kept atomic with frontier publication.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_far_terrain_presentation(
    mut commands: Commands<'_, '_>,
    spine: Res<'_, ProductionSpine>,
    players: Query<'_, '_, &Transform, With<LocalPlayerInput>>,
    materials: Option<Res<'_, ProductionTerrainMaterials>>,
    palette: Option<Res<'_, FarTerrainRolePalette>>,
    mut meshes: Option<ResMut<'_, Assets<Mesh>>>,
    existing: Query<'_, '_, (Entity, &FarTerrainPresentation)>,
    mut far_ranges: Query<'_, '_, (&FarTerrainSurfaceMesh, &mut VisibilityRange)>,
    mut near_ranges: Query<
        '_,
        '_,
        &mut VisibilityRange,
        (
            With<ChunkGroupMesh>,
            bevy::prelude::Without<FarTerrainSurfaceMesh>,
        ),
    >,
    mut presentation_status: ResMut<'_, FarTerrainPresentationStatusV1>,
) {
    let Some(distance_status) = spine.terrain_distance_status() else {
        presentation_status.contiguous_meters = None;
        return;
    };
    let chunk_edge = chunk_edge_meters(distance_status);
    let full_detail_meters = distance_status
        .full_detail_distance()
        .chunks()
        .saturating_mul(chunk_edge);
    let Some(player) = players.iter().next() else {
        presentation_status.contiguous_meters = Some(full_detail_meters);
        return;
    };
    let Some(materials) = materials else {
        presentation_status.contiguous_meters = Some(full_detail_meters);
        return;
    };
    let Some(palette) = palette else {
        presentation_status.contiguous_meters = Some(full_detail_meters);
        return;
    };
    let Some(meshes) = meshes.as_mut() else {
        presentation_status.contiguous_meters = Some(full_detail_meters);
        return;
    };

    let ready = spine.far_terrain_ready_tiles();
    let desired = ready
        .iter()
        .map(|tile| (tile.cache_key().address(), Arc::clone(tile)))
        .collect::<BTreeMap<_, _>>();
    let mut presented = BTreeMap::new();
    for (entity, current) in &existing {
        if desired.get(&current.address).is_some_and(|tile| {
            Arc::ptr_eq(tile, &current.source) || tile.cache_key() == current.source.cache_key()
        }) && !presented.contains_key(&current.address)
        {
            presented.insert(current.address, entity);
        } else {
            let _ = meshes.remove(current.solid_mesh.id());
            if let Some(water_mesh) = &current.water_mesh {
                let _ = meshes.remove(water_mesh.id());
            }
            commands.entity(entity).despawn();
        }
    }

    let player_xz = [
        f64::from(player.translation.x),
        f64::from(player.translation.z),
    ];
    let mut uploads = desired
        .iter()
        .filter(|(address, _)| !presented.contains_key(address))
        .map(|(address, tile)| {
            (
                tile_minimum_distance_squared_meters(tile, player_xz),
                *address,
                Arc::clone(tile),
            )
        })
        .collect::<Vec<_>>();
    uploads.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    for (_, address, tile) in uploads
        .into_iter()
        .take(FAR_TERRAIN_TILE_UPLOAD_CAP_PER_FRAME)
    {
        if let Some(entity) = spawn_far_tile(
            &mut commands,
            meshes,
            &materials,
            &palette,
            &tile,
            distance_status.presented_render_distance_meters().meters(),
        ) {
            presented.insert(address, entity);
        }
    }

    let cpu_frontier = distance_status.presented_render_distance_meters().meters();
    let first_missing = ready
        .iter()
        .filter(|tile| !presented.contains_key(&tile.cache_key().address()))
        .map(|tile| tile_minimum_distance_squared_meters(tile, player_xz).sqrt())
        .filter(|distance| *distance <= f64::from(cpu_frontier))
        .min_by(f64::total_cmp);
    let gpu_frontier = first_missing.map_or(cpu_frontier, |distance| {
        distance_to_u32(distance)
            .saturating_sub(chunk_edge)
            .max(full_detail_meters)
            .min(cpu_frontier)
    });
    presentation_status.contiguous_meters = Some(gpu_frontier);

    for (surface, mut range) in &mut far_ranges {
        let span = u32::from(surface.span_meters);
        let lane_margin = match surface.lane {
            FarTerrainSurfaceLane::Solid | FarTerrainSurfaceLane::Water => span,
        };
        range.set_if_neq(terrain_visibility_range(
            gpu_frontier.saturating_add(lane_margin),
        ));
    }
    for mut range in &mut near_ranges {
        range.set_if_neq(terrain_visibility_range(
            gpu_frontier.saturating_add(chunk_edge),
        ));
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "validated world coordinates enter Bevy's f32 transform space"
)]
fn spawn_far_tile(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &ProductionTerrainMaterials,
    palette: &FarTerrainRolePalette,
    tile: &Arc<FarTerrainTileV1>,
    visibility_meters: u32,
) -> Option<Entity> {
    let cpu = FarTerrainCpuMeshes::from_tile(tile, palette);
    let solid_mesh = cpu.solid.into_bevy_mesh()?;
    let [world_x, world_z] = tile.world_origin_xz();
    let address = tile.cache_key().address();
    let span = tile
        .base_tile_edge_voxels()
        .saturating_mul(tile.sample_step_voxels());
    let root = commands.spawn_empty().id();
    let solid_mesh = meshes.add(solid_mesh);
    let water_mesh = cpu.water.into_bevy_mesh().map(|mesh| meshes.add(mesh));
    commands.entity(root).insert((
        FarTerrainPresentation {
            address,
            source: Arc::clone(tile),
            solid_mesh: solid_mesh.clone(),
            water_mesh: water_mesh.clone(),
        },
        Transform::from_xyz(world_x as f32, 0.0, world_z as f32),
    ));
    let range = terrain_visibility_range(visibility_meters.saturating_add(u32::from(span)));
    let solid = commands
        .spawn((
            Transform::IDENTITY,
            Mesh3d(solid_mesh),
            MeshMaterial3d(materials.far_solid_handle().clone()),
            FarTerrainSurfaceMesh {
                span_meters: span,
                lane: FarTerrainSurfaceLane::Solid,
            },
            range.clone(),
        ))
        .id();
    commands.entity(root).add_child(solid);
    if let Some(water_mesh) = water_mesh {
        let water = commands
            .spawn((
                Transform::IDENTITY,
                Mesh3d(water_mesh),
                MeshMaterial3d(materials.far_water_handle().clone()),
                FarTerrainSurfaceMesh {
                    span_meters: span,
                    lane: FarTerrainSurfaceLane::Water,
                },
                range,
            ))
            .id();
        commands.entity(root).add_child(water);
    }
    Some(root)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "bounded terrain visibility distances are exactly representable in f32"
)]
fn terrain_visibility_range(end: u32) -> VisibilityRange {
    let mut range = VisibilityRange::abrupt(0.0, end.max(1) as f32);
    range.use_aabb = true;
    range
}

fn chunk_edge_meters(status: TerrainDistanceStatusV1) -> u32 {
    status
        .presented_render_distance_meters()
        .meters()
        .checked_div(status.presented_render_distance().chunks())
        .unwrap_or(1)
        .max(1)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "i64 voxel origins are exact throughout the supported f64 world-coordinate range"
)]
fn tile_minimum_distance_squared_meters(tile: &FarTerrainTileV1, player_xz: [f64; 2]) -> f64 {
    let [origin_x, origin_z] = tile.world_origin_xz();
    let span = i64::from(
        tile.base_tile_edge_voxels()
            .saturating_mul(tile.sample_step_voxels()),
    );
    let minimum_x = origin_x as f64;
    let maximum_x = origin_x.saturating_add(span) as f64;
    let minimum_z = origin_z as f64;
    let maximum_z = origin_z.saturating_add(span) as f64;
    let nearest_x = player_xz[0].clamp(minimum_x, maximum_x);
    let nearest_z = player_xz[1].clamp(minimum_z, maximum_z);
    (player_xz[0] - nearest_x).mul_add(
        player_xz[0] - nearest_x,
        (player_xz[1] - nearest_z) * (player_xz[1] - nearest_z),
    )
}

fn distance_to_u32(distance: f64) -> u32 {
    if !distance.is_finite() || distance <= 0.0 {
        return 0;
    }
    if distance >= f64::from(u32::MAX) {
        return u32::MAX;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "finite nonnegative distance is range-checked before flooring"
    )]
    let value = distance.floor() as u32;
    value
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::{ChunkRevision, DimensionId};
    use latticeaxiom_worldgen::{
        D4MaterialRoleV1, FarTerrainBorderSideV1, FarTerrainCommittedProvenanceV1,
        FarTerrainLodLevelV1, FarTerrainSourceProvenanceV1, FarTerrainSurfaceSampleV1,
        FarTerrainSurfaceSourceV1, FarTerrainTileAddressV1, FarTerrainTileCoordinateV1,
        SnapshotChecksumV1, WorldgenResult, build_far_terrain_tile_v1,
    };

    use super::{
        FALLBACK_SURFACE_COLOR, FAR_TERRAIN_TILE_UPLOAD_CAP_PER_FRAME, FarTerrainCpuMeshes,
        FarTerrainRolePalette, owns_border_skirt, surface_height, terrain_visibility_range,
    };

    struct WetCliff;

    impl FarTerrainSurfaceSourceV1 for WetCliff {
        fn sample_far_terrain_surface(
            &self,
            world_x: i64,
            _world_z: i64,
        ) -> WorldgenResult<FarTerrainSurfaceSampleV1> {
            let solid_y = if world_x >= 2 { 6 } else { 1 };
            let water_y = (world_x < 2).then_some(3);
            Ok(FarTerrainSurfaceSampleV1::new(
                solid_y,
                if world_x >= 2 {
                    D4MaterialRoleV1::TemperateBaseRock
                } else {
                    D4MaterialRoleV1::TemperateSurface
                },
                water_y,
            ))
        }
    }

    fn tile() -> latticeaxiom_worldgen::FarTerrainTileV1 {
        let provenance =
            FarTerrainSourceProvenanceV1::Committed(FarTerrainCommittedProvenanceV1::new(
                "terrenia:dimension/terrenia"
                    .parse::<DimensionId>()
                    .expect("fixture dimension is canonical"),
                ChunkRevision::new(1),
                SnapshotChecksumV1::from_hash(CanonicalHash::digest("far-mesh-test")),
            ));
        build_far_terrain_tile_v1(
            &WetCliff,
            provenance,
            FarTerrainTileAddressV1::new(
                FarTerrainTileCoordinateV1::new(0, 0),
                FarTerrainLodLevelV1::new(0).expect("LOD zero is valid"),
            ),
            4,
        )
        .expect("fixture far tile builds")
    }

    #[test]
    fn solid_and_water_are_distinct_nonempty_lanes() {
        let meshes = FarTerrainCpuMeshes::from_tile(
            &tile(),
            &FarTerrainRolePalette {
                colors: BTreeMap::new(),
            },
        );
        assert!(!meshes.solid.indices.is_empty());
        assert!(!meshes.water.indices.is_empty());
        assert!(meshes.solid.colors.iter().all(|color| {
            color
                .iter()
                .zip(FALLBACK_SURFACE_COLOR)
                .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
        }));
        assert!(meshes.water.colors.iter().all(|color| {
            color
                .iter()
                .all(|channel| channel.to_bits() == 1.0_f32.to_bits())
        }));
    }

    #[test]
    fn top_faces_point_up_and_cell_heights_convert_to_top_planes() {
        let meshes = FarTerrainCpuMeshes::from_tile(
            &tile(),
            &FarTerrainRolePalette {
                colors: BTreeMap::new(),
            },
        );
        assert!(
            meshes
                .solid
                .normals
                .chunks_exact(3)
                .any(|face| face[0][1] > 0.0)
        );
        assert_eq!(surface_height(6).to_bits(), 7.0_f32.to_bits());
    }

    #[test]
    fn visibility_is_abrupt_for_single_surface_ownership() {
        let range = terrain_visibility_range(672);
        assert!(range.is_abrupt());
        assert!(range.use_aabb);
        assert!(range.is_visible_at_all(671.0));
        assert!(range.is_culled(672.0));
    }

    #[test]
    fn upload_budget_is_a_small_hard_constant() {
        assert_eq!(FAR_TERRAIN_TILE_UPLOAD_CAP_PER_FRAME, 2);
    }

    #[test]
    fn border_skirts_have_one_half_open_owner() {
        assert!(owns_border_skirt(FarTerrainBorderSideV1::East));
        assert!(owns_border_skirt(FarTerrainBorderSideV1::South));
        assert!(!owns_border_skirt(FarTerrainBorderSideV1::West));
        assert!(!owns_border_skirt(FarTerrainBorderSideV1::North));
    }
}
