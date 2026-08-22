//! Deterministic solid/fluid occupancy arbitration and versioned candidates.
//!
//! Orthogonal solid and fluid layers are combined here. Dynamic flow, mixing,
//! waterlogging, and persistence envelopes are out of scope; this module only
//! decides the initial cell and records hard queue/cell/byte hooks.

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};

use crate::{
    BlockStateSemanticsV1, BlockStateV1, BoundedFluidUpdatePolicyV1, CompiledFluidPaletteV1,
    CompiledSolidPaletteV1, ContentCatalogV1, ContentError, ContentResult, FluidOccupancyPolicyV1,
    FluidPaletteEntryV1, FluidStateV1, PaletteLimitsV1, ReplaceabilityV1, SolidOccupancyV1,
    SolidPaletteEntryV1, ValidatedBlockDefinitionV1,
};

/// Canonical schema identity for a dual-layer occupancy candidate.
pub const SOLID_FLUID_OCCUPANCY_CANDIDATE_SCHEMA_V1: &str =
    "latticeaxiom:schema/solid-fluid-occupancy-candidate@1";

/// Classified v1 fluid-occupancy policy kind.
///
/// Classification uses the registration kind and the last path token so host
/// crates do not embed package-owned concrete IDs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FluidOccupancyKindV1 {
    /// Fluid may occupy an empty solid layer in place.
    Direct,
    /// Fluid is refused for this solid state.
    Reject,
    /// Reserved coexistence. v1 still forbids waterlogging of a non-empty solid.
    Coexist,
}

impl FluidOccupancyKindV1 {
    /// Classifies a versioned fluid-occupancy policy identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity is not `fluid-occupancy` or the path
    /// token is outside the closed v1 set.
    pub fn classify(policy: &FluidOccupancyPolicyV1) -> ContentResult<Self> {
        classify_policy(
            policy.as_stable_id(),
            "fluid-occupancy",
            "fluid occupancy policy",
            &[
                ("direct", Self::Direct),
                ("reject", Self::Reject),
                ("coexist", Self::Coexist),
            ],
        )
    }
}

/// Classified v1 solid-occupancy kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SolidOccupancyKindV1 {
    /// No solid volume occupies the cell.
    Empty,
    /// A solid volume occupies the cell.
    Full,
}

impl SolidOccupancyKindV1 {
    /// Classifies a versioned solid-occupancy identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity is not `solid-occupancy` or the path
    /// token is outside the closed v1 set.
    pub fn classify(policy: &SolidOccupancyV1) -> ContentResult<Self> {
        classify_policy(
            policy.as_stable_id(),
            "solid-occupancy",
            "solid occupancy policy",
            &[("empty", Self::Empty), ("full", Self::Full)],
        )
    }
}

/// Classified v1 replaceability kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplaceabilityKindV1 {
    /// An authoritative replace command may clear the solid before fluid.
    Replaceable,
    /// The solid may not be replaced to admit fluid.
    Never,
}

impl ReplaceabilityKindV1 {
    /// Classifies a versioned replaceability identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity is not `replaceability` or the path
    /// token is outside the closed v1 set.
    pub fn classify(policy: &ReplaceabilityV1) -> ContentResult<Self> {
        classify_policy(
            policy.as_stable_id(),
            "replaceability",
            "replaceability policy",
            &[("replaceable", Self::Replaceable), ("never", Self::Never)],
        )
    }
}

/// Frozen empty-solid identity used when replace-then-occupy fires.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OccupancyArbitrationContextV1 {
    empty_block: StableId,
    empty_state: BlockStateV1,
}

impl OccupancyArbitrationContextV1 {
    /// Creates the empty-solid target from frozen Role output.
    ///
    /// # Errors
    ///
    /// Returns an error unless `empty_block` is an exact `block` identity.
    pub fn new(empty_block: StableId, empty_state: BlockStateV1) -> ContentResult<Self> {
        crate::header::validate_exact_id(&empty_block, "block", "empty solid occupancy target")?;
        Ok(Self {
            empty_block,
            empty_state,
        })
    }

