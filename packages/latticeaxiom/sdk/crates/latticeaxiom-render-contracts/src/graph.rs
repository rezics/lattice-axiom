#![allow(
    clippy::result_large_err,
    reason = "activation-only typed diagnostics retain complete stable IDs"
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use latticeaxiom_core::StableId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Core3dSlotV1, RenderDataDecl, RenderFeatureDecl};

/// Schema major emitted by the first render-plan compiler.
pub const RENDER_PLAN_SCHEMA_MAJOR: u32 = 1;

/// A logical render-resource identity plus its SSA version.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderResourceVersion {
    /// Stable logical resource identity.
    pub resource: StableId,
    /// Logical SSA version local to the resource identity.
    pub version: u32,
}

impl fmt::Display for RenderResourceVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}#{}", self.resource, self.version)
    }
}

/// Portable logical resource kinds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderResourceKind {
    /// A logical texture resource.
    Texture,
    /// A logical buffer resource.
    Buffer,
}

/// Texture formats frozen by render contract major 1.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextureFormatV1 {
    /// 32-bit floating-point depth.
    #[serde(rename = "depth32float")]
    Depth32Float,
    /// Four-channel 16-bit floating-point color.
    #[serde(rename = "rgba16float")]
    Rgba16Float,
    /// Single-channel normalized unsigned 8-bit data.
    #[serde(rename = "r8unorm")]
    R8Unorm,
}

/// Portable logical resource formats.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RenderResourceFormat {
    /// A texture with a contract-major-1 format.
    Texture {
        /// Required portable texture format.
        format: TextureFormatV1,
    },
    /// A buffer whose element stride is known before activation.
    Buffer {
        /// Non-zero byte stride of one logical element.
        stride: u32,
    },
}

/// The semantic ownership scope of a logical resource.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RenderResourceScope {
    /// A resource scoped to one view.
    View,
    /// A resource shared for one presentation frame.
    Frame,
    /// A resource private to one declared feature.
    Feature {
        /// Feature that owns the resource scope.
        feature: StableId,
    },
}

/// Logical lifetime used by the host to derive allocation and aliasing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderResourceLifetime {
    /// Host-owned input imported at a semantic slot.
    Imported,
    /// Plan-local derived data whose physical storage may be aliased safely.
    Transient,
    /// Rebuildable presentation data retained across frames.
    PersistentPresentation,
}

/// Portable resource usages frozen by render contract major 1.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderResourceUsage {
    /// Sampled texture read.
    Sampled,
    /// Storage resource read.
    StorageRead,
    /// Storage resource write.
    StorageWrite,
    /// Color-attachment write.
    ColorAttachment,
    /// Depth texture read.
    DepthRead,
    /// Uniform-buffer read.
    Uniform,
    /// Host upload destination write.
    UploadDestination,
}

impl RenderResourceUsage {
    const fn is_read(self) -> bool {
        matches!(
            self,
            Self::Sampled | Self::StorageRead | Self::DepthRead | Self::Uniform
        )
    }

    const fn is_write(self) -> bool {
        matches!(
            self,
            Self::StorageWrite | Self::ColorAttachment | Self::UploadDestination
        )
    }
}

/// Portable extent policy; the host chooses the physical extent from this intent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ExtentPolicy {
    /// Match the active view extent.
    View,
    /// A fixed three-dimensional extent.
    Fixed {
        /// Width in texels or elements.
        width: u32,
        /// Height in texels or elements.
        height: u32,
        /// Depth in texels or elements.
        depth: u32,
    },
    /// A host-sized one-dimensional element count.
    Elements,
}

/// The unique logical producer of an SSA resource version.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RenderResourceProducer {
    /// A host-owned imported value established at a semantic slot.
    HostSlot {
        /// Earliest slot at which the value exists.
        slot: Core3dSlotV1,
    },
    /// A value written by one feature-owned pass.
    FeaturePass {
        /// Stable identity of the writer pass.
        pass: StableId,
    },
}

/// One logical SSA resource declaration.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderResourceDecl {
    /// Logical resource and SSA version.
    pub id: RenderResourceVersion,
    /// Logical resource kind.
    pub kind: RenderResourceKind,
    /// Semantic ownership scope.
    pub scope: RenderResourceScope,
    /// Logical lifetime.
    pub lifetime: RenderResourceLifetime,
    /// Portable format requirement.
    pub format: RenderResourceFormat,
    /// Non-zero power-of-two sample count; buffers require one.
    pub sample_count: u32,
    /// Logical extent policy.
    pub extent: ExtentPolicy,
    /// Complete declared usage set.
    pub usages: BTreeSet<RenderResourceUsage>,
    /// Unique producer.
    pub producer: RenderResourceProducer,
}

