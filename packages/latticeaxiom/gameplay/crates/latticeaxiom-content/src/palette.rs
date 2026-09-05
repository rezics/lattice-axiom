use latticeaxiom_core::{StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};

use crate::{
    BlockStateV1, ContentCatalogV1, ContentError, ContentResult, FluidStateV1,
    header::validate_exact_id,
};

/// Caller-supplied in-memory palette compilation limits.
///
/// These limits bound allocation only. Bit packing, chunk traversal, and
/// snapshot encoding remain owned by the persistence contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteLimitsV1 {
    /// Maximum solid entries.
    pub max_solid_entries: usize,
    /// Maximum fluid entries, including canonical `Empty` at index zero.
    pub max_fluid_entries: usize,
}

impl Default for PaletteLimitsV1 {
    fn default() -> Self {
        Self {
            max_solid_entries: 65_536,
            max_fluid_entries: 65_536,
        }
    }
}

/// Untrusted solid palette entry using stable identity plus canonical state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SolidPaletteEntryV1 {
    /// Exact block identity.
    pub block: StableId,
    /// Complete concrete block state.
    pub state: BlockStateV1,
}

/// Untrusted fluid palette entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum FluidPaletteEntryV1 {
    /// No fluid. Compilation places this at index zero.
    Empty,
    /// Exact fluid identity plus complete authoritative per-cell state.
    Fluid {
        /// Exact fluid identity.
        fluid: StableId,
        /// Level and explicit flow.
        state: FluidStateV1,
    },
}

/// Validated solid palette entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledSolidPaletteEntryV1 {
    block: StableId,
    state: BlockStateV1,
}

impl CompiledSolidPaletteEntryV1 {
    /// Returns the exact block identity.
    #[must_use]
    pub const fn block(&self) -> &StableId {
        &self.block
    }

    /// Returns the complete canonical block state.
    #[must_use]
    pub const fn state(&self) -> &BlockStateV1 {
        &self.state
    }
}

/// Validated fluid palette entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CompiledFluidPaletteEntryV1(CompiledFluidPaletteEntryKindV1);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum CompiledFluidPaletteEntryKindV1 {
    Empty,
    Fluid {
        fluid: StableId,
        state: FluidStateV1,
    },
}

impl CompiledFluidPaletteEntryV1 {
    /// Returns whether this is the canonical empty entry.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.0, CompiledFluidPaletteEntryKindV1::Empty)
    }

    /// Returns the exact fluid identity for a non-empty entry.
    #[must_use]
    pub const fn fluid(&self) -> Option<&StableId> {
        match &self.0 {
            CompiledFluidPaletteEntryKindV1::Empty => None,
            CompiledFluidPaletteEntryKindV1::Fluid { fluid, .. } => Some(fluid),
        }
    }

    /// Returns the authoritative fluid state for a non-empty entry.
    #[must_use]
    pub const fn state(&self) -> Option<&FluidStateV1> {
        match &self.0 {
            CompiledFluidPaletteEntryKindV1::Empty => None,
            CompiledFluidPaletteEntryKindV1::Fluid { state, .. } => Some(state),
        }
    }
}

/// Validated solid palette in StableId-plus-state canonical order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CompiledSolidPaletteV1(Vec<CompiledSolidPaletteEntryV1>);

impl CompiledSolidPaletteV1 {
    /// Validates and canonically sorts solid palette entries.
    ///
    /// # Errors
    ///
    /// Returns an error for an exceeded limit, wrong or unknown block ID,
    /// state outside the definition's explicit palette, duplicate entry, or
    /// failed canonical state encoding.
    pub fn compile(
        catalog: &ContentCatalogV1,
        entries: Vec<SolidPaletteEntryV1>,
        limits: PaletteLimitsV1,
    ) -> ContentResult<Self> {
        enforce_limit(
            "solid_palette_entries",
            entries.len(),
            limits.max_solid_entries,
        )?;
        let mut keyed = Vec::with_capacity(entries.len());
        for entry in entries {
            validate_exact_id(&entry.block, "block", "solid palette block")?;
            let Some(definition) = catalog.block(&entry.block) else {
                return Err(ContentError::InvalidSolidPaletteEntry {
                    block: entry.block,
                    reason: "block is absent from the validated catalog",
                });
            };
            if !definition.contains_state(&entry.state) {
                return Err(ContentError::InvalidSolidPaletteEntry {
                    block: entry.block,
                    reason: "state is outside the block definition's explicit palette",
                });
            }
            let state_key = entry.state.canonical_bytes()?;
            keyed.push((entry.block, state_key, entry.state));
        }
        keyed.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        for pair in keyed.windows(2) {
            if pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1 {
                return Err(ContentError::Duplicate {
                    resource: "solid palette entry",
                    id: pair[0].0.to_string(),
                });
            }
        }
        Ok(Self(
            keyed
                .into_iter()
                .map(|(block, _, state)| CompiledSolidPaletteEntryV1 { block, state })
                .collect(),
        ))
    }