    /// Returns the frozen empty solid identity.
    #[must_use]
    pub const fn empty_block(&self) -> &StableId {
        &self.empty_block
    }

    /// Returns the canonical empty solid state.
    #[must_use]
    pub const fn empty_state(&self) -> &BlockStateV1 {
        &self.empty_state
    }
}

/// Closed reasons a fluid occupancy request is refused.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OccupancyRejectV1 {
    /// The solid's fluid-occupancy policy refuses fluid.
    PolicyRejects,
    /// v1 forbids two different fluids in one cell.
    Mixing,
    /// The solid is not the empty Role target and is not replaceable.
    SolidOccupied,
}

impl OccupancyRejectV1 {
    /// Returns the stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyRejects => "policy-rejects",
            Self::Mixing => "mixing",
            Self::SolidOccupied => "solid-occupied",
        }
    }
}

/// Deterministic arbitration result for one solid/fluid cell.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SolidFluidArbitrationV1 {
    /// The requested fluid occupies the current empty solid layer.
    Occupied {
        /// Unchanged solid identity.
        solid: StableId,
        /// Unchanged solid state.
        solid_state: BlockStateV1,
        /// Occupying fluid identity.
        fluid: StableId,
        /// Authoritative per-cell fluid state.
        fluid_state: FluidStateV1,
    },
    /// A replaceable non-empty solid is cleared to the empty Role, then occupied.
    ReplaceThenOccupy {
        /// Solid identity that must be replaced first.
        from_solid: StableId,
        /// Frozen empty solid identity.
        to_solid: StableId,
        /// Canonical empty solid state.
        to_solid_state: BlockStateV1,
        /// Occupying fluid identity.
        fluid: StableId,
        /// Authoritative per-cell fluid state.
        fluid_state: FluidStateV1,
    },
    /// Fluid is refused; the solid layer is unchanged and the fluid layer stays empty.
    Rejected {
        /// Current solid identity.
        solid: StableId,
        /// Current solid state.
        solid_state: BlockStateV1,
        /// Closed reject reason.
        reason: OccupancyRejectV1,
    },
    /// No fluid was requested; both layers stay as supplied.
    Unchanged {
        /// Current solid identity.
        solid: StableId,
        /// Current solid state.
        solid_state: BlockStateV1,
        /// Current fluid layer, which may already be empty.
        fluid: FluidPaletteEntryV1,
    },
}

/// Hard per-candidate cell, queue, and byte counters.
///
/// These hooks fail closed when a bound is exceeded. They do not run a fluid
/// tick or schedule dynamic flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FluidWorkAccountingV1 {
    cells_examined: u32,
    cells_changed: u32,
    queue_depth: u32,
    in_flight_bytes: u32,
    max_cells_per_tick: u32,
    max_queue_depth: u32,
    max_in_flight_bytes: u32,
}

impl FluidWorkAccountingV1 {
    /// Creates a zeroed ledger from a frozen fluid update policy.
    #[must_use]
    pub const fn from_policy(policy: BoundedFluidUpdatePolicyV1) -> Self {
        Self {
            cells_examined: 0,
            cells_changed: 0,
            queue_depth: 0,
            in_flight_bytes: 0,
            max_cells_per_tick: policy.max_cells_per_tick.get(),
            max_queue_depth: policy.max_queue_depth.get(),
            max_in_flight_bytes: policy.max_in_flight_bytes.get(),
        }
    }

    /// Returns examined cell count.
    #[must_use]
    pub const fn cells_examined(self) -> u32 {
        self.cells_examined
    }

    /// Returns cells whose occupancy actually changed.
    #[must_use]
    pub const fn cells_changed(self) -> u32 {
        self.cells_changed
    }

    /// Returns the high-water queued frontier depth.
    #[must_use]
    pub const fn queue_depth(self) -> u32 {
        self.queue_depth
    }