/// A pass read or write with compile-time compatibility expectations.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PassResourceUse {
    /// Referenced logical SSA version.
    pub resource: RenderResourceVersion,
    /// Usage exercised by this access.
    pub usage: RenderResourceUsage,
    /// Expected logical kind.
    pub kind: RenderResourceKind,
    /// Expected portable format.
    pub format: RenderResourceFormat,
    /// Expected sample count.
    pub sample_count: u32,
    /// Expected semantic scope.
    pub scope: RenderResourceScope,
}

/// A feature-owned execution node in one semantic slot.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderPassDecl {
    /// Stable identity of this pass.
    pub id: StableId,
    /// Feature that owns the pass.
    pub feature: StableId,
    /// Semantic execution slot.
    pub slot: Core3dSlotV1,
    /// Explicit logical reads.
    pub reads: Vec<PassResourceUse>,
    /// Explicit new SSA versions written by this pass.
    pub writes: Vec<PassResourceUse>,
    /// Passes that must complete before this pass.
    pub after: BTreeSet<StableId>,
    /// Passes that must execute after this pass.
    pub before: BTreeSet<StableId>,
}

/// Untrusted declarations supplied to the deterministic graph compiler.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderPlanInput {
    /// Requested render-plan schema major.
    pub schema_major: u32,
    /// Typed presentation data registrations.
    pub data: Vec<RenderDataDecl>,
    /// Additive feature registrations.
    pub features: Vec<RenderFeatureDecl>,
    /// Feature-owned pass registrations.
    pub passes: Vec<RenderPassDecl>,
    /// Logical SSA resource versions.
    pub resources: Vec<RenderResourceDecl>,
}

/// A pass in deterministic execution order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledPass {
    /// Stable zero-based position in the compiled plan.
    pub ordinal: u32,
    /// Validated source declaration.
    pub pass: RenderPassDecl,
}

/// A normalized, deterministic logical render plan.
///
/// All vectors are sorted by stable identity except `passes`, which is a stable
/// topological order with stable-ID tie-breaking.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledRenderPlan {
    /// Render-plan schema major.
    pub schema_major: u32,
    /// Sorted presentation data registrations.
    pub data: Vec<RenderDataDecl>,
    /// Sorted feature registrations.
    pub features: Vec<RenderFeatureDecl>,
    /// Stable topological pass order.
    pub passes: Vec<CompiledPass>,
    /// Sorted logical SSA resources.
    pub resources: Vec<RenderResourceDecl>,
}

/// Pure compiler for logical render declarations.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderGraphCompiler;

