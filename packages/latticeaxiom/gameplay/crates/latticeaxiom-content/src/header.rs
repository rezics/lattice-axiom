use std::num::NonZeroU32;

use latticeaxiom_core::{PackageName, SchemaId, StableId};
use serde::{Deserialize, Serialize};

use crate::{ContentError, ContentResult};

/// Canonical schema ID for [`crate::BlockDefinitionV1`].
pub const BLOCK_DEFINITION_SCHEMA_V1: &str = "latticeaxiom:schema/block-definition@1";
/// Canonical schema ID for [`crate::FluidDefinitionV1`].
pub const FLUID_DEFINITION_SCHEMA_V1: &str = "latticeaxiom:schema/fluid-definition@1";
/// Canonical schema ID for [`crate::FluidStateV1`].
pub const FLUID_STATE_SCHEMA_V1: &str = "latticeaxiom:schema/fluid-state@1";
/// Canonical schema ID for [`crate::BiomeDefinitionV1`].
pub const BIOME_DEFINITION_SCHEMA_V1: &str = "latticeaxiom:schema/biome-definition@1";

/// Positive, owner-managed revision of one content definition.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ContentRevisionV1(NonZeroU32);

impl ContentRevisionV1 {
    /// Creates a positive content revision.
    #[must_use]
    pub const fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    /// Returns the positive revision number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Owner-aware header shared by every v1 content definition.
///
/// Namespace grants are verified by the registration compiler. This crate
/// preserves the declaring package and validates the exact identity and schema
/// kinds without inferring ownership from either string.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentHeaderV1 {
    /// Exact content identity.
    pub stable_id: StableId,
    /// Versioned schema identity, including its positive major.
    pub schema_id_and_major: SchemaId,
    /// Owner-managed output-affecting revision.
    pub content_revision: ContentRevisionV1,
    /// Logical package that declared the definition.
    pub declared_by_package: PackageName,
}

pub(crate) fn validate_header(
    header: &ContentHeaderV1,
    expected_kind: &'static str,
    expected_schema: &'static str,
) -> ContentResult<()> {
    validate_exact_id(&header.stable_id, expected_kind, expected_kind)?;
    if header.schema_id_and_major.as_str() != expected_schema {
        return Err(ContentError::WrongDefinitionSchema {
            kind: expected_kind,
            id: header.stable_id.clone(),
            expected: expected_schema,
            actual: header.schema_id_and_major.to_string(),
        });
    }
    Ok(())
}

pub(crate) fn validate_exact_id(
    id: &StableId,
    expected_kind: &'static str,
    context: &'static str,
) -> ContentResult<()> {
    if id.kind() != expected_kind {
        return Err(ContentError::WrongIdentityKind {
            context,
            id: id.clone(),
            expected: expected_kind,
            actual: id.kind().to_owned(),
        });
    }
    if id.major().is_some() {
        return Err(ContentError::ExactIdentityHasMajor {
            context,
            id: id.clone(),
        });
    }
    Ok(())
}

pub(crate) fn validate_contract_id(
    id: &StableId,
    expected_kind: &'static str,
    context: &'static str,
) -> ContentResult<()> {
    if id.kind() != expected_kind {
        return Err(ContentError::WrongIdentityKind {
            context,
            id: id.clone(),
            expected: expected_kind,
            actual: id.kind().to_owned(),
        });
    }
    if id.major().is_none() {
        return Err(ContentError::ContractIdentityMissingMajor {
            context,
            id: id.clone(),
        });
    }
    Ok(())
}

pub(crate) fn validate_optional_exact_reference(
    id: Option<&StableId>,
    expected_kind: &'static str,
    context: &'static str,
) -> ContentResult<()> {
    if let Some(id) = id {
        validate_exact_id(id, expected_kind, context)?;
    }
    Ok(())
}
