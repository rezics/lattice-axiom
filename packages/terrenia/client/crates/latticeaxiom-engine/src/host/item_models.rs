//! Shared native item geometry and bounded model-rendered inventory previews.

use super::{ProductionSpine, ProductionSurfaceRouter};
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, visibility::RenderLayers},
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    },
};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_gameplay::ItemId;
use latticeaxiom_render_contracts::{
    MaterialPattern, ResolvedResourcePacks, ResourceMaterial, ResourceModel,
};
use latticeaxiom_webview::GeneratedImages;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

const PREVIEW_LAYER: usize = 20;
const HELD_LAYER: usize = 21;
const EDGE: u32 = 128;

/// One source-authored mesh part shared by held and inventory presentation.
#[derive(Debug)]
pub struct ProceduralItemPart {
    /// Native Bevy geometry produced by the installed code package.
    pub mesh: Mesh,
    /// Local pose within the complete item model.
    pub transform: Transform,
    /// Resolved presentation-only material parameters.
    pub material: ResourceMaterial,
}

/// Source-package model builder. Importing a reference does not activate code.
pub type ProceduralItemBuilder =
    fn(&ResourceModel, &ResolvedResourcePacks) -> Vec<ProceduralItemPart>;

#[derive(Resource, Default)]
struct Providers(BTreeMap<StableId, ProceduralItemBuilder>);

impl crate::EngineInstance {
    /// Installs a source package's item-model provider for this client instance.
    ///
    /// # Errors
    /// Rejects a duplicate provider instead of silently replacing another package.
    pub fn register_item_model_provider(
        &mut self,
        id: StableId,
        builder: ProceduralItemBuilder,
    ) -> Result<(), String> {
        self.app.init_resource::<Providers>();
        let mut providers = self.app.world_mut().resource_mut::<Providers>();
        if providers.0.contains_key(&id) {
            return Err(format!("Item model provider {id} is already installed"));
        }
        providers.0.insert(id, builder);
        Ok(())
    }
}

#[derive(Resource, Debug, Default)]
pub(super) struct PreviewImages(pub GeneratedImages);

#[derive(Resource, Debug, Default)]
struct Requests {
    generation: String,
    queue: Mutex<PreviewQueue>,
}

type PreviewQueue = (VecDeque<(String, String)>, BTreeSet<String>);

#[derive(Clone)]
struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
}

#[derive(Resource, Default)]
struct Models {
    world: Option<String>,
    parts: BTreeMap<String, Vec<Part>>,
    camera: Option<Entity>,
    held_camera: Option<Entity>,
    target: Handle<Image>,
    active: Option<(String, Entity, u32)>,
    finished: bool,
    held: Option<(String, Entity)>,
}

#[derive(Component)]
struct PreviewReadback(String);

pub(super) fn install(app: &mut App) {
    app.init_resource::<PreviewImages>()
        .init_resource::<Providers>()
        .init_resource::<Requests>()
        .init_resource::<Models>();
    app.add_systems(Update, update);
}

pub(super) fn preview_url(world: &World, item: &str) -> Option<String> {
    let requests = world.get_resource::<Requests>()?;
    if requests.generation.is_empty() {
        return None;
    }
    let key = CanonicalHash::digest(format!("model-preview-v1|{}|{item}", requests.generation))
        .to_string();
    if world.get_resource::<PreviewImages>()?.0.contains(&key) {
        return Some(format!("/generated/{key}.png"));
    }
    if let Ok(mut queue) = requests.queue.lock() {
        if queue.0.len() < 128 && queue.1.insert(key.clone()) {
            queue.0.push_back((item.to_owned(), key));
        }
    }
    None
}

