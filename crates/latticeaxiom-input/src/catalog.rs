//! Package-selected action catalog documents and exactly-one provider selection.

use std::{collections::BTreeMap, str::FromStr};

use latticeaxiom_core::{CapabilityId, PackageName, StableId};
use serde::{Deserialize, Serialize};

use crate::{
    ActionSpecV1, AuthoritativePlayerActionV1, ClientSurfaceActionV1, InputError,
    ids::{self, INPUT_ACTIONS_CAPABILITY},
};

/// Authored catalog document shipped by an input-actions provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionCatalogDocumentV1 {
    /// Versioned schema identifier.
    pub schema: String,
    /// Package that owns every action row.
    pub owner_package: PackageName,
    /// Exactly-one capability this catalog satisfies.
    pub capability: CapabilityId,
    /// Authored action rows. Compile sorts by stable ID.
    pub actions: Vec<ActionSpecV1>,
}

impl ActionCatalogDocumentV1 {
    /// Parses catalog bytes without compiling bindings.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when JSON, schema, identity, or action rows fail.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InputError> {
        let document: Self =
            serde_json::from_slice(bytes).map_err(|error| InputError::InvalidCatalog {
                reason: error.to_string(),
            })?;
        document.validate_identities()?;
        Ok(document)
    }

    fn validate_identities(&self) -> Result<(), InputError> {
        let major = ids::schema_major(&self.schema, "input-action-catalog")?;
        if major != 1 {
            return Err(InputError::UnknownRequiredMajor {
                schema: self.schema.clone(),
                reason: "unknown required action-catalog major",
            });
        }
        if self.capability.as_str() != INPUT_ACTIONS_CAPABILITY {
            return Err(InputError::InvalidCatalog {
                reason: format!(
                    "catalog capability `{}` is not {INPUT_ACTIONS_CAPABILITY}",
                    self.capability
                ),
            });
        }
        for spec in &self.actions {
            spec.validate()?;
        }
        Ok(())
    }
}

/// One graph-selected input-actions provider and its catalog document.
#[derive(Clone, Debug, PartialEq)]
pub struct InputActionsProviderV1 {
    /// Providing package.
    pub package: PackageName,
    /// Declared capability.
    pub capability: CapabilityId,
    /// Catalog document supplied by that package.
    pub catalog: ActionCatalogDocumentV1,
}

impl InputActionsProviderV1 {
    /// Creates a provider row from a parsed catalog.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the package or capability does not match the
    /// catalog document.
    pub fn new(
        package: PackageName,
        capability: CapabilityId,
        catalog: ActionCatalogDocumentV1,
    ) -> Result<Self, InputError> {
        if package != catalog.owner_package {
            return Err(InputError::InvalidCatalog {
                reason: format!(
                    "provider package `{package}` does not own catalog package `{}`",
                    catalog.owner_package
                ),
            });
        }
        if capability != catalog.capability {
            return Err(InputError::InvalidCatalog {
                reason: format!(
                    "provider capability `{capability}` does not match catalog `{}`",
                    catalog.capability
                ),
            });
        }
        Ok(Self {
            package,
            capability,
            catalog,
        })
    }
}

/// Selects the exactly-one input-actions provider from graph candidates.
///
/// Candidate order cannot change a successful selection.
///
/// # Errors
///
/// Returns [`InputError::MissingProvider`] or [`InputError::DuplicateProviders`].
pub fn select_exactly_one_input_actions_provider<I>(
    providers: I,
) -> Result<InputActionsProviderV1, InputError>
where
    I: IntoIterator<Item = InputActionsProviderV1>,
{
    let mut by_package = BTreeMap::new();
    for provider in providers {
        if provider.capability.as_str() != INPUT_ACTIONS_CAPABILITY {
            return Err(InputError::InvalidCatalog {
                reason: format!(
                    "provider `{}` declared `{}`",
                    provider.package, provider.capability
                ),
            });
        }
        if by_package
            .insert(provider.package.clone(), provider)
            .is_some()
        {
            // Duplicate package identity is still a duplicate provider.
        }
    }
    match by_package.len() {
        0 => Err(InputError::MissingProvider),
        1 => match by_package.into_values().next() {
            Some(provider) => Ok(provider),
            None => Err(InputError::MissingProvider),
        },
        _ => Err(InputError::DuplicateProviders {
            packages: by_package.keys().cloned().collect(),
        }),
    }
}

pub(crate) fn assert_static_enum_goldens(actions: &[ActionSpecV1]) -> Result<(), InputError> {
    let mut surface = BTreeMap::new();
    let mut player = BTreeMap::new();
    for spec in actions {
        if let Some(action) = spec.client_surface_action
            && surface.insert(action, spec.id.clone()).is_some()
        {
            return Err(InputError::CatalogEnumMismatch {
                detail: format!("duplicate client surface mapping for {action:?}"),
            });
        }
        if let Some(action) = spec.authoritative_player_action
            && player.insert(action, spec.id.clone()).is_some()
        {
            return Err(InputError::CatalogEnumMismatch {
                detail: format!("duplicate player mapping for {action:?}"),
            });
        }
    }
    for action in ClientSurfaceActionV1::ALL {
        match surface.get(&action) {
            None => {
                return Err(InputError::CatalogEnumMismatch {
                    detail: format!("catalog is missing {}", action.stable_id()),
                });
            }
            Some(id) if id.as_str() != action.stable_id() => {
                return Err(InputError::CatalogEnumMismatch {
                    detail: format!("catalog id {id} does not match {}", action.stable_id()),
                });
            }
            Some(_) => {}
        }
    }
    for action in AuthoritativePlayerActionV1::ALL {
        match player.get(&action) {
            None => {
                return Err(InputError::CatalogEnumMismatch {
                    detail: format!("catalog is missing {}", action.stable_id()),
                });
            }
            Some(id) if id.as_str() != action.stable_id() => {
                return Err(InputError::CatalogEnumMismatch {
                    detail: format!("catalog id {id} does not match {}", action.stable_id()),
                });
            }
            Some(_) => {}
        }
    }
    Ok(())
}

pub(crate) fn unique_sorted_actions(
    actions: Vec<ActionSpecV1>,
) -> Result<Vec<ActionSpecV1>, InputError> {
    let mut by_id = BTreeMap::<StableId, ActionSpecV1>::new();
    for spec in actions {
        let id = spec.id.clone();
        if by_id.insert(id.clone(), spec).is_some() {
            return Err(InputError::DuplicateAction { action: id });
        }
    }
    Ok(by_id.into_values().collect())
}

/// Parses a package name, mapping identifier errors into catalog errors.
///
/// # Errors
///
/// Returns [`InputError`] when `value` is not a package name.
pub fn parse_package_name(value: &str) -> Result<PackageName, InputError> {
    PackageName::from_str(value).map_err(|error| InputError::InvalidCatalog {
        reason: error.to_string(),
    })
}