impl RenderGraphCompiler {
    /// Compiles and validates a deterministic logical render plan.
    ///
    /// # Errors
    ///
    /// Returns [`GraphCompileError`] for schema mismatches, duplicate identities,
    /// missing producers, incompatible resource uses, invalid slot ordering, or
    /// dependency cycles.
    #[allow(
        clippy::too_many_lines,
        reason = "one deterministic compiler transaction keeps validation order explicit"
    )]
    pub fn compile(input: &RenderPlanInput) -> Result<CompiledRenderPlan, GraphCompileError> {
        if input.schema_major != RENDER_PLAN_SCHEMA_MAJOR {
            return Err(GraphCompileError::UnsupportedSchema {
                actual: input.schema_major,
            });
        }

        let mut data = input.data.clone();
        data.sort_by(|left, right| left.id.cmp(&right.id));
        reject_duplicate_ids(
            data.iter().map(|entry| &entry.id),
            GraphCompileError::DuplicateData,
        )?;

        let mut features = input.features.clone();
        features.sort_by(|left, right| left.id.cmp(&right.id));
        reject_duplicate_ids(
            features.iter().map(|entry| &entry.id),
            GraphCompileError::DuplicateFeature,
        )?;
        let feature_ids: BTreeSet<_> = features.iter().map(|entry| entry.id.clone()).collect();

        let mut normalized_passes = input.passes.clone();
        normalized_passes.sort_by(|left, right| left.id.cmp(&right.id));
        for pass in &mut normalized_passes {
            pass.reads.sort();
            pass.writes.sort();
        }
        let mut pass_map = BTreeMap::new();
        for pass in normalized_passes {
            if pass_map.insert(pass.id.clone(), pass.clone()).is_some() {
                return Err(GraphCompileError::DuplicatePass {
                    pass: pass.id.clone(),
                });
            }
            if !feature_ids.contains(&pass.feature) {
                return Err(GraphCompileError::UnknownFeature {
                    pass: pass.id.clone(),
                    feature: pass.feature.clone(),
                });
            }
        }

        let mut normalized_resources = input.resources.clone();
        normalized_resources.sort_by(|left, right| left.id.cmp(&right.id));
        let mut resource_map = BTreeMap::new();
        for resource in normalized_resources {
            validate_resource_shape(&resource, &feature_ids, &pass_map)?;
            if resource_map
                .insert(resource.id.clone(), resource.clone())
                .is_some()
            {
                return Err(GraphCompileError::MultipleWriters {
                    resource: resource.id.clone(),
                });
            }
        }

        let mut edges: BTreeMap<StableId, BTreeSet<StableId>> = pass_map
            .keys()
            .cloned()
            .map(|pass| (pass, BTreeSet::new()))
            .collect();
        let mut writer_counts: BTreeMap<RenderResourceVersion, usize> = BTreeMap::new();

        for pass in pass_map.values() {
            validate_ordering(pass, &pass_map, &mut edges)?;
            let read_versions: BTreeSet<_> = pass
                .reads
                .iter()
                .map(|access| access.resource.clone())
                .collect();

            for read in &pass.reads {
                validate_access(pass, read, false, &resource_map)?;
                let resource = resource_map.get(&read.resource).ok_or_else(|| {
                    GraphCompileError::MissingProducer {
                        pass: pass.id.clone(),
                        resource: read.resource.clone(),
                    }
                })?;
                match &resource.producer {
                    RenderResourceProducer::HostSlot { slot } => {
                        if slot.ordinal() > pass.slot.ordinal() {
                            return Err(GraphCompileError::ReadBeforeProduce {
                                pass: pass.id.clone(),
                                resource: read.resource.clone(),
                            });
                        }
                    }
                    RenderResourceProducer::FeaturePass { pass: writer } => {
                        if writer == &pass.id {
                            return Err(GraphCompileError::ReadWriteSameVersion {
                                pass: pass.id.clone(),
                                resource: read.resource.clone(),
                            });
                        }
                        add_edge(writer, &pass.id, &pass_map, &mut edges)?;
                    }
                }
            }

            for write in &pass.writes {
                if read_versions.contains(&write.resource) {
                    return Err(GraphCompileError::ReadWriteSameVersion {
                        pass: pass.id.clone(),
                        resource: write.resource.clone(),
                    });
                }
                validate_access(pass, write, true, &resource_map)?;
                let resource = resource_map.get(&write.resource).ok_or_else(|| {
                    GraphCompileError::MissingProducer {
                        pass: pass.id.clone(),
                        resource: write.resource.clone(),
                    }
                })?;
                match &resource.producer {
                    RenderResourceProducer::FeaturePass { pass: writer } if writer == &pass.id => {}
                    _ => {
                        return Err(GraphCompileError::WriterMismatch {
                            pass: pass.id.clone(),
                            resource: write.resource.clone(),
                        });
                    }
                }
                *writer_counts.entry(write.resource.clone()).or_default() += 1;
            }
        }

        for resource in resource_map.values() {
            if matches!(
                &resource.producer,
                RenderResourceProducer::FeaturePass { .. }
            ) {
                match writer_counts.get(&resource.id).copied().unwrap_or_default() {
                    0 => {
                        return Err(GraphCompileError::ProducerWriteMissing {
                            resource: resource.id.clone(),
                        });
                    }
                    1 => {}
                    _ => {
                        return Err(GraphCompileError::MultipleWriters {
                            resource: resource.id.clone(),
                        });
                    }
                }
            }
        }

        validate_edge_slots(&edges, &pass_map)?;
        let ordered_ids = stable_topological_order(&edges)?;
        let mut passes = Vec::with_capacity(ordered_ids.len());
        for (ordinal, id) in ordered_ids.into_iter().enumerate() {
            let pass = pass_map
                .remove(&id)
                .ok_or_else(|| GraphCompileError::InternalPlanInvariant { pass: id.clone() })?;
            let ordinal = u32::try_from(ordinal).map_err(|_| GraphCompileError::TooManyPasses {
                count: input.passes.len(),
            })?;
            passes.push(CompiledPass { ordinal, pass });
        }

        Ok(CompiledRenderPlan {
            schema_major: RENDER_PLAN_SCHEMA_MAJOR,
            data,
            features,
            passes,
            resources: resource_map.into_values().collect(),
        })
    }
}