    /// Returns accounted in-flight candidate bytes.
    #[must_use]
    pub const fn in_flight_bytes(self) -> u32 {
        self.in_flight_bytes
    }

    /// Records examined cells against the hard cell bound.
    ///
    /// # Errors
    ///
    /// Returns [`ContentError::LimitExceeded`] when the bound is crossed.
    pub fn examine(&mut self, count: u32) -> ContentResult<()> {
        self.cells_examined = saturating_add_u32(self.cells_examined, count);
        enforce_account(
            "fluid_cells_per_tick",
            self.cells_examined,
            self.max_cells_per_tick,
        )
    }

    /// Records occupancy changes against the hard cell bound.
    ///
    /// # Errors
    ///
    /// Returns [`ContentError::LimitExceeded`] when the bound is crossed.
    pub fn change(&mut self, count: u32) -> ContentResult<()> {
        self.cells_changed = saturating_add_u32(self.cells_changed, count);
        enforce_account(
            "fluid_cells_per_tick",
            self.cells_changed,
            self.max_cells_per_tick,
        )
    }

    /// Records frontier queue depth against the hard queue bound.
    ///
    /// # Errors
    ///
    /// Returns [`ContentError::LimitExceeded`] when the bound is crossed.
    pub fn enqueue(&mut self, depth: u32) -> ContentResult<()> {
        self.queue_depth = self.queue_depth.max(depth);
        enforce_account("fluid_queue_depth", self.queue_depth, self.max_queue_depth)
    }

    /// Records candidate bytes against the hard in-flight bound.
    ///
    /// # Errors
    ///
    /// Returns [`ContentError::LimitExceeded`] when the bound is crossed.
    pub fn record_bytes(&mut self, bytes: u32) -> ContentResult<()> {
        self.in_flight_bytes = saturating_add_u32(self.in_flight_bytes, bytes);
        enforce_account(
            "fluid_in_flight_bytes",
            self.in_flight_bytes,
            self.max_in_flight_bytes,
        )
    }
}

/// One locally addressed occupancy intent accepted by candidate compilation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OccupancyCellIntentV1 {
    /// Local voxel X.
    pub x: u16,
    /// Local voxel Y.
    pub y: u16,
    /// Local voxel Z.
    pub z: u16,
    /// Current solid layer.
    pub solid: SolidPaletteEntryV1,
    /// Current fluid layer before arbitration.
    pub current_fluid: FluidPaletteEntryV1,
    /// Requested fluid layer after hydrology or placement.
    pub requested_fluid: FluidPaletteEntryV1,
}

/// Dual-layer cell stored in a compiled occupancy candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OccupancyCellV1 {
    x: u16,
    y: u16,
    z: u16,
    solid_palette_index: u16,
    fluid_palette_index: u16,
}

impl OccupancyCellV1 {
    /// Returns local X.
    #[must_use]
    pub const fn x(self) -> u16 {
        self.x
    }

    /// Returns local Y.
    #[must_use]
    pub const fn y(self) -> u16 {
        self.y
    }

    /// Returns local Z.
    #[must_use]
    pub const fn z(self) -> u16 {
        self.z
    }

    /// Returns the solid palette index.
    #[must_use]
    pub const fn solid_palette_index(self) -> u16 {
        self.solid_palette_index
    }

    /// Returns the fluid palette index. Zero is canonical empty.
    #[must_use]
    pub const fn fluid_palette_index(self) -> u16 {
        self.fluid_palette_index
    }
}

/// Versioned dual-layer occupancy candidate. Not a persistence snapshot schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OccupancyCandidateV1 {
    schema: &'static str,
    empty_block: StableId,
    occupied: u32,
    replaced: u32,
    rejected: u32,
    solid_palette: CompiledSolidPaletteV1,
    fluid_palette: CompiledFluidPaletteV1,
    cells: Vec<OccupancyCellV1>,
    accounting: FluidWorkAccountingV1,
}

