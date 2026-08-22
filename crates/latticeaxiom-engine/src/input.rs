//! Compile the lock-selected `@latticeaxiom/input` catalog into runtime maps.

use latticeaxiom_compose::{LockedGameGraph, SourceSnapshot};
use latticeaxiom_core::{CapabilityId, PackageName};
use latticeaxiom_input::{
    ActionCatalogDocumentV1, BindingProfileV1, CompiledInputCatalogV1, INPUT_ACTIONS_CAPABILITY,
    INPUT_PACKAGE_NAME, InputActionsProviderV1, InputError, SHIPPED_ACTION_CATALOG_PACKAGE_PATH,
    compile_input_catalog, select_exactly_one_input_actions_provider,
};
use latticeaxiom_packages::{CasObjectId, CasObjectKind, CasObjectStore};
use thiserror::Error;

use crate::LockVerifiedComposeImages;

/// Failure to select or compile the lock-selected input catalog.
#[derive(Debug, Error)]
pub enum HostInputError {
    /// Catalog selection or compile failed.
    #[error(transparent)]
    Input(#[from] InputError),
    /// The selected package artifact could not be read from CAS.
    #[error("input-actions catalog artifact is unavailable: {reason}")]
    CatalogUnavailable {
        /// Diagnostic.
        reason: String,
    },
}

/// Compiles the exactly-one input-actions provider selected by a reopened lock.
///
/// Returns `Ok(None)` when the graph does not declare the capability, so
/// synthetic headless fixtures that omit the provider remain usable. A declared
/// provider with a missing, duplicate, or mismatched catalog fails closed.
///
/// # Errors
///
/// Returns [`HostInputError`] when the graph selects the capability but the
/// catalog cannot be loaded or compiled.
pub fn compile_lock_selected_input<S: CasObjectStore>(
    images: &LockVerifiedComposeImages,
    store: Option<&S>,
    profile: &BindingProfileV1,
) -> Result<Option<CompiledInputCatalogV1>, HostInputError> {
    let graph = images.images().graph();
    let Some(providers) = graph_input_providers(graph) else {
        return Ok(None);
    };
    let catalog = load_provider_catalog(graph, store, &providers[0])?;
    let provider = InputActionsProviderV1::new(
        catalog.owner_package.clone(),
        catalog.capability.clone(),
        catalog,
    )?;
    let selected = select_exactly_one_input_actions_provider([provider])?;
    Ok(Some(compile_input_catalog([selected], profile)?))
}

/// Returns whether the reopened graph selected `@latticeaxiom/input`.
#[must_use]
pub fn graph_selects_input_actions(graph: &LockedGameGraph) -> bool {
    graph_input_providers(graph).is_some()
}

fn graph_input_providers(graph: &LockedGameGraph) -> Option<Vec<PackageName>> {
    let Ok(capability) = INPUT_ACTIONS_CAPABILITY.parse::<CapabilityId>() else {
        return None;
    };
    if let Some(packages) = graph.capability_providers.get(&capability)
        && !packages.is_empty()
    {
        return Some(packages.clone());
    }
    let package = graph
        .packages
        .keys()
        .find(|name| name.as_str() == INPUT_PACKAGE_NAME);
    package.map(|name| vec![name.clone()])
}

fn load_provider_catalog<S: CasObjectStore>(
    graph: &LockedGameGraph,
    store: Option<&S>,
    package: &PackageName,
) -> Result<ActionCatalogDocumentV1, HostInputError> {
    let Some(locked) = graph.packages.get(package) else {
        return Err(HostInputError::CatalogUnavailable {
            reason: format!("locked graph is missing package {package}"),
        });
    };
    let Some(store) = store else {
        return Err(HostInputError::CatalogUnavailable {
            reason: "catalog CAS is required to load the lock-selected input catalog".to_owned(),
        });
    };
    let id = CasObjectId::new(CasObjectKind::RealizedArtifact, locked.artifact_hash);
    let bytes = store
        .get(&id)
        .map_err(|error| HostInputError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    let snapshot: SourceSnapshot =
        serde_json::from_slice(&bytes).map_err(|error| HostInputError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    let file = snapshot
        .resolve_path(SHIPPED_ACTION_CATALOG_PACKAGE_PATH)
        .map_err(|error| HostInputError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    Ok(ActionCatalogDocumentV1::from_bytes(file.bytes())?)
}