#[allow(
    clippy::too_many_lines,
    reason = "One bounded preview and held-item lifecycle updates shared camera state"
)]
fn update(world: &mut World) {
    let Some(spine) = world.get_resource::<ProductionSpine>().cloned() else {
        return;
    };
    if !world.contains_resource::<Assets<StandardMaterial>>()
        || !world.contains_resource::<Assets<Image>>()
    {
        return;
    }
    let Some(world_id) = spine.world_id().map(|id| id.to_string()) else {
        return;
    };
    let mut models = world.remove_resource::<Models>().unwrap_or_default();
    if models.world.as_ref() != Some(&world_id)
        || models
            .camera
            .is_none_or(|entity| world.get_entity(entity).is_err())
    {
        for entity in [
            models.camera,
            models.held_camera,
            models.active.as_ref().map(|(_, e, _)| *e),
            models.held.as_ref().map(|(_, e)| *e),
        ]
        .into_iter()
        .flatten()
        {
            let _ = world.despawn(entity);
        }
        models = Models::default();
        models.world = Some(world_id.clone());
        let resources = world
            .get_resource::<crate::resource_packs::ClientResourcePacks>()
            .map(|r| format!("{:?}", r.0))
            .unwrap_or_default();
        let mut requests = world.resource_mut::<Requests>();
        requests.generation = format!("{world_id}|{}", CanonicalHash::digest(resources));
        if let Ok(mut queue) = requests.queue.lock() {
            queue.0.clear();
            queue.1.clear();
        }
        setup_cameras(world, &mut models);
        #[cfg(feature = "development")]
        if std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").is_some()
            && std::path::Path::new(".latticeaxiom-qa").is_file()
            && let Some(catalog) = spine.gameplay_catalog()
        {
            if let Ok(item) = std::env::var("LATTICEAXIOM_CAPTURE_ITEM")
                && let Ok(id) = item.parse::<ItemId>()
                && catalog.item(&id).is_some()
            {
                let stack = catalog.tool(&id).map_or_else(
                    || latticeaxiom_gameplay::ItemStackV1::plain(id.clone(), 1),
                    |tool| {
                        latticeaxiom_gameplay::ItemStackV1::tool(
                            id.clone(),
                            tool.maximum_durability.get(),
                        )
                    },
                );
                if let Ok(stack) = stack {
                    let _ = spine
                        .seed_inventory_slot(latticeaxiom_gameplay::SlotIndex::new(0), Some(stack));
                    let _ = spine.select_hotbar_slot(0);
                }
            }
            let mut manifest = BTreeMap::new();
            for id in catalog
                .tools()
                .keys()
                .chain(catalog.items().keys())
                .take(128)
            {
                let _ = preview_url(world, id.as_str());
                let key = CanonicalHash::digest(format!(
                    "model-preview-v1|{}|{id}",
                    world.resource::<Requests>().generation,
                ))
                .to_string();
                manifest.insert(id.to_string(), key);
            }
            if let Some(path) = std::env::var_os("LATTICEAXIOM_CAPTURE_PATH")
                && let Some(parent) = std::path::Path::new(&path).parent()
                && let Ok(bytes) = serde_json::to_vec_pretty(&manifest)
            {
                let _ = std::fs::write(parent.join("item-previews.json"), bytes);
            }
        }
    }
    if models.finished {
        if let Some((key, entity, _)) = models.active.take() {
            let _ = world.despawn(entity);
            if let Ok(mut queue) = world.resource::<Requests>().queue.lock() {
                queue.1.remove(&key);
            }
        }
        models.finished = false;
    }
    let mut pending_readback = None;
    if let Some((key, _, age)) = &mut models.active {
        *age += 1;
        if *age == 240 {
            bevy::log::error!(preview = %key, "Item preview timed out before a nonempty GPU image");
            models.finished = true;
        }
        if *age == 4 {
            pending_readback = Some((key.clone(), models.target.clone()));
        }
    } else {
        let next = world
            .resource::<Requests>()
            .queue
            .lock()
            .ok()
            .and_then(|mut queue| queue.0.pop_front());
        if let Some((item, key)) = next {
            let parts = model_parts(world, &mut models, &spine, &item);
            let root = spawn_parts(world, &parts, PREVIEW_LAYER, Transform::IDENTITY);
            models.active = Some((key, root, 0));
        }
    }
    let selected = spine.inventory_view().and_then(|inventory| {
        inventory
            .slots()
            .get(usize::from(inventory.hotbar_slot()))
            .and_then(Option::as_ref)
            .map(|stack| stack.item().to_string())
    });
    if models.held.as_ref().map(|(item, _)| item.as_str()) != selected.as_deref() {
        if let Some((_, root)) = models.held.take() {
            let _ = world.despawn(root);
        }
        if let Some(item) = selected {
            let parts = model_parts(world, &mut models, &spine, &item);
            let root = spawn_parts(world, &parts, HELD_LAYER, Transform::IDENTITY);
            models.held = Some((item, root));
        }
    }
    let active = world
        .get_resource::<ProductionSurfaceRouter>()
        .is_some_and(|router| {
            let route = router.inner().route();
            route.overlay() == latticeaxiom_client_ui::GameOverlayV1::None
                && route.modal() == latticeaxiom_client_ui::GameModalV1::None
        });
    if let Some(camera) = models.held_camera
        && let Some(mut camera) = world.get_mut::<Camera>(camera)
    {
        camera.is_active = active && models.held.is_some();
    }
    if let Some(camera) = models.camera
        && let Some(mut camera) = world.get_mut::<Camera>(camera)
    {
        camera.is_active = models.active.is_some();
    }
    let time = world.get_resource::<Time>().map_or(0.0, Time::elapsed_secs);
    let swinging = active
        && world
            .get_resource::<ButtonInput<MouseButton>>()
            .is_some_and(|buttons| buttons.pressed(MouseButton::Left));
    if let Some((_, root)) = &models.held
        && let Some(mut transform) = world.get_mut::<Transform>(*root)
    {
        let swing = if swinging {
            (time * 12.0).sin().max(0.0)
        } else {
            0.0
        };
        *transform = Transform::from_xyz(0.47 - swing * 0.12, -0.39 - swing * 0.08, -0.9)
            .with_scale(Vec3::splat(0.48))
            .with_rotation(Quat::from_euler(
                EulerRot::XYZ,
                -0.2 + swing * 0.65,
                -0.5,
                -0.32 - swing * 0.35,
            ));
    }
    world.insert_resource(models);
    if let Some((key, target)) = pending_readback {
        world
            .spawn((
                Readback::texture(target),
                PreviewReadback(key),
                super::InProcessPlayEntity,
            ))
            .observe(capture);
    }
}