impl OccupancyCandidateV1 {
    /// Arbitrates intents, compiles palettes, and accounts queue/cell/bytes.
    ///
    /// Intents may arrive in any order. Compilation sorts by `(y, z, x)` so
    /// discovery order cannot change candidate bytes. Dynamic flow is not run.
    ///
    /// # Errors
    ///
    /// Returns a catalog, policy, palette, encoding, or accounting error.
    #[allow(
        clippy::too_many_lines,
        reason = "candidate compilation keeps arbitration, palettes, and accounting in one transaction"
    )]
    pub fn compile(
        catalog: &ContentCatalogV1,
        context: &OccupancyArbitrationContextV1,
        mut intents: Vec<OccupancyCellIntentV1>,
        policy: BoundedFluidUpdatePolicyV1,
        palette_limits: PaletteLimitsV1,
    ) -> ContentResult<Self> {
        intents.sort_by(|left, right| {
            left.y
                .cmp(&right.y)
                .then(left.z.cmp(&right.z))
                .then(left.x.cmp(&right.x))
        });
        for pair in intents.windows(2) {
            if pair[0].x == pair[1].x && pair[0].y == pair[1].y && pair[0].z == pair[1].z {
                return Err(ContentError::InvalidOccupancyCandidate {
                    reason: "duplicate occupancy cell coordinate",
                });
            }
        }

        let mut accounting = FluidWorkAccountingV1::from_policy(policy);
        let mut occupied = 0_u32;
        let mut replaced = 0_u32;
        let mut rejected = 0_u32;
        let mut queue_depth = 0_u32;
        let mut solid_rows = Vec::new();
        let mut fluid_rows = Vec::new();
        let mut resolved = Vec::with_capacity(intents.len());

        for intent in intents {
            accounting.examine(1)?;
            let decision = arbitrate_cell(
                catalog,
                context,
                &intent.solid,
                &intent.current_fluid,
                &intent.requested_fluid,
            )?;
            let (solid, fluid, changed, queued) = match &decision {
                SolidFluidArbitrationV1::Occupied {
                    solid,
                    solid_state,
                    fluid,
                    fluid_state,
                } => {
                    occupied = saturating_add_u32(occupied, 1);
                    (
                        SolidPaletteEntryV1 {
                            block: solid.clone(),
                            state: solid_state.clone(),
                        },
                        FluidPaletteEntryV1::Fluid {
                            fluid: fluid.clone(),
                            state: *fluid_state,
                        },
                        true,
                        true,
                    )
                }
                SolidFluidArbitrationV1::ReplaceThenOccupy {
                    to_solid,
                    to_solid_state,
                    fluid,
                    fluid_state,
                    ..
                } => {
                    replaced = saturating_add_u32(replaced, 1);
                    (
                        SolidPaletteEntryV1 {
                            block: to_solid.clone(),
                            state: to_solid_state.clone(),
                        },
                        FluidPaletteEntryV1::Fluid {
                            fluid: fluid.clone(),
                            state: *fluid_state,
                        },
                        true,
                        true,
                    )
                }
                SolidFluidArbitrationV1::Rejected {
                    solid, solid_state, ..
                } => {
                    rejected = saturating_add_u32(rejected, 1);
                    (
                        SolidPaletteEntryV1 {
                            block: solid.clone(),
                            state: solid_state.clone(),
                        },
                        FluidPaletteEntryV1::Empty,
                        false,
                        false,
                    )
                }
                SolidFluidArbitrationV1::Unchanged {
                    solid,
                    solid_state,
                    fluid,
                } => (
                    SolidPaletteEntryV1 {
                        block: solid.clone(),
                        state: solid_state.clone(),
                    },
                    fluid.clone(),
                    false,
                    false,
                ),
            };
            if changed {
                accounting.change(1)?;
            }
            if queued {
                queue_depth = saturating_add_u32(queue_depth, 1);
                accounting.enqueue(queue_depth)?;
            }
            solid_rows.push(solid.clone());
            fluid_rows.push(fluid.clone());
            resolved.push((intent.x, intent.y, intent.z, solid, fluid));
        }

        solid_rows.sort_by(|left, right| left.block.cmp(&right.block));
        solid_rows.dedup();
        fluid_rows.sort_by(compare_fluid_entries);
        fluid_rows.dedup();
        let solid_palette = CompiledSolidPaletteV1::compile(catalog, solid_rows, palette_limits)?;
        let fluid_palette = CompiledFluidPaletteV1::compile(catalog, fluid_rows, palette_limits)?;
        let mut cells = Vec::with_capacity(resolved.len());
        for (x, y, z, solid, fluid) in resolved {
            let solid_palette_index = solid_index(&solid_palette, &solid)?;
            let fluid_palette_index = fluid_index(&fluid_palette, &fluid)?;
            cells.push(OccupancyCellV1 {
                x,
                y,
                z,
                solid_palette_index,
                fluid_palette_index,
            });
        }

        let candidate = Self {
            schema: SOLID_FLUID_OCCUPANCY_CANDIDATE_SCHEMA_V1,
            empty_block: context.empty_block.clone(),
            occupied,
            replaced,
            rejected,
            solid_palette,
            fluid_palette,
            cells,
            accounting,
        };
        let bytes = candidate.canonical_bytes()?;
        let byte_len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        let mut accounting = candidate.accounting;
        accounting.record_bytes(byte_len)?;
        Ok(Self {
            accounting,
            ..candidate
        })
    }

    /// Returns the frozen candidate schema identity.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }

    /// Returns occupied-in-place cell count.
    #[must_use]
    pub const fn occupied(&self) -> u32 {
        self.occupied
    }

    /// Returns replace-then-occupy cell count.
    #[must_use]
    pub const fn replaced(&self) -> u32 {
        self.replaced
    }

    /// Returns rejected cell count.
    #[must_use]
    pub const fn rejected(&self) -> u32 {
        self.rejected
    }

    /// Returns the compiled solid palette.
    #[must_use]
    pub const fn solid_palette(&self) -> &CompiledSolidPaletteV1 {
        &self.solid_palette
    }

    /// Returns the compiled fluid palette with empty at index zero.
    #[must_use]
    pub const fn fluid_palette(&self) -> &CompiledFluidPaletteV1 {
        &self.fluid_palette
    }

    /// Returns dual-layer cells in `(y, z, x)` order.
    #[must_use]
    pub fn cells(&self) -> &[OccupancyCellV1] {
        &self.cells
    }

    /// Returns the hard accounting ledger captured for this candidate.
    #[must_use]
    pub const fn accounting(&self) -> FluidWorkAccountingV1 {
        self.accounting
    }

    /// Returns canonical compact JSON for the occupancy candidate.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> ContentResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(ContentError::from)
    }

    /// Returns the canonical occupancy-candidate hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_hash(&self) -> ContentResult<CanonicalHash> {
        self.canonical_bytes().map(CanonicalHash::digest)
    }
}

