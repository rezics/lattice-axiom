//! A separately locked client appearance graph; never part of world persistence.

use std::path::{Path, PathBuf};

use bevy::prelude::Resource;
use latticeaxiom_render_contracts::{ResolvedResourcePacks, ResourcePackV1};

use crate::{EngineInstance, ProductionClientError, load_lock_verified_images_from};

/// Runtime-only presentation overlay, independent of the saved world's lock.
#[derive(Clone, Debug, Default, Resource)]
pub(crate) struct ClientResourcePacks(pub ResolvedResourcePacks);

pub(crate) fn install_client_resource_packs(
    instance: &mut EngineInstance,
    workspace: &Path,
) -> Result<(), ProductionClientError> {
    let explicit = std::env::var_os("LATTICEAXIOM_RESOURCE_LOCK");
    let path = explicit.as_ref().map_or_else(
        || workspace.join("run/client-resources/latticeaxiom.lock"),
        PathBuf::from,
    );
    let mut resolved = ResolvedResourcePacks::default();
    if path.exists() || explicit.is_some() {
        let images = load_lock_verified_images_from(workspace, &path)?;
        for package in images.locked_artifacts().data_package_names() {
            let root = images
                .locked_artifacts()
                .data_root(package)
                .map_err(|error| resource_error(error.to_string()))?;
            let descriptor = root
                .files()
                .find(|(path, _)| path.as_str() == "data/resource-pack.json")
                .map(|(_, bytes)| bytes)
                .ok_or_else(|| resource_error(format!("{package} has no resource descriptor")))?;
            let pack: ResourcePackV1 = serde_json::from_slice(descriptor)
                .map_err(|error| resource_error(error.to_string()))?;
            let shader = pack
                .water_shader
                .as_ref()
                .map(|path| {
                    let bytes = root
                        .files()
                        .find(|(candidate, _)| *candidate == path)
                        .map(|(_, bytes)| bytes)
                        .ok_or_else(|| resource_error(format!("{package} is missing {path}")))?;
                    std::str::from_utf8(bytes).map_err(|error| resource_error(error.to_string()))
                })
                .transpose()?;
            resolved
                .apply(&pack, shader)
                .map_err(|error| resource_error(error.to_string()))?;
        }
        bevy::log::info!(resource_lock = %images.product_lock_hash(), "client resource graph loaded");
    }
    if let Some(shader) = resolved.water_shader() {
        crate::host::install_resource_water_shader(&mut instance.app, shader)
            .map_err(resource_error)?;
    }
    instance.app.insert_resource(ClientResourcePacks(resolved));
    Ok(())
}

fn resource_error(reason: String) -> ProductionClientError {
    ProductionClientError::ResourcePack { reason }
}