fn setup_cameras(world: &mut World, models: &mut Models) {
    let mut image = Image::new_target_texture(EDGE, EDGE, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    models.target = world.resource_mut::<Assets<Image>>().add(image);
    models.camera = Some(
        world
            .spawn((
                Camera3d::default(),
                Camera {
                    order: -2,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..Default::default()
                },
                RenderTarget::Image(models.target.clone().into()),
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                        viewport_height: 1.55,
                    },
                    ..OrthographicProjection::default_3d()
                }),
                Transform::from_xyz(2.0, 1.4, 2.8).looking_at(Vec3::new(0.0, 0.03, 0.0), Vec3::Y),
                RenderLayers::layer(PREVIEW_LAYER),
                AmbientLight {
                    brightness: 140.0,
                    ..Default::default()
                },
                super::InProcessPlayEntity,
            ))
            .id(),
    );
    models.held_camera = Some(
        world
            .spawn((
                Camera3d::default(),
                Camera {
                    order: 2,
                    clear_color: ClearColorConfig::None,
                    ..Default::default()
                },
                Transform::default(),
                RenderLayers::layer(HELD_LAYER),
                AmbientLight {
                    brightness: 100.0,
                    ..Default::default()
                },
                super::InProcessPlayEntity,
            ))
            .id(),
    );
    world.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: false,
            ..Default::default()
        },
        Transform::from_xyz(2.0, 4.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(PREVIEW_LAYER).with(HELD_LAYER),
        super::InProcessPlayEntity,
    ));
}