/// Arbitrates one solid/fluid cell using catalog policies and frozen empty Role output.
///
/// v1 never mixes fluids, never waterlogs a non-empty solid, and only occupies
/// the frozen empty solid in place. Other replaceable solids emit a replace
/// command first. Dynamic flow is not evaluated.
///
/// # Errors
///
/// Returns an error for unknown IDs, illegal states, or unclassifiable policies.
pub fn arbitrate_cell(
    catalog: &ContentCatalogV1,
    context: &OccupancyArbitrationContextV1,
    solid: &SolidPaletteEntryV1,
    current_fluid: &FluidPaletteEntryV1,
    requested_fluid: &FluidPaletteEntryV1,
) -> ContentResult<SolidFluidArbitrationV1> {
    let definition =
        catalog
            .block(&solid.block)
            .ok_or_else(|| ContentError::InvalidSolidPaletteEntry {
                block: solid.block.clone(),
                reason: "block is absent from the validated catalog",
            })?;
    let semantics = required_semantics(definition, &solid.state)?;
    validate_fluid_entry(catalog, current_fluid)?;
    validate_fluid_entry(catalog, requested_fluid)?;

    match (current_fluid, requested_fluid) {
        (_, FluidPaletteEntryV1::Empty) => Ok(SolidFluidArbitrationV1::Unchanged {
            solid: solid.block.clone(),
            solid_state: solid.state.clone(),
            fluid: current_fluid.clone(),
        }),
        (
            FluidPaletteEntryV1::Fluid {
                fluid: current,
                state: current_state,
            },
            FluidPaletteEntryV1::Fluid {
                fluid: requested,
                state: requested_state,
            },
        ) if current == requested && current_state == requested_state => {
            Ok(SolidFluidArbitrationV1::Occupied {
                solid: solid.block.clone(),
                solid_state: solid.state.clone(),
                fluid: requested.clone(),
                fluid_state: *requested_state,
            })
        }
        (
            FluidPaletteEntryV1::Fluid { fluid: current, .. },
            FluidPaletteEntryV1::Fluid {
                fluid: requested, ..
            },
        ) if current != requested => Ok(SolidFluidArbitrationV1::Rejected {
            solid: solid.block.clone(),
            solid_state: solid.state.clone(),
            reason: OccupancyRejectV1::Mixing,
        }),
        (_, FluidPaletteEntryV1::Fluid { fluid, state }) => {
            decide_requested_fluid(context, solid, semantics, fluid.clone(), *state)
        }
    }
}

