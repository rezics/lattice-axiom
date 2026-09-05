//! Compile the lock-selected input-actions catalog into runtime maps.

use latticeaxiom_compose::{LockedGameGraph, RealizedDataRootV1};
use latticeaxiom_core::{CanonicalLogicalPath, CapabilityId, PackageName};
use latticeaxiom_input::{
    ActionCatalogDocumentV1, BindingProfileV1, CompiledInputCatalogV1, INPUT_ACTIONS_CAPABILITY,
    InputActionsProviderV1, InputError, SHIPPED_ACTION_CATALOG_PACKAGE_PATH, compile_input_catalog,
};
use thiserror::Error;

use crate::LockVerifiedComposeImages;

/// Failure to select or compile the lock-selected input catalog.
#[derive(Debug, Error)]
pub enum HostInputError {
    /// Catalog selection or compile failed.
    #[error(transparent)]
    Input(#[from] InputError),
    /// The selected provider artifact or required catalog file was unavailable.
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
pub fn compile_lock_selected_input(
    images: &LockVerifiedComposeImages,
    profile: &BindingProfileV1,
) -> Result<Option<CompiledInputCatalogV1>, HostInputError> {
    let graph = images.images().graph();
    let Some(package) = graph_input_provider(graph)? else {
        return Ok(None);
    };
    let data_root = images
        .locked_artifacts()
        .data_root(&package)
        .map_err(|error| HostInputError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    Ok(Some(compile_data_root_input(
        &package,
        data_root.as_ref(),
        profile,
    )?))
}

/// Returns whether the reopened graph declares the input-actions capability.
#[must_use]
pub fn graph_selects_input_actions(graph: &LockedGameGraph) -> bool {
    INPUT_ACTIONS_CAPABILITY
        .parse::<CapabilityId>()
        .is_ok_and(|capability| graph.capability_providers.contains_key(&capability))
}

fn graph_input_provider(graph: &LockedGameGraph) -> Result<Option<PackageName>, HostInputError> {
    let capability = INPUT_ACTIONS_CAPABILITY
        .parse::<CapabilityId>()
        .map_err(|error| HostInputError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    select_declared_input_provider(
        graph
            .capability_providers
            .get(&capability)
            .map(Vec::as_slice),
        |package| graph.packages.contains_key(package),
    )
}

fn select_declared_input_provider(
    providers: Option<&[PackageName]>,
    package_exists: impl Fn(&PackageName) -> bool,
) -> Result<Option<PackageName>, HostInputError> {
    let Some(providers) = providers else {
        return Ok(None);
    };
    match providers {
        [] => Err(InputError::MissingProvider.into()),
        [package] => {
            if !package_exists(package) {
                return Err(HostInputError::CatalogUnavailable {
                    reason: format!(
                        "locked graph is missing selected input-actions provider `{package}`"
                    ),
                });
            }
            Ok(Some(package.clone()))
        }
        _ => {
            let mut packages = providers.to_vec();
            packages.sort();
            Err(InputError::DuplicateProviders { packages }.into())
        }
    }
}

fn compile_data_root_input(
    package: &PackageName,
    data_root: &RealizedDataRootV1,
    profile: &BindingProfileV1,
) -> Result<CompiledInputCatalogV1, HostInputError> {
    let catalog_path =
        CanonicalLogicalPath::new(SHIPPED_ACTION_CATALOG_PACKAGE_PATH).map_err(|error| {
            HostInputError::CatalogUnavailable {
                reason: error.to_string(),
            }
        })?;
    let bytes = data_root.require_file(&catalog_path).map_err(|error| {
        HostInputError::CatalogUnavailable {
            reason: error.to_string(),
        }
    })?;
    let catalog = ActionCatalogDocumentV1::from_bytes(bytes)?;
    let capability = INPUT_ACTIONS_CAPABILITY
        .parse::<CapabilityId>()
        .map_err(|error| HostInputError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    let provider = InputActionsProviderV1::new(package.clone(), capability, catalog)?;
    Ok(compile_input_catalog([provider], profile)?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use latticeaxiom_compose::RealizedDataRootV1;
    use latticeaxiom_core::{CanonicalLogicalPath, PackageName};
    use latticeaxiom_input::{
        ActionCatalogDocumentV1, BindingProfileV1, INPUT_PACKAGE_NAME,
        SHIPPED_ACTION_CATALOG_PACKAGE_PATH,
    };

    use super::{HostInputError, compile_data_root_input, select_declared_input_provider};

    const SHIPPED_CATALOG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../latticeaxiom/input/data/action-catalog-v1.json"
    ));

    fn package(value: &str) -> PackageName {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture package is valid: {error}"))
    }

    fn path(value: &str) -> CanonicalLogicalPath {
        CanonicalLogicalPath::new(value)
            .unwrap_or_else(|error| panic!("fixture path is valid: {error}"))
    }

    fn catalog_bytes_for(owner: &PackageName) -> Vec<u8> {
        let mut catalog = ActionCatalogDocumentV1::from_bytes(SHIPPED_CATALOG)
            .unwrap_or_else(|error| panic!("shipped fixture catalog is valid: {error}"));
        catalog.owner_package.clone_from(owner);
        serde_json::to_vec(&catalog)
            .unwrap_or_else(|error| panic!("fixture catalog serializes: {error}"))
    }

    fn data_root(
        package: &PackageName,
        files: BTreeMap<CanonicalLogicalPath, Vec<u8>>,
    ) -> RealizedDataRootV1 {
        RealizedDataRootV1::new(package.clone(), path("data"), files)
            .unwrap_or_else(|error| panic!("fixture data root is valid: {error}"))
    }

    #[test]
    fn absent_capability_does_not_fall_back_to_first_party_package() {
        let first_party = package(INPUT_PACKAGE_NAME);
        let selected = select_declared_input_provider(None, |candidate| candidate == &first_party)
            .unwrap_or_else(|error| panic!("absent capability is allowed: {error}"));

        assert_eq!(selected, None);
    }

    #[test]
    fn declared_capability_requires_exactly_one_provider() {
        let first_party = package(INPUT_PACKAGE_NAME);
        let selected =
            select_declared_input_provider(Some(std::slice::from_ref(&first_party)), |_| true)
                .unwrap_or_else(|error| panic!("one provider is accepted: {error}"));
        assert_eq!(selected, Some(first_party.clone()));

        let missing = select_declared_input_provider(Some(&[]), |_| true);
        assert!(matches!(
            missing,
            Err(HostInputError::Input(
                latticeaxiom_input::InputError::MissingProvider
            ))
        ));

        let other = package("@example/other-input");
        let duplicate =
            select_declared_input_provider(Some(&[first_party.clone(), other.clone()]), |_| true);
        assert!(matches!(
            duplicate,
            Err(HostInputError::Input(
                latticeaxiom_input::InputError::DuplicateProviders { packages }
            )) if packages == vec![other, first_party]
        ));
    }

    #[test]
    fn capability_can_select_a_substitute_provider() {
        let substitute = package("@example/input-substitute");
        let selected =
            select_declared_input_provider(Some(std::slice::from_ref(&substitute)), |_| true)
                .unwrap_or_else(|error| panic!("substitute provider is accepted: {error}"));

        assert_eq!(selected, Some(substitute));
    }

    #[test]
    fn declared_provider_must_exist_in_the_locked_graph() {
        let missing = package("@example/missing-input");
        let result =
            select_declared_input_provider(Some(std::slice::from_ref(&missing)), |_| false);

        assert!(matches!(
            result,
            Err(HostInputError::CatalogUnavailable { reason })
                if reason.contains(missing.as_str())
        ));
    }

    #[test]
    fn realized_data_root_round_trip_compiles_substitute_catalog() {
        let substitute = package("@example/input-substitute");
        let catalog_path = path(SHIPPED_ACTION_CATALOG_PACKAGE_PATH);
        let root = data_root(
            &substitute,
            BTreeMap::from([(catalog_path, catalog_bytes_for(&substitute))]),
        );
        let bytes = root
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("fixture data root encodes: {error}"));
        let decoded = RealizedDataRootV1::from_canonical_bytes_for_package(&bytes, &substitute)
            .unwrap_or_else(|error| panic!("fixture data root decodes: {error}"));

        let compiled = compile_data_root_input(&substitute, &decoded, &BindingProfileV1::empty())
            .unwrap_or_else(|error| panic!("substitute catalog compiles: {error}"));
        assert_eq!(compiled.provider(), &substitute);
    }

    #[test]
    fn realized_data_root_requires_the_canonical_catalog_path() {
        let provider = package(INPUT_PACKAGE_NAME);
        let root = data_root(
            &provider,
            BTreeMap::from([(path("data/other.json"), catalog_bytes_for(&provider))]),
        );

        let result = compile_data_root_input(&provider, &root, &BindingProfileV1::empty());
        assert!(matches!(
            result,
            Err(HostInputError::CatalogUnavailable { reason })
                if reason.contains(SHIPPED_ACTION_CATALOG_PACKAGE_PATH)
        ));
    }

    #[test]
    fn catalog_owner_must_match_the_lock_selected_provider() {
        let selected = package("@example/input-substitute");
        let first_party = package(INPUT_PACKAGE_NAME);
        let root = data_root(
            &selected,
            BTreeMap::from([(
                path(SHIPPED_ACTION_CATALOG_PACKAGE_PATH),
                catalog_bytes_for(&first_party),
            )]),
        );

        let result = compile_data_root_input(&selected, &root, &BindingProfileV1::empty());
        assert!(matches!(
            result,
            Err(HostInputError::Input(
                latticeaxiom_input::InputError::InvalidCatalog { reason }
            )) if reason.contains("does not own catalog package")
        ));
    }
}