#[allow(clippy::needless_pass_by_value, reason = "Bevy observer parameters")]
fn capture(
    event: On<'_, '_, ReadbackComplete>,
    mut commands: Commands<'_, '_>,
    pending: Query<'_, '_, &PreviewReadback>,
    images: Res<'_, PreviewImages>,
    // GPU readback can complete while `update` has exclusive world access without this resource.
    models: Option<ResMut<'_, Models>>,
) {
    let Some(mut models) = models else {
        commands.entity(event.entity).despawn();
        return;
    };
    let Ok(request) = pending.get(event.entity) else {
        return;
    };
    if models
        .active
        .as_ref()
        .is_none_or(|(key, _, _)| key != &request.0)
    {
        commands.entity(event.entity).despawn();
        return;
    }
    if event.data.len() < (EDGE * EDGE * 4) as usize
        || !event.data.chunks_exact(4).any(|pixel| pixel[3] > 0)
    {
        return;
    }
    let mut bytes = Vec::new();
    let result = (|| {
        let mut encoder = png::Encoder::new(&mut bytes, EDGE, EDGE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&event.data[..(EDGE * EDGE * 4) as usize])?;
        writer.finish()
    })();
    if result.is_ok() {
        #[cfg(feature = "development")]
        if let Some(path) = std::env::var_os("LATTICEAXIOM_CAPTURE_PATH")
            && std::path::Path::new(".latticeaxiom-qa").is_file()
            && let Some(parent) = std::path::Path::new(&path).parent()
        {
            let directory = parent.join("item-previews");
            if std::fs::create_dir_all(&directory).is_ok() {
                let _ = std::fs::write(directory.join(format!("{}.png", request.0)), &bytes);
            }
        }
        if let Err(error) = images.0.publish(&request.0, bytes) {
            bevy::log::error!(%error, "Item preview publication failed");
        }
    }
    commands.entity(event.entity).despawn();
    models.finished = true;
}

fn spawn_parts(world: &mut World, parts: &[Part], layer: usize, transform: Transform) -> Entity {
    let root = world
        .spawn((transform, Visibility::default(), super::InProcessPlayEntity))
        .id();
    for part in parts {
        let child = world
            .spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(part.material.clone()),
                part.transform,
                RenderLayers::layer(layer),
                Visibility::default(),
            ))
            .id();
        world.entity_mut(root).add_child(child);
    }
    root
}

