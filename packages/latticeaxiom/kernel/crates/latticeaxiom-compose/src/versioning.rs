//! Compatibility-coordinate ownership catalog.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Independent compatibility coordinates used by Lattice Axiom artifacts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionCoordinateKind {
    /// User-facing product release line.
    ProductRelease,
    /// Logical package API and behavior version.
    Package,
    /// Negotiated capability interface version.
    Capability,
    /// Portable native bootstrap or table ABI.
    PortableAbi,
    /// Exact engine build for engine-coupled code.
    EngineBuild,
    /// Frozen package closure and artifact receipt.
    PackageLock,
    /// Exact Rust and Bevy host dependency closure.
    CargoBevyLock,
    /// Stable content identity plus owner revision.
    ContentRevision,
    /// Semantic contract stable ID plus major.
    SemanticContract,
    /// Concrete Role binding and active bundle set.
    RoleBinding,
    /// Persistent schema identity and owner version.
    Schema,
    /// World-generator algorithm revision and configuration hash.
    Generator,
    /// Asset format or semantic importer version.
    AssetFormat,
}

/// Authority responsible for one compatibility coordinate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionOwnerClass {
    /// Product release engineering.
    Release,
    /// Logical package owner.
    PackageOwner,
    /// Stable contract or capability owner.
    ContractOwner,
    /// Core host build.
    CoreBuild,
    /// Deterministic package kernel.
    PackageKernel,
    /// Cargo workspace lock owner.
    CargoWorkspace,
    /// Content registration owner.
    ContentOwner,
    /// Active composition or frozen world lock.
    CompositionWorld,
    /// Persistent schema and migration owner.
    SchemaOwner,
    /// World-generator owner.
    GeneratorOwner,
    /// Asset format or importer owner.
    AssetOwner,
}

/// Documentation carried with one version coordinate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionCoordinateDefinition {
    /// Coordinate described by this row.
    pub kind: VersionCoordinateKind,
    /// Authority allowed to evolve it.
    pub owner: VersionOwnerClass,
    /// Compatibility question answered by this coordinate.
    pub compatibility_scope: String,
    /// Important compatibility question this coordinate cannot answer.
    pub excluded_scope: String,
}

/// Returns the normative R0 compatibility-coordinate ownership catalog.
#[must_use]
pub fn version_coordinate_catalog() -> BTreeMap<VersionCoordinateKind, VersionCoordinateDefinition>
{
    use VersionCoordinateKind as Kind;
    use VersionOwnerClass as Owner;

    [
        row(
            Kind::ProductRelease,
            Owner::Release,
            "which supported product release the user obtained",
            "package or save compatibility",
        ),
        row(
            Kind::Package,
            Owner::PackageOwner,
            "logical package API, behavior, and dependency ranges",
            "registration namespace, binary layout, or exact artifact",
        ),
        row(
            Kind::Capability,
            Owner::ContractOwner,
            "which provider and consumer interface versions can negotiate",
            "package source or persisted schema",
        ),
        row(
            Kind::PortableAbi,
            Owner::ContractOwner,
            "whether native tables, callbacks, and batch layouts can exchange",
            "gameplay behavior",
        ),
        row(
            Kind::EngineBuild,
            Owner::CoreBuild,
            "whether an engine-coupled artifact targets this exact host",
            "portable artifact compatibility",
        ),
        row(
            Kind::PackageLock,
            Owner::PackageKernel,
            "the exact selected closure, sources, and artifacts",
            "how an old world record migrates",
        ),
        row(
            Kind::CargoBevyLock,
            Owner::CargoWorkspace,
            "the exact Rust and Bevy host dependency closure",
            "external package resolution",
        ),
        row(
            Kind::ContentRevision,
            Owner::ContentOwner,
            "the identity and revision of a content registration",
            "code artifact identity",
        ),
        row(
            Kind::SemanticContract,
            Owner::ContractOwner,
            "how a Tag, Map, Affordance, or Predicate contract is interpreted",
            "the current concrete members or provider artifact",
        ),
        row(
            Kind::RoleBinding,
            Owner::CompositionWorld,
            "which concrete content and fallback bundles this closure selected",
            "whether materialized voxels should be rewritten",
        ),
        row(
            Kind::Schema,
            Owner::SchemaOwner,
            "how a record or component decodes and migrates",
            "package dependency resolution",
        ),
        row(
            Kind::Generator,
            Owner::GeneratorOwner,
            "which algorithm and parameters govern unmaterialized space",
            "whether materialized snapshots are regenerated",
        ),
        row(
            Kind::AssetFormat,
            Owner::AssetOwner,
            "how an importer interprets asset bytes and sidecars",
            "gameplay ABI compatibility",
        ),
    ]
    .into_iter()
    .map(|definition| (definition.kind, definition))
    .collect()
}

fn row(
    kind: VersionCoordinateKind,
    owner: VersionOwnerClass,
    compatibility_scope: &'static str,
    excluded_scope: &'static str,
) -> VersionCoordinateDefinition {
    VersionCoordinateDefinition {
        kind,
        owner,
        compatibility_scope: compatibility_scope.to_owned(),
        excluded_scope: excluded_scope.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_coordinate_has_one_owner_and_scope() {
        let catalog = version_coordinate_catalog();
        assert_eq!(catalog.len(), 13);
        assert!(catalog.iter().all(|(key, row)| {
            key == &row.kind
                && !row.compatibility_scope.is_empty()
                && !row.excluded_scope.is_empty()
        }));
    }
}
