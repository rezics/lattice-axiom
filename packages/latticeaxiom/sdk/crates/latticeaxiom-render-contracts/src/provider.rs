#![allow(
    clippy::result_large_err,
    reason = "activation-only typed diagnostics retain complete provider identities"
)]

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{CapabilityId, StableId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::TextureFormatV1;

/// Standard GPU features understood by provider contract major 1.
///
/// Vendor and device identifiers are intentionally absent. A separately
/// governed denylist may mark a realization unavailable, but positive selection
/// is based only on portable capabilities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpuFeatureV1 {
    /// Compute pipeline support required by D5 portable compute fixtures.
    ComputeShaders,
    /// 16-bit floating-point shader arithmetic.
    ShaderF16,
    /// Read/write storage support for `r8unorm` textures.
    StorageTextureR8Unorm,
}

/// Standard numeric GPU limits understood by provider contract major 1.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpuLimitV1 {
    /// Maximum workgroups in each dispatch dimension.
    MaxComputeWorkgroupsPerDimension,
    /// Maximum storage buffers visible to one shader stage.
    MaxStorageBuffersPerShaderStage,
    /// Maximum two-dimensional texture dimension.
    MaxTextureDimension2d,
}

/// A normalized standard-capability snapshot supplied by the Bevy host adapter.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GpuCapabilities {
    /// Available standard features.
    pub features: BTreeSet<GpuFeatureV1>,
    /// Advertised standard numeric limits.
    pub limits: BTreeMap<GpuLimitV1, u64>,
    /// Available portable physical formats.
    pub formats: BTreeSet<TextureFormatV1>,
}

/// Hardware requirements for one presentation realization.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequirement {
    /// Standard features that must all be present.
    pub features: BTreeSet<GpuFeatureV1>,
    /// Inclusive minimum standard limits.
    pub minimum_limits: BTreeMap<GpuLimitV1, u64>,
    /// Portable formats that must all be supported.
    pub formats: BTreeSet<TextureFormatV1>,
}

impl ProviderRequirement {
    fn first_failure(&self, gpu: &GpuCapabilities) -> Option<ProviderRequirementFailure> {
        if let Some(feature) = self.features.difference(&gpu.features).next() {
            return Some(ProviderRequirementFailure::MissingFeature { feature: *feature });
        }
        for (limit, required) in &self.minimum_limits {
            let available = gpu.limits.get(limit).copied().unwrap_or_default();
            if available < *required {
                return Some(ProviderRequirementFailure::LimitTooLow {
                    limit: *limit,
                    required: *required,
                    available,
                });
            }
        }
        self.formats
            .difference(&gpu.formats)
            .next()
            .map(|format| ProviderRequirementFailure::MissingFormat { format: *format })
    }
}

/// Stable reason a realization could not run on a capability snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProviderRequirementFailure {
    /// A required standard feature is unavailable.
    MissingFeature {
        /// Missing feature.
        feature: GpuFeatureV1,
    },
    /// A required standard limit is lower than requested.
    LimitTooLow {
        /// Limit being checked.
        limit: GpuLimitV1,
        /// Inclusive minimum.
        required: u64,
        /// Advertised value, or zero when absent.
        available: u64,
    },
    /// A required portable format is unavailable.
    MissingFormat {
        /// Missing format.
        format: TextureFormatV1,
    },
}

/// Mandatory outcome when a realization does not satisfy the GPU matrix.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProviderFallback {
    /// Try another realization owned by the same logical provider.
    Realization {
        /// Target realization identity.
        realization: StableId,
    },
    /// Disable a presentation-optional provider without changing world state.
    Disabled,
    /// Fail activation because no supported realization remains.
    Fail,
}

/// One device-selectable realization of a locked logical provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRealization {
    /// Stable realization identity.
    pub id: StableId,
    /// Profile-authored stable priority; larger values are preferred.
    pub stable_priority: i32,
    /// Standard GPU requirements.
    pub requirements: ProviderRequirement,
    /// Mandatory unsupported-device fallback.
    pub fallback: ProviderFallback,
}

/// An exactly-one logical render mechanism provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderProviderDecl {
    /// Stable logical provider identity.
    pub id: StableId,
    /// Exactly-one capability implemented by this provider.
    pub capability: CapabilityId,
    /// Whether `Disabled` is a legal presentation fallback.
    pub presentation_optional: bool,
    /// Candidate realizations; discovery order has no semantic meaning.
    pub realizations: Vec<ProviderRealization>,
}