#[allow(
    clippy::too_many_lines,
    reason = "Declarative geometry for the small built-in model family"
)]
fn model_parts(
    world: &mut World,
    models: &mut Models,
    spine: &ProductionSpine,
    item: &str,
) -> Vec<Part> {
    if let Some(parts) = models.parts.get(item) {
        return parts.clone();
    }
    let resources = world
        .get_resource::<crate::resource_packs::ClientResourcePacks>()
        .map(|r| r.0.clone())
        .unwrap_or_default();
    let catalog = spine.gameplay_catalog();
    let id = item.parse::<ItemId>().ok();
    let definition = id.as_ref().and_then(|id| catalog.as_ref()?.item(id));
    let block = definition
        .and_then(|item| item.placement_block.as_ref())
        .map(ToString::to_string);
    let reference = resources
        .model(item)
        .or_else(|| block.as_ref().and_then(|id| resources.model(id)));
    let kind = reference.map_or(if block.is_some() { "block" } else { "lump" }, |model| {
        model.provider.path()
    });
    let material_id = reference
        .and_then(|model| model.material.as_ref())
        .map_or("terrenia:material/stone", |id| id.as_str());
    let main = resources
        .material(material_id)
        .cloned()
        .unwrap_or(ResourceMaterial {
            color: [118, 124, 127],
            pattern: MaterialPattern::Grain,
            ..Default::default()
        });
    let wood = resources
        .material("terrenia:material/wood")
        .cloned()
        .unwrap_or(ResourceMaterial {
            color: [145, 105, 64],
            pattern: MaterialPattern::Wood,
            ..Default::default()
        });
    let mut geometry = Vec::new();
    if let Some(reference) = reference
        && let Some(builder) = world
            .get_resource::<Providers>()
            .and_then(|providers| providers.0.get(&reference.provider))
            .copied()
    {
        geometry.extend(
            builder(reference, &resources)
                .into_iter()
                .map(|part| (part.mesh, part.transform, part.material)),
        );
    } else if kind == "block" {
        let block = block.as_deref().unwrap_or(item);
        let layers = world
            .get_resource::<super::chunk_mesh::ProductionTerrainPalette>()
            .and_then(|palette| palette.model_face_layers(block));
        for face in latticeaxiom_voxel_mesh::Face::ALL {
            let layer = layers
                .as_ref()
                .map(|layers| layers[face.index()].as_str())
                .unwrap_or(block);
            let material = resources
                .material_for_layer(block, layer)
                .cloned()
                .unwrap_or_else(|| main.clone());
            geometry.push((
                face_mesh(face),
                Transform::from_scale(Vec3::splat(0.85)),
                material,
            ));
        }
    } else if matches!(kind, "pickaxe" | "axe" | "shovel" | "sword" | "hoe") {
        geometry.push((
            Mesh::from(Cuboid::new(
                0.105,
                if kind == "sword" { 0.30 } else { 0.88 },
                0.105,
            )),
            Transform::from_xyz(0.0, if kind == "sword" { -0.37 } else { -0.08 }, 0.0),
            wood.clone(),
        ));
        match kind {
            "pickaxe" | "hoe" => {
                geometry.push((
                    Mesh::from(Cuboid::new(0.63, 0.15, 0.18)),
                    Transform::from_xyz(0.0, 0.36, 0.0),
                    main.clone(),
                ));
                for side in [-1.0, 1.0] {
                    geometry.push((
                        Mesh::from(Cuboid::new(0.27, 0.1, 0.13)),
                        Transform::from_xyz(side * 0.37, 0.28, 0.0)
                            .with_rotation(Quat::from_rotation_z(-side * 0.5)),
                        main.clone(),
                    ));
                }
            }
            "axe" => geometry.push((
                prism(
                    &[[-0.08, -0.17], [0.4, -0.22], [0.45, 0.23], [-0.08, 0.16]],
                    0.15,
                ),
                Transform::from_xyz(0.0, 0.24, 0.0),
                main.clone(),
            )),
            "shovel" => geometry.push((
                prism(
                    &[
                        [-0.17, -0.17],
                        [0.17, -0.17],
                        [0.18, 0.1],
                        [0.0, 0.25],
                        [-0.18, 0.1],
                    ],
                    0.08,
                ),
                Transform::from_xyz(0.0, 0.38, 0.0),
                main.clone(),
            )),
            _ => {
                geometry.push((
                    prism(
                        &[
                            [-0.085, -0.25],
                            [0.085, -0.25],
                            [0.075, 0.38],
                            [0.0, 0.53],
                            [-0.075, 0.38],
                        ],
                        0.055,
                    ),
                    Transform::from_xyz(0.0, 0.05, 0.0),
                    main.clone(),
                ));
                geometry.push((
                    Mesh::from(Cuboid::new(0.34, 0.08, 0.12)),
                    Transform::from_xyz(0.0, -0.2, 0.0),
                    main.clone(),
                ));
            }
        }
        geometry.push((
            Mesh::from(Cuboid::new(0.16, 0.14, 0.16)),
            Transform::from_xyz(0.0, if kind == "sword" { -0.42 } else { 0.22 }, 0.0),
            wood,
        ));
    } else if matches!(kind, "grass-tuft" | "fern" | "reed") {
        let color = Color::srgb_u8(main.color[0], main.color[1], main.color[2]).to_linear();
        geometry.push((
            super::foliage::tuft_mesh([color.red, color.green, color.blue]),
            Transform::default(),
            ResourceMaterial {
                color: [255, 255, 255],
                pattern: MaterialPattern::Solid,
                ..Default::default()
            },
        ));
    } else if kind == "ingot" {
        geometry.push((
            prism(
                &[[-0.45, -0.16], [0.45, -0.16], [0.34, 0.16], [-0.34, 0.16]],
                0.33,
            ),
            Transform::default(),
            main,
        ));
    } else if kind == "stick" {
        geometry.push((
            Mesh::from(Cuboid::new(0.11, 0.9, 0.11)),
            Transform::from_rotation(Quat::from_rotation_z(-0.35)),
            wood,
        ));
    } else {
        geometry.push((
            Sphere::new(0.38).mesh().uv(7, 5),
            Transform::default(),
            main,
        ));
    }
    let parts = geometry
        .into_iter()
        .map(|(mesh, transform, style)| {
            let mut texture = Image::new_uninit(
                Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            );
            texture.data = Some(
                (0..16)
                    .flat_map(|y| {
                        (0..16).flat_map({
                            let style = style.clone();
                            move |x| style.texel(x, y)
                        })
                    })
                    .collect(),
            );
            texture.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                mag_filter: ImageFilterMode::Nearest,
                min_filter: ImageFilterMode::Linear,
                ..Default::default()
            });
            let texture = world.resource_mut::<Assets<Image>>().add(texture);
            let material = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial {
                    base_color_texture: Some(texture),
                    perceptual_roughness: f32::from(style.roughness.unwrap_or(220)) / 255.0,
                    metallic: f32::from(style.metallic.unwrap_or(0)) / 255.0,
                    alpha_mode: AlphaMode::Mask(0.5),
                    cull_mode: None,
                    double_sided: true,
                    ..Default::default()
                });
            Part {
                mesh: world.resource_mut::<Assets<Mesh>>().add(mesh),
                material,
                transform,
            }
        })
        .collect::<Vec<_>>();
    if models.parts.len() >= 256 {
        models.parts.pop_first();
    }
    models.parts.insert(item.to_owned(), parts.clone());
    parts
}