    /// Returns canonical entries.
    #[must_use]
    pub fn entries(&self) -> &[CompiledSolidPaletteEntryV1] {
        &self.0
    }

    /// Returns whether the palette is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the entry count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// Validated fluid palette with `Empty` fixed at index zero.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CompiledFluidPaletteV1(Vec<CompiledFluidPaletteEntryV1>);

impl CompiledFluidPaletteV1 {
    /// Validates and canonically sorts fluid palette entries.
    ///
    /// The input may omit `Empty`; compilation inserts it. Supplying it more
    /// than once is rejected. Non-empty rows sort by `StableId` then canonical
    /// state bytes.
    ///
    /// # Errors
    ///
    /// Returns an error for an exceeded limit, duplicate entry, wrong fluid
    /// identity, or a fluid absent from the validated catalog.
    pub fn compile(
        catalog: &ContentCatalogV1,
        entries: Vec<FluidPaletteEntryV1>,
        limits: PaletteLimitsV1,
    ) -> ContentResult<Self> {
        enforce_limit(
            "authored_fluid_palette_entries",
            entries.len(),
            limits.max_fluid_entries,
        )?;
        let mut saw_empty = false;
        let mut fluids = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry {
                FluidPaletteEntryV1::Empty => {
                    if saw_empty {
                        return Err(ContentError::Duplicate {
                            resource: "fluid palette entry",
                            id: "empty".to_owned(),
                        });
                    }
                    saw_empty = true;
                }
                FluidPaletteEntryV1::Fluid { fluid, state } => {
                    validate_exact_id(&fluid, "fluid", "fluid palette fluid")?;
                    if catalog.fluid(&fluid).is_none() {
                        return Err(ContentError::UnknownFluidPaletteEntry { fluid });
                    }
                    let state_key = canonical_json_bytes(&state)?;
                    fluids.push((fluid, state_key, state));
                }
            }
        }
        let final_len = fluids.len().saturating_add(1);
        enforce_limit("fluid_palette_entries", final_len, limits.max_fluid_entries)?;
        fluids.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        for pair in fluids.windows(2) {
            if pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1 {
                return Err(ContentError::Duplicate {
                    resource: "fluid palette entry",
                    id: format!("{}:{}", pair[0].0, String::from_utf8_lossy(&pair[0].1)),
                });
            }
        }
        let mut compiled = Vec::with_capacity(final_len);
        compiled.push(CompiledFluidPaletteEntryV1(
            CompiledFluidPaletteEntryKindV1::Empty,
        ));
        compiled.extend(fluids.into_iter().map(|(fluid, _, state)| {
            CompiledFluidPaletteEntryV1(CompiledFluidPaletteEntryKindV1::Fluid { fluid, state })
        }));
        Ok(Self(compiled))
    }

    /// Returns canonical entries with `Empty` at index zero.
    #[must_use]
    pub fn entries(&self) -> &[CompiledFluidPaletteEntryV1] {
        &self.0
    }

    /// Returns the entry count, including `Empty`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `false`; a compiled fluid palette always contains `Empty`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

fn enforce_limit(resource: &'static str, actual: usize, limit: usize) -> ContentResult<()> {
    if actual > limit {
        return Err(ContentError::LimitExceeded {
            resource,
            actual,
            limit,
        });
    }
    Ok(())
}