/// Exactly-one logical selection and device-dependent realization input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSelectionRequest {
    /// Capability for which exactly one provider is required.
    pub capability: CapabilityId,
    /// Logical providers present in the locked closure.
    pub providers: Vec<RenderProviderDecl>,
    /// Profile-selected provider, required when more than one exists.
    pub explicit_provider: Option<StableId>,
    /// Standard hardware capability snapshot.
    pub gpu: GpuCapabilities,
}

/// One realization rejected while traversing a mandatory fallback chain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedRealization {
    /// Rejected realization identity.
    pub realization: StableId,
    /// Deterministic first unsatisfied requirement.
    pub reason: ProviderRequirementFailure,
}

/// Selected logical provider and presentation-only realization result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSelection {
    /// Exactly-one logical provider selected by the lock/profile layer.
    pub provider: StableId,
    /// Selected supported realization, or `None` for optional disabled fallback.
    pub realization: Option<StableId>,
    /// Stable trace of rejected realizations along the fallback chain.
    pub rejected: Vec<RejectedRealization>,
    /// Whether selection ended at the optional `Disabled` terminal.
    pub disabled: bool,
}

/// Selects exactly one logical provider, then a supported presentation realization.
///
/// GPU capabilities never switch logical providers. With multiple logical
/// providers, the profile must select one explicitly. Within the locked provider,
/// stable priority chooses the starting realization and the declaration's
/// mandatory acyclic fallback chain controls degradation.
///
/// # Errors
///
/// Returns [`ProviderSelectError`] for missing or conflicting logical providers,
/// malformed realization/fallback graphs, or an exhausted required fallback.
pub fn select_provider(
    request: &ProviderSelectionRequest,
) -> Result<ProviderSelection, ProviderSelectError> {
    let mut providers = request.providers.clone();
    providers.sort_by(|left, right| left.id.cmp(&right.id));
    for pair in providers.windows(2) {
        if pair[0].id == pair[1].id {
            return Err(ProviderSelectError::DuplicateProvider {
                provider: pair[0].id.clone(),
            });
        }
    }
    for provider in &providers {
        if provider.capability != request.capability {
            return Err(ProviderSelectError::CapabilityMismatch {
                provider: provider.id.clone(),
                expected: request.capability.clone(),
                actual: provider.capability.clone(),
            });
        }
        validate_provider(provider)?;
    }

    let provider = match (providers.as_slice(), &request.explicit_provider) {
        ([], _) => {
            return Err(ProviderSelectError::MissingProvider {
                capability: request.capability.clone(),
            });
        }
        ([only], None) => only,
        (_, None) => {
            return Err(ProviderSelectError::ProviderConflict {
                capability: request.capability.clone(),
                candidates: providers.iter().map(|entry| entry.id.clone()).collect(),
            });
        }
        (_, Some(selected)) => providers
            .iter()
            .find(|entry| &entry.id == selected)
            .ok_or_else(|| ProviderSelectError::UnknownExplicitProvider {
                capability: request.capability.clone(),
                provider: selected.clone(),
            })?,
    };

    let realization_map: BTreeMap<_, _> = provider
        .realizations
        .iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect();
    let mut starts: Vec<_> = provider.realizations.iter().collect();
    starts.sort_by(|left, right| {
        right
            .stable_priority
            .cmp(&left.stable_priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut current =
        starts
            .first()
            .copied()
            .ok_or_else(|| ProviderSelectError::NoRealizations {
                provider: provider.id.clone(),
            })?;
    let mut rejected = Vec::new();
    loop {
        if let Some(reason) = current.requirements.first_failure(&request.gpu) {
            rejected.push(RejectedRealization {
                realization: current.id.clone(),
                reason,
            });
            match &current.fallback {
                ProviderFallback::Realization { realization } => {
                    current = realization_map.get(realization).copied().ok_or_else(|| {
                        ProviderSelectError::UnknownFallback {
                            provider: provider.id.clone(),
                            realization: current.id.clone(),
                            target: realization.clone(),
                        }
                    })?;
                }
                ProviderFallback::Disabled => {
                    return Ok(ProviderSelection {
                        provider: provider.id.clone(),
                        realization: None,
                        rejected,
                        disabled: true,
                    });
                }
                ProviderFallback::Fail => {
                    return Err(ProviderSelectError::NoSupportedRealization {
                        provider: provider.id.clone(),
                        rejected,
                    });
                }
            }
        } else {
            return Ok(ProviderSelection {
                provider: provider.id.clone(),
                realization: Some(current.id.clone()),
                rejected,
                disabled: false,
            });
        }
    }
}

fn validate_provider(provider: &RenderProviderDecl) -> Result<(), ProviderSelectError> {
    if provider.realizations.is_empty() {
        return Err(ProviderSelectError::NoRealizations {
            provider: provider.id.clone(),
        });
    }
    let mut ids = BTreeSet::new();
    for realization in &provider.realizations {
        if !ids.insert(realization.id.clone()) {
            return Err(ProviderSelectError::DuplicateRealization {
                provider: provider.id.clone(),
                realization: realization.id.clone(),
            });
        }
        if !provider.presentation_optional
            && matches!(&realization.fallback, ProviderFallback::Disabled)
        {
            return Err(ProviderSelectError::RequiredProviderCanDisable {
                provider: provider.id.clone(),
                realization: realization.id.clone(),
            });
        }
    }

    for realization in &provider.realizations {
        let mut path = BTreeSet::new();
        let mut current = realization;
        loop {
            if !path.insert(current.id.clone()) {
                return Err(ProviderSelectError::FallbackCycle {
                    provider: provider.id.clone(),
                    cycle: path.into_iter().collect(),
                });
            }
            match &current.fallback {
                ProviderFallback::Realization {
                    realization: target,
                } => {
                    current = provider
                        .realizations
                        .iter()
                        .find(|candidate| &candidate.id == target)
                        .ok_or_else(|| ProviderSelectError::UnknownFallback {
                            provider: provider.id.clone(),
                            realization: current.id.clone(),
                            target: target.clone(),
                        })?;
                }
                ProviderFallback::Disabled | ProviderFallback::Fail => break,
            }
        }
    }
    Ok(())
}

/// Deterministic provider selection or fallback validation failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProviderSelectError {
    /// No logical provider supplies the requested exactly-one capability.
    #[error("missing provider for exactly-one capability `{capability}`")]
    MissingProvider {
        /// Missing capability.
        capability: CapabilityId,
    },
    /// Multiple logical providers exist without an explicit profile selection.
    #[error("provider conflict for `{capability}`: {candidates:?}")]
    ProviderConflict {
        /// Conflicted capability.
        capability: CapabilityId,
        /// Stable-sorted candidate providers.
        candidates: Vec<StableId>,
    },
    /// The profile selected an ID that is not a candidate provider.
    #[error("profile selected unknown provider `{provider}` for `{capability}`")]
    UnknownExplicitProvider {
        /// Requested capability.
        capability: CapabilityId,
        /// Unknown provider.
        provider: StableId,
    },
    /// Two provider declarations have the same stable identity.
    #[error("duplicate provider declaration `{provider}`")]
    DuplicateProvider {
        /// Duplicated provider.
        provider: StableId,
    },
    /// A provider declaration supplies a different capability than requested.
    #[error("provider `{provider}` supplies `{actual}`, expected `{expected}`")]
    CapabilityMismatch {
        /// Provider with the mismatch.
        provider: StableId,
        /// Requested capability.
        expected: CapabilityId,
        /// Declared capability.
        actual: CapabilityId,
    },
    /// A logical provider has no realization candidate.
    #[error("provider `{provider}` has no realizations")]
    NoRealizations {
        /// Empty provider.
        provider: StableId,
    },
    /// Two realization declarations have the same stable identity.
    #[error("provider `{provider}` has duplicate realization `{realization}`")]
    DuplicateRealization {
        /// Provider containing the duplicate.
        provider: StableId,
        /// Duplicated realization.
        realization: StableId,
    },
    /// A required provider uses the optional disabled terminal.
    #[error("required provider `{provider}` realization `{realization}` can disable")]
    RequiredProviderCanDisable {
        /// Required provider.
        provider: StableId,
        /// Invalid realization.
        realization: StableId,
    },
    /// A realization fallback target does not exist in the same provider.
    #[error("provider `{provider}` realization `{realization}` has unknown fallback `{target}`")]
    UnknownFallback {
        /// Provider containing the bad edge.
        provider: StableId,
        /// Realization declaring the edge.
        realization: StableId,
        /// Missing target.
        target: StableId,
    },
    /// Mandatory realization fallbacks contain a cycle.
    #[error("provider `{provider}` fallback graph is cyclic: {cycle:?}")]
    FallbackCycle {
        /// Provider containing the cycle.
        provider: StableId,
        /// Stable-sorted members observed in the cycle walk.
        cycle: Vec<StableId>,
    },
    /// All realizations on a required fallback chain were unsupported.
    #[error("provider `{provider}` has no supported realization: {rejected:?}")]
    NoSupportedRealization {
        /// Provider that could not activate.
        provider: StableId,
        /// Stable traversal trace.
        rejected: Vec<RejectedRealization>,
    },
}