fn face_mesh(face: latticeaxiom_voxel_mesh::Face) -> Mesh {
    use latticeaxiom_voxel_mesh::Face;
    let positions = match face {
        Face::PosX => [
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, 0.5],
        ],
        Face::NegX => [
            [-0.5, -0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, 0.5, -0.5],
        ],
        Face::PosY => [
            [-0.5, 0.5, -0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
        ],
        Face::NegY => [
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
        ],
        Face::PosZ => [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ],
        Face::NegZ => [
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, -0.5, -0.5],
        ],
    };
    let uvs = positions.map(|p| match face {
        Face::PosX | Face::NegX => [p[2] + 0.5, p[1] + 0.5],
        Face::PosZ | Face::NegZ => [p[0] + 0.5, p[1] + 0.5],
        _ => [p[0] + 0.5, p[2] + 0.5],
    });
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![face.normal(); 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.to_vec());
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Built-in convex profiles have at most five vertices"
)]
fn prism(points: &[[f32; 2]], depth: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for sign in [-1.0, 1.0] {
        let base = positions.len() as u32;
        for p in points {
            positions.push([p[0], p[1], sign * depth / 2.0]);
            normals.push([0.0, 0.0, sign]);
            uvs.push([p[0] + 0.5, p[1] + 0.5]);
        }
        for i in 1..points.len() - 1 {
            let tri = [base, base + i as u32, base + i as u32 + 1];
            indices.extend(if sign > 0.0 {
                tri
            } else {
                [tri[0], tri[2], tri[1]]
            });
        }
    }
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let normal = Vec3::new(b[1] - a[1], a[0] - b[0], 0.0).normalize_or_zero();
        let base = positions.len() as u32;
        positions.extend([
            [a[0], a[1], -depth / 2.0],
            [b[0], b[1], -depth / 2.0],
            [b[0], b[1], depth / 2.0],
            [a[0], a[1], depth / 2.0],
        ]);
        normals.extend([normal.to_array(); 4]);
        uvs.extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn outward_triangles(mesh: &Mesh) {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals");
        };
        let indices = mesh
            .indices()
            .expect("triangles")
            .iter()
            .collect::<Vec<_>>();
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] =
                [triangle[0], triangle[1], triangle[2]].map(|i| Vec3::from(positions[i]));
            assert!(
                (b - a).cross(c - a).dot(Vec3::from(normals[triangle[0]])) > 0.0,
                "front winding agrees with surface lighting"
            );
        }
    }

    #[test]
    fn block_faces_and_tool_heads_have_outward_winding() {
        for face in latticeaxiom_voxel_mesh::Face::ALL {
            outward_triangles(&face_mesh(face));
        }
        outward_triangles(&prism(
            &[
                [-0.17, -0.17],
                [0.17, -0.17],
                [0.18, 0.1],
                [0.0, 0.25],
                [-0.18, 0.1],
            ],
            0.08,
        ));
    }
}