fn decide_requested_fluid(
    context: &OccupancyArbitrationContextV1,
    solid: &SolidPaletteEntryV1,
    semantics: &BlockStateSemanticsV1,
    fluid: StableId,
    fluid_state: FluidStateV1,
) -> ContentResult<SolidFluidArbitrationV1> {
    let occupancy = FluidOccupancyKindV1::classify(&semantics.fluid_occupancy)?;
    let replaceability = ReplaceabilityKindV1::classify(&semantics.replaceability)?;
    if occupancy == FluidOccupancyKindV1::Reject {
        return Ok(SolidFluidArbitrationV1::Rejected {
            solid: solid.block.clone(),
            solid_state: solid.state.clone(),
            reason: OccupancyRejectV1::PolicyRejects,
        });
    }
    if solid.block == context.empty_block {
        return Ok(SolidFluidArbitrationV1::Occupied {
            solid: solid.block.clone(),
            solid_state: solid.state.clone(),
            fluid,
            fluid_state,
        });
    }
    if replaceability == ReplaceabilityKindV1::Replaceable {
        return Ok(SolidFluidArbitrationV1::ReplaceThenOccupy {
            from_solid: solid.block.clone(),
            to_solid: context.empty_block.clone(),
            to_solid_state: context.empty_state.clone(),
            fluid,
            fluid_state,
        });
    }
    let _ = SolidOccupancyKindV1::classify(&semantics.solid_occupancy)?;
    Ok(SolidFluidArbitrationV1::Rejected {
        solid: solid.block.clone(),
        solid_state: solid.state.clone(),
        reason: OccupancyRejectV1::SolidOccupied,
    })
}

fn required_semantics<'a>(
    definition: &'a ValidatedBlockDefinitionV1,
    state: &BlockStateV1,
) -> ContentResult<&'a BlockStateSemanticsV1> {
    definition
        .semantics_for(state)
        .ok_or_else(|| ContentError::InvalidSolidPaletteEntry {
            block: definition.definition().header.stable_id.clone(),
            reason: "state is outside the block definition's explicit palette",
        })
}

fn validate_fluid_entry(
    catalog: &ContentCatalogV1,
    entry: &FluidPaletteEntryV1,
) -> ContentResult<()> {
    match entry {
        FluidPaletteEntryV1::Empty => Ok(()),
        FluidPaletteEntryV1::Fluid { fluid, .. } => {
            crate::header::validate_exact_id(fluid, "fluid", "occupancy fluid")?;
            if catalog.fluid(fluid).is_none() {
                return Err(ContentError::UnknownFluidPaletteEntry {
                    fluid: fluid.clone(),
                });
            }
            Ok(())
        }
    }
}