fn reject_duplicate_ids<'a, I, F>(ids: I, error: F) -> Result<(), GraphCompileError>
where
    I: IntoIterator<Item = &'a StableId>,
    F: Fn(StableId) -> GraphCompileError,
{
    let mut previous: Option<&StableId> = None;
    for id in ids {
        if previous == Some(id) {
            return Err(error(id.clone()));
        }
        previous = Some(id);
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the v1 format and lifetime matrix is intentionally centralized"
)]
fn validate_resource_shape(
    resource: &RenderResourceDecl,
    features: &BTreeSet<StableId>,
    passes: &BTreeMap<StableId, RenderPassDecl>,
) -> Result<(), GraphCompileError> {
    let format_matches = matches!(
        (resource.kind, resource.format),
        (
            RenderResourceKind::Texture,
            RenderResourceFormat::Texture { .. }
        ) | (
            RenderResourceKind::Buffer,
            RenderResourceFormat::Buffer { .. }
        )
    );
    if !format_matches {
        return Err(GraphCompileError::KindFormatMismatch {
            resource: resource.id.clone(),
        });
    }
    if matches!(resource.format, RenderResourceFormat::Buffer { stride: 0 }) {
        return Err(GraphCompileError::ZeroBufferStride {
            resource: resource.id.clone(),
        });
    }
    if resource.sample_count == 0
        || !resource.sample_count.is_power_of_two()
        || resource.sample_count > 16
        || (resource.kind == RenderResourceKind::Buffer && resource.sample_count != 1)
    {
        return Err(GraphCompileError::InvalidSampleCount {
            resource: resource.id.clone(),
            sample_count: resource.sample_count,
        });
    }
    if resource.usages.is_empty() {
        return Err(GraphCompileError::EmptyUsageSet {
            resource: resource.id.clone(),
        });
    }
    for usage in &resource.usages {
        let valid = match resource.format {
            RenderResourceFormat::Texture {
                format: TextureFormatV1::Depth32Float,
            } => matches!(
                usage,
                RenderResourceUsage::Sampled
                    | RenderResourceUsage::DepthRead
                    | RenderResourceUsage::UploadDestination
            ),
            RenderResourceFormat::Texture {
                format: TextureFormatV1::Rgba16Float | TextureFormatV1::R8Unorm,
            } => matches!(
                usage,
                RenderResourceUsage::Sampled
                    | RenderResourceUsage::StorageRead
                    | RenderResourceUsage::StorageWrite
                    | RenderResourceUsage::ColorAttachment
                    | RenderResourceUsage::UploadDestination
            ),
            RenderResourceFormat::Buffer { .. } => matches!(
                usage,
                RenderResourceUsage::StorageRead
                    | RenderResourceUsage::StorageWrite
                    | RenderResourceUsage::Uniform
                    | RenderResourceUsage::UploadDestination
            ),
        } && !(resource.sample_count > 1
            && matches!(
                usage,
                RenderResourceUsage::StorageRead | RenderResourceUsage::StorageWrite
            ));
        if !valid {
            return Err(GraphCompileError::InvalidUsageForFormat {
                resource: resource.id.clone(),
                usage: *usage,
            });
        }
    }
    if let ExtentPolicy::Fixed {
        width,
        height,
        depth,
    } = resource.extent
        && (width == 0 || height == 0 || depth == 0)
    {
        return Err(GraphCompileError::ZeroExtent {
            resource: resource.id.clone(),
        });
    }
    if let RenderResourceScope::Feature { feature } = &resource.scope
        && !features.contains(feature)
    {
        return Err(GraphCompileError::UnknownScopeFeature {
            resource: resource.id.clone(),
            feature: feature.clone(),
        });
    }

    match (&resource.lifetime, &resource.producer) {
        (RenderResourceLifetime::Imported, RenderResourceProducer::HostSlot { .. }) => {}
        (
            RenderResourceLifetime::Transient | RenderResourceLifetime::PersistentPresentation,
            RenderResourceProducer::FeaturePass { pass },
        ) => {
            let writer = passes
                .get(pass)
                .ok_or_else(|| GraphCompileError::UnknownWriterPass {
                    resource: resource.id.clone(),
                    pass: pass.clone(),
                })?;
            if let RenderResourceScope::Feature { feature } = &resource.scope
                && feature != &writer.feature
            {
                return Err(GraphCompileError::ScopeOwnerMismatch {
                    resource: resource.id.clone(),
                    feature: feature.clone(),
                    writer_feature: writer.feature.clone(),
                });
            }
        }
        _ => {
            return Err(GraphCompileError::LifetimeProducerMismatch {
                resource: resource.id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_access(
    pass: &RenderPassDecl,
    access: &PassResourceUse,
    write: bool,
    resources: &BTreeMap<RenderResourceVersion, RenderResourceDecl>,
) -> Result<(), GraphCompileError> {
    let resource =
        resources
            .get(&access.resource)
            .ok_or_else(|| GraphCompileError::MissingProducer {
                pass: pass.id.clone(),
                resource: access.resource.clone(),
            })?;
    if (write && !access.usage.is_write()) || (!write && !access.usage.is_read()) {
        return Err(GraphCompileError::InvalidAccessMode {
            pass: pass.id.clone(),
            resource: access.resource.clone(),
            usage: access.usage,
        });
    }
    if !resource.usages.contains(&access.usage) {
        return Err(GraphCompileError::UsageNotDeclared {
            pass: pass.id.clone(),
            resource: access.resource.clone(),
            usage: access.usage,
        });
    }
    if resource.kind != access.kind {
        return Err(resource_mismatch(pass, access, "kind"));
    }
    if resource.format != access.format {
        return Err(resource_mismatch(pass, access, "format"));
    }
    if resource.sample_count != access.sample_count {
        return Err(resource_mismatch(pass, access, "sample-count"));
    }
    if resource.scope != access.scope {
        return Err(resource_mismatch(pass, access, "scope"));
    }
    Ok(())
}

fn resource_mismatch(
    pass: &RenderPassDecl,
    access: &PassResourceUse,
    field: &'static str,
) -> GraphCompileError {
    GraphCompileError::ResourceMismatch {
        pass: pass.id.clone(),
        resource: access.resource.clone(),
        field,
    }
}

fn validate_ordering(
    pass: &RenderPassDecl,
    passes: &BTreeMap<StableId, RenderPassDecl>,
    edges: &mut BTreeMap<StableId, BTreeSet<StableId>>,
) -> Result<(), GraphCompileError> {
    for predecessor in &pass.after {
        add_edge(predecessor, &pass.id, passes, edges)?;
    }
    for successor in &pass.before {
        add_edge(&pass.id, successor, passes, edges)?;
    }
    Ok(())
}

fn add_edge(
    predecessor: &StableId,
    successor: &StableId,
    passes: &BTreeMap<StableId, RenderPassDecl>,
    edges: &mut BTreeMap<StableId, BTreeSet<StableId>>,
) -> Result<(), GraphCompileError> {
    if !passes.contains_key(predecessor) {
        return Err(GraphCompileError::UnknownOrderTarget {
            pass: successor.clone(),
            target: predecessor.clone(),
        });
    }
    if !passes.contains_key(successor) {
        return Err(GraphCompileError::UnknownOrderTarget {
            pass: predecessor.clone(),
            target: successor.clone(),
        });
    }
    let successors =
        edges
            .get_mut(predecessor)
            .ok_or_else(|| GraphCompileError::InternalPlanInvariant {
                pass: predecessor.clone(),
            })?;
    successors.insert(successor.clone());
    Ok(())
}

fn validate_edge_slots(
    edges: &BTreeMap<StableId, BTreeSet<StableId>>,
    passes: &BTreeMap<StableId, RenderPassDecl>,
) -> Result<(), GraphCompileError> {
    for (predecessor, successors) in edges {
        let predecessor_slot = passes
            .get(predecessor)
            .ok_or_else(|| GraphCompileError::InternalPlanInvariant {
                pass: predecessor.clone(),
            })?
            .slot;
        for successor in successors {
            let successor_slot = passes
                .get(successor)
                .ok_or_else(|| GraphCompileError::InternalPlanInvariant {
                    pass: successor.clone(),
                })?
                .slot;
            if predecessor_slot.ordinal() > successor_slot.ordinal() {
                return Err(GraphCompileError::SlotOrderConflict {
                    predecessor: predecessor.clone(),
                    successor: successor.clone(),
                });
            }
        }
    }
    Ok(())
}

fn stable_topological_order(
    edges: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Result<Vec<StableId>, GraphCompileError> {
    let mut indegree: BTreeMap<StableId, usize> = edges.keys().cloned().map(|id| (id, 0)).collect();
    for successors in edges.values() {
        for successor in successors {
            let degree = indegree.get_mut(successor).ok_or_else(|| {
                GraphCompileError::InternalPlanInvariant {
                    pass: successor.clone(),
                }
            })?;
            *degree += 1;
        }
    }
    let mut ready: BTreeSet<StableId> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut ordered = Vec::with_capacity(edges.len());
    while let Some(next) = ready.pop_first() {
        ordered.push(next.clone());
        let successors = edges
            .get(&next)
            .ok_or_else(|| GraphCompileError::InternalPlanInvariant { pass: next.clone() })?;
        for successor in successors {
            let degree = indegree.get_mut(successor).ok_or_else(|| {
                GraphCompileError::InternalPlanInvariant {
                    pass: successor.clone(),
                }
            })?;
            *degree -= 1;
            if *degree == 0 {
                ready.insert(successor.clone());
            }
        }
    }
    if ordered.len() != edges.len() {
        let remaining = indegree
            .into_iter()
            .filter(|(_, degree)| *degree != 0)
            .map(|(id, _)| id)
            .collect();
        return Err(GraphCompileError::Cycle { remaining });
    }
    Ok(ordered)
}

/// Deterministic failure produced while compiling a logical render plan.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum GraphCompileError {
    /// The input requests a schema major this compiler does not implement.
    #[error("unsupported render-plan schema major {actual}; expected 1")]
    UnsupportedSchema {
        /// Requested major.
        actual: u32,
    },
    /// Two presentation data declarations use the same identity.
    #[error("duplicate RenderData declaration `{0}`")]
    DuplicateData(StableId),
    /// Two feature declarations use the same identity.
    #[error("duplicate RenderFeature declaration `{0}`")]
    DuplicateFeature(StableId),
    /// Two pass declarations use the same identity.
    #[error("duplicate RenderPass declaration `{pass}`")]
    DuplicatePass {
        /// Duplicated pass.
        pass: StableId,
    },
    /// A pass names a feature that is not declared.
    #[error("pass `{pass}` belongs to unknown feature `{feature}`")]
    UnknownFeature {
        /// Pass with the bad owner.
        pass: StableId,
        /// Missing feature.
        feature: StableId,
    },
    /// A feature-scoped resource names a missing feature.
    #[error("resource `{resource}` has unknown scope feature `{feature}`")]
    UnknownScopeFeature {
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Missing feature.
        feature: StableId,
    },
    /// A resource names a writer pass that is not declared.
    #[error("resource `{resource}` has unknown writer pass `{pass}`")]
    UnknownWriterPass {
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Missing pass.
        pass: StableId,
    },
    /// More than one declaration or write owns an SSA version.
    #[error("resource version `{resource}` has multiple writers")]
    MultipleWriters {
        /// Conflicting resource version.
        resource: RenderResourceVersion,
    },
    /// Resource kind and portable format disagree.
    #[error("resource `{resource}` kind and format disagree")]
    KindFormatMismatch {
        /// Affected resource.
        resource: RenderResourceVersion,
    },
    /// A buffer declares an unusable zero stride.
    #[error("buffer resource `{resource}` has zero stride")]
    ZeroBufferStride {
        /// Affected resource.
        resource: RenderResourceVersion,
    },
    /// Sample count is zero, non-power-of-two, too large for v1, or non-one for a buffer.
    #[error("resource `{resource}` has invalid sample count {sample_count}")]
    InvalidSampleCount {
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Rejected sample count.
        sample_count: u32,
    },
    /// A resource declares no legal use.
    #[error("resource `{resource}` has an empty usage set")]
    EmptyUsageSet {
        /// Affected resource.
        resource: RenderResourceVersion,
    },
    /// Resource usage is incompatible with its format or sample count.
    #[error("resource `{resource}` usage `{usage:?}` is incompatible with its format")]
    InvalidUsageForFormat {
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Rejected usage.
        usage: RenderResourceUsage,
    },
    /// A fixed extent contains a zero dimension.
    #[error("resource `{resource}` has a zero fixed extent")]
    ZeroExtent {
        /// Affected resource.
        resource: RenderResourceVersion,
    },
    /// Imported and pass-produced lifetime rules disagree with the producer.
    #[error("resource `{resource}` lifetime and producer disagree")]
    LifetimeProducerMismatch {
        /// Affected resource.
        resource: RenderResourceVersion,
    },
    /// Feature scope differs from the writer pass owner.
    #[error(
        "resource `{resource}` scope owner `{feature}` differs from writer feature `{writer_feature}`"
    )]
    ScopeOwnerMismatch {
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Declared scope owner.
        feature: StableId,
        /// Writer pass owner.
        writer_feature: StableId,
    },
    /// A resource use references an undeclared SSA version.
    #[error("pass `{pass}` references resource `{resource}` without a producer")]
    MissingProducer {
        /// Pass containing the use.
        pass: StableId,
        /// Missing resource version.
        resource: RenderResourceVersion,
    },
    /// A pass uses a read usage for a write or a write usage for a read.
    #[error("pass `{pass}` uses `{usage:?}` in the wrong access mode for `{resource}`")]
    InvalidAccessMode {
        /// Pass containing the use.
        pass: StableId,
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Rejected usage.
        usage: RenderResourceUsage,
    },
    /// A pass exercises a usage omitted from the resource declaration.
    #[error("pass `{pass}` uses undeclared `{usage:?}` usage for `{resource}`")]
    UsageNotDeclared {
        /// Pass containing the use.
        pass: StableId,
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Missing declared usage.
        usage: RenderResourceUsage,
    },
    /// A pass expectation differs from the declared resource shape.
    #[error("pass `{pass}` has a {field} mismatch for resource `{resource}`")]
    ResourceMismatch {
        /// Pass containing the use.
        pass: StableId,
        /// Affected resource.
        resource: RenderResourceVersion,
        /// Stable mismatch field name.
        field: &'static str,
    },
    /// A pass attempts to rewrite the exact SSA version it reads.
    #[error("pass `{pass}` reads and writes the same SSA version `{resource}`")]
    ReadWriteSameVersion {
        /// Pass containing the conflict.
        pass: StableId,
        /// Conflicting version.
        resource: RenderResourceVersion,
    },
    /// A pass write disagrees with the resource's unique producer.
    #[error("pass `{pass}` is not the declared writer of resource `{resource}`")]
    WriterMismatch {
        /// Pass attempting the write.
        pass: StableId,
        /// Affected resource.
        resource: RenderResourceVersion,
    },
    /// A pass-produced resource is not present in the pass write set.
    #[error("resource `{resource}` names a producer but has no matching pass write")]
    ProducerWriteMissing {
        /// Affected resource.
        resource: RenderResourceVersion,
    },
    /// An explicit ordering edge names an unknown pass.
    #[error("pass `{pass}` has an ordering edge to unknown pass `{target}`")]
    UnknownOrderTarget {
        /// Pass that declares or receives the edge.
        pass: StableId,
        /// Missing target.
        target: StableId,
    },
    /// A dependency would require reading before a host slot establishes a value.
    #[error("pass `{pass}` reads resource `{resource}` before it is produced")]
    ReadBeforeProduce {
        /// Reader pass.
        pass: StableId,
        /// Resource unavailable at the pass slot.
        resource: RenderResourceVersion,
    },
    /// A dependency edge contradicts the semantic slot order.
    #[error("pass `{predecessor}` cannot precede earlier-slot pass `{successor}`")]
    SlotOrderConflict {
        /// Required predecessor.
        predecessor: StableId,
        /// Required successor.
        successor: StableId,
    },
    /// The pass dependency graph is cyclic.
    #[error("render pass graph is cyclic: {remaining:?}")]
    Cycle {
        /// Stable-sorted passes remaining in the cycle or downstream of it.
        remaining: Vec<StableId>,
    },
    /// The platform cannot represent the number of passes as v1 ordinals.
    #[error("render plan has too many passes: {count}")]
    TooManyPasses {
        /// Pass count.
        count: usize,
    },
    /// A checked internal invariant failed without panicking.
    #[error("internal render-plan invariant failed for pass `{pass}`")]
    InternalPlanInvariant {
        /// Pass associated with the failed invariant.
        pass: StableId,
    },
}
