//! Frozen schema, capability, and package identity strings.

use latticeaxiom_core::SchemaId;

use crate::InputError;

/// Exactly-one capability provided by `@latticeaxiom/input`.
pub const INPUT_ACTIONS_CAPABILITY: &str = "latticeaxiom:capability/input-actions@1";

/// Schema identity of a version-one action catalog document.
pub const ACTION_CATALOG_SCHEMA: &str = "latticeaxiom:schema/input-action-catalog@1";

/// Schema identity of a version-one physical binding.
pub const INPUT_BINDING_SCHEMA: &str = "latticeaxiom:schema/input-binding@1";

/// Schema identity of a version-one user binding profile.
pub const BINDING_PROFILE_SCHEMA: &str = "latticeaxiom:schema/binding-profile@1";

/// Logical package that ships the first-party catalog.
pub const INPUT_PACKAGE_NAME: &str = "@latticeaxiom/input";

/// Relative path of the shipped catalog inside the package data root.
pub const SHIPPED_ACTION_CATALOG_PACKAGE_PATH: &str = "data/action-catalog-v1.json";

/// Returns the contract major encoded in a versioned schema identifier.
pub(crate) fn schema_major(schema: &str, expected_path: &str) -> Result<u64, InputError> {
    let parsed = schema
        .parse::<SchemaId>()
        .map_err(|_| InputError::UnknownRequiredMajor {
            schema: schema.to_owned(),
            reason: "schema identifier is not a versioned Lattice schema ID",
        })?;
    if parsed.as_stable_id().path() != expected_path {
        return Err(InputError::UnknownRequiredMajor {
            schema: schema.to_owned(),
            reason: "schema path does not match the expected input contract",
        });
    }
    Ok(parsed.major().get())
}