fn classify_policy<T: Copy>(
    id: &StableId,
    expected_kind: &'static str,
    context: &'static str,
    table: &[(&str, T)],
) -> ContentResult<T> {
    if id.kind() != expected_kind {
        return Err(ContentError::WrongIdentityKind {
            context,
            id: id.clone(),
            expected: expected_kind,
            actual: id.kind().to_owned(),
        });
    }
    let token = last_path_token(id.path());
    table
        .iter()
        .find_map(|(name, value)| (*name == token).then_some(*value))
        .ok_or_else(|| ContentError::InvalidOccupancyPolicy {
            id: id.clone(),
            reason: "path token is outside the closed v1 occupancy vocabulary",
        })
}

fn last_path_token(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn compare_fluid_entries(
    left: &FluidPaletteEntryV1,
    right: &FluidPaletteEntryV1,
) -> std::cmp::Ordering {
    match (left, right) {
        (FluidPaletteEntryV1::Empty, FluidPaletteEntryV1::Empty) => std::cmp::Ordering::Equal,
        (FluidPaletteEntryV1::Empty, FluidPaletteEntryV1::Fluid { .. }) => std::cmp::Ordering::Less,
        (FluidPaletteEntryV1::Fluid { .. }, FluidPaletteEntryV1::Empty) => {
            std::cmp::Ordering::Greater
        }
        (
            FluidPaletteEntryV1::Fluid {
                fluid: left_id,
                state: left_state,
            },
            FluidPaletteEntryV1::Fluid {
                fluid: right_id,
                state: right_state,
            },
        ) => left_id
            .cmp(right_id)
            .then_with(|| left_state.level.get().cmp(&right_state.level.get()))
            .then_with(|| left_state.flow.cmp(&right_state.flow)),
    }
}

fn solid_index(
    palette: &CompiledSolidPaletteV1,
    entry: &SolidPaletteEntryV1,
) -> ContentResult<u16> {
    palette
        .entries()
        .iter()
        .position(|candidate| {
            candidate.block() == &entry.block && candidate.state() == &entry.state
        })
        .and_then(|index| u16::try_from(index).ok())
        .ok_or(ContentError::InvalidOccupancyCandidate {
            reason: "solid palette lost an arbitrated entry",
        })
}

fn fluid_index(
    palette: &CompiledFluidPaletteV1,
    entry: &FluidPaletteEntryV1,
) -> ContentResult<u16> {
    let index = match entry {
        FluidPaletteEntryV1::Empty => palette
            .entries()
            .iter()
            .position(crate::CompiledFluidPaletteEntryV1::is_empty),
        FluidPaletteEntryV1::Fluid { fluid, state } => {
            palette.entries().iter().position(|candidate| {
                candidate.fluid() == Some(fluid) && candidate.state() == Some(state)
            })
        }
    };
    index.and_then(|value| u16::try_from(value).ok()).ok_or(
        ContentError::InvalidOccupancyCandidate {
            reason: "fluid palette lost an arbitrated entry",
        },
    )
}

fn saturating_add_u32(left: u32, right: u32) -> u32 {
    left.saturating_add(right)
}

fn enforce_account(resource: &'static str, actual: u32, limit: u32) -> ContentResult<()> {
    if actual > limit {
        return Err(ContentError::LimitExceeded {
            resource,
            actual: usize::try_from(actual).unwrap_or(usize::MAX),
            limit: usize::try_from(limit).unwrap_or(usize::MAX),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::last_path_token;

    #[test]
    fn last_path_token_uses_the_final_segment() {
        assert_eq!(last_path_token("direct"), "direct");
        assert_eq!(last_path_token("family/direct"), "direct");
    }
}
